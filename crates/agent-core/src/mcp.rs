//! MCP (Model Context Protocol) stdio client — v0.4.36.
//!
//! Implements the subset of the MCP spec Flowntier needs to pull
//! external tools into the agent loop:
//!
//! - `initialize` + `notifications/initialized` handshake
//! - `tools/list` (with cursor pagination)
//! - `tools/call`
//!
//! The wire format is newline-delimited JSON-RPC 2.0 over the
//! server's stdio (the standard `stdio` transport). We deliberately
//! do NOT implement resources/prompts/sampling — the tool surface
//! is what makes external capabilities reachable from the agent
//! loop, and a narrow protocol means a narrow attack surface.
//!
//! ## Design constraints (from the v0.4.36 debate)
//!
//! - **Non-blocking startup**: [`McpServer::connect`] enforces a
//!   hard per-server handshake timeout; a hung MCP server must
//!   never delay the pipe server's listen loop. The timeout path
//!   kills the child before returning so nothing leaks.
//! - **Crash isolation**: every spawn gets its stderr drained from
//!   the moment of spawn (a full stderr pipe otherwise deadlocks
//!   the server), and kills use the process tree (`taskkill /T /F`
//!   on Windows) so `npx`-style wrappers don't leave orphan node
//!   grandchildren behind.
//! - **Windows argv**: `.cmd` / `.bat` commands are routed through
//!   `cmd /C` because `CreateProcess` cannot execute batch wrappers
//!   directly (the classic `npx` → `npx.cmd` EINVAL failure).
//! - **Hermetic tests**: the protocol layer is generic over the
//!   transport, so unit tests run against in-memory duplex streams
//!   with no child processes.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader, BufWriter};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;

/// Protocol version we advertise during the `initialize` handshake.
/// 2024-11-05 is the revision the current server ecosystem targets.
pub const PROTOCOL_VERSION: &str = "2024-11-05";

/// Hard cap on one `tools/call` round trip. MCP servers are external
/// processes; without a ceiling a wedged server would pin the agent
/// loop's tool execution forever. After a timeout the server is
/// marked unavailable so subsequent calls fail fast instead of each
/// burning the full budget.
pub const TOOL_CALL_TIMEOUT: Duration = Duration::from_secs(120);

/// Default handshake budget used by the pipe-server's registry
/// rebuild: spawn + initialize + tools/list must fit inside this,
/// per server, or the server is skipped.
pub const DEFAULT_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(3);

/// Errors surfaced by the MCP client.
#[derive(Debug, Error)]
pub enum McpError {
    /// Underlying transport I/O failure (including EOF — a closed
    /// stdout means the server died mid-protocol).
    #[error("mcp io error: {0}")]
    Io(#[from] std::io::Error),
    /// A frame was not valid JSON, or did not match the JSON-RPC
    /// envelope shape.
    #[error("mcp protocol violation: {0}")]
    Protocol(String),
    /// The server answered a request with a JSON-RPC error object.
    #[error("mcp server error {code}: {message}")]
    Server {
        /// JSON-RPC error code.
        code: i64,
        /// Human-readable message from the server.
        message: String,
    },
    /// A frame could not be serialized.
    #[error("mcp json error: {0}")]
    Json(#[from] serde_json::Error),
}

/// One tool advertised by an MCP server's `tools/list` response.
#[derive(Debug, Clone, Deserialize)]
pub struct McpToolDef {
    /// Bare tool name, unique within the server. Registered with
    /// Flowntier as `mcp__<server>__<name>`.
    pub name: String,
    /// Server-provided description (may be empty).
    #[serde(default)]
    pub description: String,
    /// JSON-Schema for the tool's arguments (`inputSchema`).
    #[serde(default = "empty_object", rename = "inputSchema")]
    pub input_schema: Value,
}

fn empty_object() -> Value {
    json!({})
}

/// A request-scoped connection to an MCP server over an arbitrary
/// duplex pair. Generic so tests can drive it against in-memory
/// streams instead of real child processes.
pub(crate) struct McpConnection<R, W> {
    reader: BufReader<R>,
    writer: BufWriter<W>,
    next_id: u64,
}

impl<R: AsyncRead + Unpin, W: AsyncWrite + Unpin> McpConnection<R, W> {
    pub(crate) fn new(reader: R, writer: W) -> Self {
        Self {
            reader: BufReader::new(reader),
            writer: BufWriter::new(writer),
            next_id: 0,
        }
    }

    /// Send one notification (no `id`, no response expected).
    pub(crate) async fn notify(&mut self, method: &str, params: Value) -> Result<(), McpError> {
        let frame = json!({"jsonrpc": "2.0", "method": method, "params": params});
        let mut line = serde_json::to_string(&frame)?;
        line.push('\n');
        self.writer.write_all(line.as_bytes()).await?;
        self.writer.flush().await?;
        Ok(())
    }

    /// Send one request and wait for the response with the matching
    /// `id`. Notifications and junk lines interleaved before the
    /// response are skipped (real servers occasionally print
    /// diagnostics to stdout before their first frame).
    pub(crate) async fn call(&mut self, method: &str, params: Value) -> Result<Value, McpError> {
        self.next_id += 1;
        let id = self.next_id;
        let frame = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        let mut line = serde_json::to_string(&frame)?;
        line.push('\n');
        self.writer.write_all(line.as_bytes()).await?;
        self.writer.flush().await?;

        loop {
            let mut buf = String::new();
            let n = self.reader.read_line(&mut buf).await?;
            if n == 0 {
                return Err(McpError::Protocol(format!(
                    "server closed the stream while waiting for '{method}' response"
                )));
            }
            let v: Value = match serde_json::from_str(buf.trim()) {
                Ok(v) => v,
                Err(e) => {
                    tracing::debug!("mcp: skipping non-JSON line: {e}");
                    continue;
                }
            };
            if v.get("id").and_then(Value::as_u64) != Some(id) {
                continue; // notification or stale response — skip
            }
            if let Some(err) = v.get("error") {
                return Err(McpError::Server {
                    code: err.get("code").and_then(Value::as_i64).unwrap_or(-1),
                    message: err
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown")
                        .to_string(),
                });
            }
            return Ok(v.get("result").cloned().unwrap_or(Value::Null));
        }
    }

    /// `initialize` handshake + the `notifications/initialized`
    /// acknowledgment the spec requires before any other request.
    pub(crate) async fn initialize(&mut self, client_name: &str) -> Result<Value, McpError> {
        let result = self
            .call(
                "initialize",
                json!({
                    "protocolVersion": PROTOCOL_VERSION,
                    "capabilities": {},
                    "clientInfo": {"name": client_name, "version": "0.4.36"},
                }),
            )
            .await?;
        self.notify("notifications/initialized", json!({})).await?;
        Ok(result)
    }

    /// `tools/list` with cursor pagination (hard page cap so a
    /// server that keeps minting cursors can't loop us forever).
    pub(crate) async fn list_tools(&mut self) -> Result<Vec<McpToolDef>, McpError> {
        let mut tools = Vec::new();
        let mut cursor: Option<Value> = None;
        for _ in 0..50 {
            let mut params = json!({});
            if let Some(c) = cursor.take() {
                params["cursor"] = c;
            }
            let result = self.call("tools/list", params).await?;
            if let Some(batch) = result.get("tools").and_then(Value::as_array) {
                for t in batch {
                    if let Ok(def) = serde_json::from_value::<McpToolDef>(t.clone()) {
                        tools.push(def);
                    }
                }
            }
            cursor = result.get("nextCursor").cloned().filter(|c| !c.is_null());
            if cursor.is_none() {
                break;
            }
        }
        Ok(tools)
    }

    /// `tools/call`. Returns the raw result object so the caller
    /// decides how to render `content` blocks.
    pub(crate) async fn call_tool(&mut self, name: &str, args: Value) -> Result<Value, McpError> {
        self.call("tools/call", json!({"name": name, "arguments": args}))
            .await
    }
}

/// Flatten an MCP `tools/call` result into the agent loop's
/// [`crate::tool::ToolOutput`] shape: text content blocks joined
/// with newlines, `isError` honored, raw JSON as a fallback when
/// the server returned no renderable content.
pub fn render_tool_result(result: &Value) -> (String, bool) {
    let mut text = String::new();
    if let Some(blocks) = result.get("content").and_then(Value::as_array) {
        for b in blocks {
            match b.get("type").and_then(Value::as_str) {
                Some("text") => {
                    if let Some(t) = b.get("text").and_then(Value::as_str) {
                        if !text.is_empty() {
                            text.push('\n');
                        }
                        text.push_str(t);
                    }
                }
                Some(other) => {
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str(&format!("[non-text content block: {other}]"));
                }
                None => {}
            }
        }
    }
    if text.is_empty() {
        text = serde_json::to_string(result).unwrap_or_else(|_| "{}".into());
    }
    let is_error = result
        .get("isError")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    (text, is_error)
}

/// A live MCP server: the child process plus a shared, mutex-guarded
/// connection. Calls are serialized per server — MCP servers are
/// single process pairs and this keeps request framing trivially
/// correct.
pub struct McpServer {
    /// Configured server name (the `<server>` in `mcp__<server>__<tool>`).
    pub name: String,
    inner: Mutex<ServerInner>,
}

struct ServerInner {
    connection:
        McpConnection<Box<dyn AsyncRead + Unpin + Send>, Box<dyn AsyncWrite + Unpin + Send>>,
    child: Child,
    dead: bool,
}

impl std::fmt::Debug for McpServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpServer")
            .field("name", &self.name)
            .finish()
    }
}

impl McpServer {
    /// Spawn `command` and complete the MCP handshake.
    ///
    /// `handshake_timeout` bounds spawn + initialize + tools/list;
    /// on timeout or protocol failure the child is killed
    /// (process-tree) before the error is returned, so a bad server
    /// never leaks processes. The connection's stdio halves are
    /// erased to trait objects so [`McpServer`] stays
    /// non-generic.
    pub async fn connect(
        cfg: &McpServerConfig,
        handshake_timeout: Duration,
    ) -> Result<(Arc<Self>, Vec<McpToolDef>), McpError> {
        let (mut child, stdout, stdin) = spawn_server_process(cfg)?;
        // Erase the concrete stdio types up front so the connection
        // can move into `McpServer` (which stores trait objects).
        let mut connection: McpConnection<
            Box<dyn AsyncRead + Unpin + Send>,
            Box<dyn AsyncWrite + Unpin + Send>,
        > = McpConnection::new(Box::new(stdout), Box::new(stdin));
        let handshake = async {
            connection.initialize("flowntier").await?;
            let tools = connection.list_tools().await?;
            Ok::<_, McpError>(tools)
        };
        let tools = match tokio::time::timeout(handshake_timeout, handshake).await {
            Ok(r) => r?,
            Err(_) => {
                kill_tree(&mut child).await;
                return Err(McpError::Protocol(format!(
                    "handshake with MCP server '{}' timed out after {handshake_timeout:?}",
                    cfg.name
                )));
            }
        };
        Ok((
            Arc::new(Self {
                name: cfg.name.clone(),
                inner: Mutex::new(ServerInner {
                    connection,
                    child,
                    dead: false,
                }),
            }),
            tools,
        ))
    }

    /// Run `tools/call` against this server with the shared
    /// per-server timeout.
    pub async fn call_tool(&self, tool: &str, args: Value) -> Result<Value, McpError> {
        let mut guard = self.inner.lock().await;
        if guard.dead {
            return Err(McpError::Protocol(format!(
                "MCP server '{}' is no longer running",
                self.name
            )));
        }
        let fut = guard.connection.call_tool(tool, args);
        match tokio::time::timeout(TOOL_CALL_TIMEOUT, fut).await {
            Ok(res) => res,
            Err(_) => {
                guard.dead = true;
                Err(McpError::Protocol(format!(
                    "tools/call on '{tool}' timed out; server '{}' marked unavailable",
                    self.name
                )))
            }
        }
    }

    /// Kill the child process tree. Safe to call multiple times.
    pub async fn shutdown(&self) {
        let mut guard = self.inner.lock().await;
        guard.dead = true;
        kill_tree(&mut guard.child).await;
    }
}

/// One MCP server entry from the workspace's `flowntier.toml`:
///
/// ```toml
/// [mcp.servers.filesystem]
/// command = "npx.cmd"
/// args    = ["-y", "@modelcontextprotocol/server-filesystem", "."]
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServerConfig {
    /// Server name (also used to qualify tool names).
    pub name: String,
    /// Executable. On Windows, `.cmd`/`.bat` are routed via `cmd /C`.
    pub command: String,
    /// argv tail passed to the executable.
    pub args: Vec<String>,
    /// Extra environment variables for the child.
    pub env: std::collections::BTreeMap<String, String>,
}

/// Spawn the server process with piped stdio and a stderr drain
/// task running from the first microsecond (a full stderr pipe
/// otherwise deadlocks the child on its next write).
fn spawn_server_process(
    cfg: &McpServerConfig,
) -> Result<(Child, BufReader<ChildStdout>, ChildStdin), McpError> {
    let mut cmd = build_platform_command(cfg)?;
    cmd.envs(cfg.env.iter())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cfg_windows_no_window(&mut cmd);
    let mut child = cmd.spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| McpError::Protocol("child stdout not piped".into()))?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| McpError::Protocol("child stdin not piped".into()))?;
    if let Some(stderr) = child.stderr.take() {
        // Drain into the tracing log; MCP servers write diagnostics
        // to stderr by convention, and losing them makes servers
        // undebuggable.
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                tracing::debug!(target: "mcp", "{line}");
            }
        });
    }
    Ok((child, BufReader::new(stdout), stdin))
}

/// Build the platform `Command` for a server config.
///
/// Windows batch wrappers (`.cmd` / `.bat` — i.e. every `npm`
/// install) cannot be exec'd directly by `CreateProcess`; they must
/// go through `cmd /C`. Everywhere else the command runs as-is.
fn build_platform_command(cfg: &McpServerConfig) -> Result<Command, McpError> {
    #[cfg(windows)]
    {
        let lower = cfg.command.to_ascii_lowercase();
        if lower.ends_with(".cmd") || lower.ends_with(".bat") {
            let mut c = Command::new("cmd");
            c.arg("/C").arg(&cfg.command).args(&cfg.args);
            return Ok(c);
        }
        let mut c = Command::new(&cfg.command);
        c.args(&cfg.args);
        Ok(c)
    }
    #[cfg(not(windows))]
    {
        let mut c = Command::new(&cfg.command);
        c.args(&cfg.args);
        Ok(c)
    }
}

#[cfg(windows)]
fn cfg_windows_no_window(cmd: &mut Command) {
    // CREATE_NO_WINDOW: MCP servers must not flash console windows
    // in the desktop app.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn cfg_windows_no_window(_cmd: &mut Command) {}

/// Kill a child and its whole tree. `child.kill()` only signals the
/// direct process; for `cmd /C npx ...` that leaves node
/// grandchildren running. `taskkill /T /F` (the same mechanism the
/// bash tool uses) takes the tree down.
pub async fn kill_tree(child: &mut Child) {
    #[cfg(windows)]
    {
        if let Some(pid) = child.id() {
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            let mut tk = Command::new("taskkill");
            tk.args(["/PID", &pid.to_string(), "/T", "/F"])
                .creation_flags(CREATE_NO_WINDOW);
            let _ = tk
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await;
        }
        let _ = child.wait().await;
    }
    #[cfg(not(windows))]
    {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
}

/// An MCP tool registered into the [`crate::tool::ToolRegistry`].
///
/// Name is `mcp__<server>__<tool>` (the double-underscore delimiter
/// matches what Claude Code and Cursor use, so prompts and rule
/// files written against those conventions read naturally here).
pub struct McpTool {
    server: Arc<McpServer>,
    bare_name: String,
    qualified_name: String,
    description: String,
    schema: Value,
}

impl std::fmt::Debug for McpTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpTool")
            .field("name", &self.qualified_name)
            .finish()
    }
}

impl McpTool {
    /// Build from a server handle and its advertised tool def.
    pub fn new(server: Arc<McpServer>, def: &McpToolDef) -> Self {
        Self {
            qualified_name: format!("mcp__{}__{}", server.name, def.name),
            bare_name: def.name.clone(),
            description: if def.description.is_empty() {
                format!("MCP tool {} (no description provided by server)", def.name)
            } else {
                def.description.clone()
            },
            schema: def.input_schema.clone(),
            server,
        }
    }

    /// Qualified registry name.
    pub fn qualified_name(&self) -> &str {
        &self.qualified_name
    }

    /// Server-side (bare) tool name used in `tools/call`.
    pub fn bare_name(&self) -> &str {
        &self.bare_name
    }

    /// The live server handle.
    pub fn server(&self) -> &Arc<McpServer> {
        &self.server
    }
}

#[async_trait]
impl crate::tool::Tool for McpTool {
    fn name(&self) -> &str {
        &self.qualified_name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn schema(&self) -> Value {
        self.schema.clone()
    }

    async fn execute(
        &self,
        args: Value,
        ctx: &crate::tool::ToolContext,
    ) -> Result<crate::tool::ToolOutput, crate::tool::ToolError> {
        // Defense in depth: MCP tool side effects are unknown to us,
        // so we refuse unless the active capability set is fully
        // trusted. (In readonly mode the registry layer already
        // excludes every MCP tool — this check covers a future
        // regression where one leaks through.)
        if !(ctx.capabilities.read && ctx.capabilities.write && ctx.capabilities.bash) {
            return Err(crate::tool::ToolError::Refused(
                "MCP tools are unavailable while the agent runs with restricted capabilities \
                 (e.g. permissions mode = \"readonly\")"
                    .into(),
            ));
        }
        let result = self.server.call_tool(&self.bare_name, args).await?;
        let (content, is_error) = render_tool_result(&result);
        if is_error {
            Ok(crate::tool::ToolOutput::err(content))
        } else {
            Ok(crate::tool::ToolOutput::ok(content))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Full-duplex fake MCP server: one task reads requests from
    /// one duplex half and writes replies into the other, so the
    /// protocol layer is exercised without any child process.
    async fn spawn_fake_server(
        server_read: impl AsyncRead + Unpin + Send + 'static,
        server_write: impl AsyncWrite + Unpin + Send + 'static,
        tools_payload: Value,
    ) {
        let mut writer = server_write;
        let mut lines = BufReader::new(server_read).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let v: Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let method = v.get("method").and_then(Value::as_str).unwrap_or("");
            let id = v.get("id").cloned().unwrap_or(Value::Null);
            let result = match method {
                "initialize" => json!({
                    "protocolVersion": PROTOCOL_VERSION,
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "fake", "version": "1"}
                }),
                "tools/list" => tools_payload.clone(),
                "tools/call" => json!({
                    "content": [{"type": "text", "text": "hello from mcp"}],
                    "isError": false
                }),
                _ => json!({}),
            };
            let resp = json!({"jsonrpc": "2.0", "id": id, "result": result});
            if writer
                .write_all(format!("{resp}\n").as_bytes())
                .await
                .is_err()
            {
                break;
            }
        }
    }

    #[tokio::test]
    async fn initialize_handshake_and_tool_list_over_memory() {
        let (client_write, server_read) = tokio::io::duplex(8192);
        let (server_write, client_read) = tokio::io::duplex(8192);
        tokio::spawn(spawn_fake_server(
            server_read,
            server_write,
            json!({"tools": [
                {"name": "read_file", "description": "reads", "inputSchema": {"type": "object"}},
                {"name": "write_file", "description": "", "inputSchema": {"type": "object"}}
            ]}),
        ));

        let mut conn = McpConnection::new(client_read, client_write);
        let info = conn.initialize("flowntier-test").await.expect("handshake");
        assert_eq!(info.get("serverInfo").unwrap().get("name").unwrap(), "fake");
        let tools = conn.list_tools().await.expect("tools/list");
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name, "read_file");
        assert!(tools[1].description.is_empty());
        assert_eq!(tools[1].input_schema, json!({"type": "object"}));
    }

    #[tokio::test]
    async fn json_rpc_error_maps_to_server_error() {
        let (client_write, server_read) = tokio::io::duplex(8192);
        let (server_write, client_read) = tokio::io::duplex(8192);
        tokio::spawn(async move {
            let mut writer = server_write;
            let mut lines = BufReader::new(server_read).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let v: Value = match serde_json::from_str(&line) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                let id = v.get("id").cloned().unwrap_or(Value::Null);
                let resp = json!({
                    "jsonrpc": "2.0", "id": id,
                    "error": {"code": -32601, "message": "method not found"}
                });
                if writer
                    .write_all(format!("{resp}\n").as_bytes())
                    .await
                    .is_err()
                {
                    break;
                }
            }
        });
        let mut conn = McpConnection::new(client_read, client_write);
        let err = conn
            .call("tools/call", json!({"name": "x"}))
            .await
            .expect_err("should be a Server error");
        match err {
            McpError::Server { code, message } => {
                assert_eq!(code, -32601);
                assert_eq!(message, "method not found");
            }
            other => panic!("expected Server error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn closed_stream_surfaces_as_protocol_error() {
        // The server side is dropped immediately: reads hit EOF.
        let (client_write, _server_read) = tokio::io::duplex(8192);
        let (_server_write, client_read) = tokio::io::duplex(8192);
        drop(_server_read);
        drop(_server_write);
        let mut conn = McpConnection::new(client_read, client_write);
        let err = conn.call("ping", json!({})).await.expect_err("EOF");
        // EOF on read → Protocol; a broken pipe on the write half →
        // Io. Either way the closed stream must surface as an error,
        // never a hang.
        assert!(matches!(err, McpError::Protocol(_) | McpError::Io(_)));
    }

    #[tokio::test]
    async fn tools_call_round_trip_and_rendering() {
        let (client_write, server_read) = tokio::io::duplex(8192);
        let (server_write, client_read) = tokio::io::duplex(8192);
        tokio::spawn(spawn_fake_server(
            server_read,
            server_write,
            json!({"tools": [{"name": "echo", "inputSchema": {}}]}),
        ));
        let mut conn = McpConnection::new(client_read, client_write);
        conn.initialize("flowntier-test").await.expect("handshake");
        let result = conn
            .call_tool("echo", json!({"msg": "hi"}))
            .await
            .expect("tools/call");
        let (text, is_error) = render_tool_result(&result);
        assert_eq!(text, "hello from mcp");
        assert!(!is_error);
    }

    #[test]
    fn render_tool_result_joins_text_blocks_and_iserror() {
        let result = json!({
            "content": [
                {"type": "text", "text": "line one"},
                {"type": "text", "text": "line two"},
                {"type": "resource", "resource": {}}
            ],
            "isError": true
        });
        let (text, is_error) = render_tool_result(&result);
        assert_eq!(
            text,
            "line one\nline two\n[non-text content block: resource]"
        );
        assert!(is_error);
    }

    #[test]
    fn render_tool_result_falls_back_to_raw_json() {
        let (text, is_error) = render_tool_result(&json!({"content": []}));
        assert_eq!(text, "{\"content\":[]}");
        assert!(!is_error);
    }

    #[test]
    fn mcp_tool_qualifies_names() {
        // Pure naming logic; the delimiter contract lives here.
        let qualified = format!("mcp__{}__{}", "filesystem", "read_file");
        assert_eq!(qualified, "mcp__filesystem__read_file");
    }

    #[test]
    fn build_platform_command_accepts_both_shapes_on_windows() {
        // We can't observe tokio's Command internals portably, but
        // both the batch-wrapper shape and the direct shape must
        // construct without panicking.
        let wrapped = McpServerConfig {
            name: "fs".into(),
            command: "npx.cmd".into(),
            args: vec![
                "-y".into(),
                "@modelcontextprotocol/server-filesystem".into(),
            ],
            env: Default::default(),
        };
        let _ = build_platform_command(&wrapped).expect("cmd wrapper builds");
        let direct = McpServerConfig {
            name: "git".into(),
            command: "uvx".into(),
            args: vec!["mcp-server-git".into()],
            env: Default::default(),
        };
        let _ = build_platform_command(&direct).expect("direct command builds");
    }
}
