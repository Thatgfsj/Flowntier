//! RPC method dispatcher.
//!
//! Maps `(method, path)` strings to an async handler. Handlers
//! receive the request body and return a JSON body or a tuple
//! `(status, body)`.
//!
//! The set of registered handlers mirrors what the Python
//! runtime used to serve under FastAPI. Only a minimal subset is
//! implemented here — enough to unblock the Tauri client; new
//! methods land as they're ported.

use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

use crate::protocol::{codes, RpcRequest, RpcResponse};

/// A handler: takes the request body, returns `(status, body)`.
pub type Handler = Arc<dyn Fn(Value) -> HandlerFuture + Send + Sync>;
pub type HandlerFuture =
    std::pin::Pin<Box<dyn std::future::Future<Output = Result<(u16, Value), String>> + Send>>;

/// A registry of RPC handlers keyed by `(method, path)`.
#[derive(Default, Clone)]
pub struct Dispatcher {
    /// (HTTP method, path) -> handler. The pair is what HTTP itself
    /// uses to identify a route; using it as the key here means a
    /// GET and a PUT on the same path can coexist (e.g. `GET
    /// /api/router/roles` reads the role list, `PUT /api/router/roles`
    /// overwrites it).
    handlers: HashMap<(String, String), Handler>,
}

impl std::fmt::Debug for Dispatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Dispatcher")
            .field(
                "routes",
                &self
                    .handlers
                    .keys()
                    .map(|(m, p)| format!("{m} {p}"))
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl Dispatcher {
    /// New empty dispatcher.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a handler for `(method, path)`.
    ///
    /// `method` is the HTTP verb (`GET`, `POST`, `PUT`, `PATCH`,
    /// `DELETE`, ...). The caller is responsible for keeping
    /// `(method, path)` unique; registering the same pair twice
    /// overwrites the previous handler, which is usually a bug
    /// in the caller — see `register_all` for the canonical
    /// endpoint list.
    ///
    /// Path patterns: a path containing `{name}` (one per segment)
    /// is a placeholder. During dispatch, the placeholder matches
    /// any non-empty segment of the incoming request path and the
    /// extracted value is written into the request body under the
    /// same key (e.g. `{name}` -> body["name"]). This lets
    /// handlers be registered once for an entire collection of
    /// concrete paths (PUT /api/settings/secrets/{name} matches
    /// /api/settings/secrets/OPENAI_API_KEY, etc.).
    pub fn register<F>(&mut self, method: impl Into<String>, path: impl Into<String>, f: F)
    where
        F: Fn(Value) -> HandlerFuture + Send + Sync + 'static,
    {
        self.handlers
            .insert((method.into().to_uppercase(), path.into()), Arc::new(f));
    }

    /// List registered routes as `METHOD path` pairs, sorted
    /// deterministically. Useful for diagnostics.
    pub fn methods(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .handlers
            .keys()
            .map(|(m, p)| format!("{m} {p}"))
            .collect();
        v.sort();
        v
    }

    /// Dispatch an RPC request. Looks up the handler by
    /// `(req.method, req.params.path)`.
    ///
    /// Lookup algorithm:
    ///   1. Exact match — fastest path.
    ///   2. Pattern match — scan registered paths with the
    ///      same method, find the first where the placeholder
    ///      pattern matches the incoming path. Extracted
    ///      placeholders are injected into the request body
    ///      under the same key.
    pub async fn dispatch(&self, req_id: u64, req: RpcRequest) -> RpcResponse {
        let method = req.method.to_uppercase();
        let path = req.params.path.clone();
        let mut body = req.params.body.unwrap_or(Value::Null);
        tracing::info!(target: "dispatcher", req_id = req_id, method = %method, path = %path, "[TRACE] dispatch: entering");

        // v0.4.21 (event 000064 follow-up): strip `?query` from
        // the path so handlers registered on the bare path
        // (`/api/tasks`) still match when the caller appends
        // query parameters (`/api/tasks?wf_id=...`). Also parse
        // the query into the body so handlers can read params
        // via `body.get("wf_id")` instead of having to re-parse
        // the query themselves.
        let (path, query) = match path.split_once('?') {
            Some((p, q)) => (p.to_string(), q),
            None => (path.clone(), ""),
        };
        if !query.is_empty() {
            if let Value::Object(ref mut map) = body {
                for pair in query.split('&') {
                    if let Some((k, v)) = pair.split_once('=') {
                        map.insert(
                            k.to_string(),
                            Value::String(urldecode(v).unwrap_or_else(|| v.to_string())),
                        );
                    } else if !pair.is_empty() {
                        map.insert(pair.to_string(), Value::Bool(true));
                    }
                }
            } else {
                let mut map = serde_json::Map::new();
                for pair in query.split('&') {
                    if let Some((k, v)) = pair.split_once('=') {
                        map.insert(
                            k.to_string(),
                            Value::String(urldecode(v).unwrap_or_else(|| v.to_string())),
                        );
                    } else if !pair.is_empty() {
                        map.insert(pair.to_string(), Value::Bool(true));
                    }
                }
                body = Value::Object(map);
            }
        }

        // 1. Exact match.
        if let Some(handler) = self.handlers.get(&(method.clone(), path.clone())) {
            tracing::info!(target: "dispatcher", req_id = req_id, method = %method, path = %path, "[TRACE] dispatch: EXACT MATCH found, calling handler");
            return match handler(body).await {
                Ok((status, b)) => {
                    tracing::info!(target: "dispatcher", req_id = req_id, status = status, "[TRACE] dispatch: handler returned OK");
                    RpcResponse::status(req_id, status, b)
                }
                Err(e) => {
                    tracing::error!(target: "dispatcher", req_id = req_id, error = %e, "[TRACE] dispatch: handler returned Err");
                    RpcResponse::err(req_id, codes::INTERNAL, e)
                }
            };
        }
        tracing::debug!(target: "dispatcher", req_id = req_id, method = %method, path = %path, "[TRACE] dispatch: no exact match, trying pattern match");

        // 2. Pattern match.
        let incoming_segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        for ((registered_method, registered_path), handler) in &self.handlers {
            if *registered_method != method {
                continue;
            }
            let pattern_segments: Vec<&str> = registered_path
                .split('/')
                .filter(|s| !s.is_empty())
                .collect();

            // v0.4.30 (audit 000130): a `{name+}` placeholder is
            // a greedy wildcard — it consumes one or more
            // consecutive incoming segments. This lets the
            // secret-name routes handle values like
            // `flowntier/minimax` that contain a `/` (the secret
            // namespace itself does, and so do
            // `/api/settings/secrets/{name+}/reveal`-style
            // patterns where the wildcard sits in the middle).
            //
            // Strategy: do a 2-pointer scan over pattern vs
            // incoming. When the pattern segment is a wildcard
            // (`{name+}`), find the longest tail of incoming
            // segments that lets the rest of the pattern still
            // match (greedy from the left).
            let mut placeholder_values: Vec<(String, String)> = Vec::new();
            let mut matched = true;
            let mut inc_idx: usize = 0;
            let mut pc_idx: usize = 0;
            while pc_idx < pattern_segments.len() {
                let pat = pattern_segments[pc_idx];
                if pat.starts_with('{') && pat.ends_with("+}") {
                    // Find the longest prefix of the remaining
                    // incoming segments such that the rest of
                    // the pattern (after this wildcard) still
                    // matches exactly.
                    let name = &pat[1..pat.len() - 2]; // strip { and +}
                    let remaining_pattern = pattern_segments.len() - pc_idx - 1;
                    let remaining_incoming = incoming_segments.len() - inc_idx;
                    if remaining_incoming <= remaining_pattern {
                        matched = false;
                        break;
                    }
                    // Max number of incoming segments the
                    // wildcard can consume = remaining_incoming
                    // - remaining_pattern. Greedy = take them
                    // all.
                    let consume = remaining_incoming - remaining_pattern;
                    if consume == 0 {
                        // `{name+}` requires at least one
                        // segment.
                        matched = false;
                        break;
                    }
                    let value: String = incoming_segments[inc_idx..inc_idx + consume].join("/");
                    placeholder_values.push((name.to_string(), value));
                    inc_idx += consume;
                    pc_idx += 1;
                    continue;
                }
                if inc_idx >= incoming_segments.len() {
                    matched = false;
                    break;
                }
                if let Some(name) = pat.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
                    if incoming_segments[inc_idx].is_empty() {
                        matched = false;
                        break;
                    }
                    placeholder_values
                        .push((name.to_string(), incoming_segments[inc_idx].to_string()));
                    inc_idx += 1;
                    pc_idx += 1;
                } else if pat == incoming_segments[inc_idx] {
                    inc_idx += 1;
                    pc_idx += 1;
                } else {
                    matched = false;
                    break;
                }
            }
            if matched && inc_idx != incoming_segments.len() {
                matched = false;
            }
            if matched {
                tracing::info!(target: "dispatcher", req_id = req_id, method = %method, path = %registered_path, "[TRACE] dispatch: PATTERN MATCH found, calling handler");
                // Inject placeholders into the body so handlers
                // can access them via `body.get("name")` etc.
                if let Value::Object(ref mut map) = body {
                    for (name, value) in &placeholder_values {
                        map.insert(name.clone(), Value::String(value.clone()));
                    }
                } else {
                    let mut map = serde_json::Map::new();
                    for (name, value) in &placeholder_values {
                        map.insert(name.clone(), Value::String(value.clone()));
                    }
                    body = Value::Object(map);
                }
                return match handler(body).await {
                    Ok((status, b)) => {
                        tracing::info!(target: "dispatcher", req_id = req_id, status = status, "[TRACE] dispatch: pattern handler returned OK");
                        RpcResponse::status(req_id, status, b)
                    }
                    Err(e) => {
                        tracing::error!(target: "dispatcher", req_id = req_id, error = %e, "[TRACE] dispatch: pattern handler returned Err");
                        RpcResponse::err(req_id, codes::INTERNAL, e)
                    }
                };
            }
        }

        tracing::warn!(target: "dispatcher", req_id = req_id, method = %method, path = %path, "[TRACE] dispatch: NO HANDLER FOUND — returning NOT_FOUND");
        RpcResponse::err(
            req_id,
            codes::NOT_FOUND,
            format!("no handler registered for {method} {path}"),
        )
    }
}

/// v0.4.21: minimal URL-decode for query-string values. Handles
/// `%XX` hex escapes plus `+` → space (form-encoding style).
/// Used by [Dispatcher::dispatch] to expose query params as
/// string values in the request body.
fn urldecode(s: &str) -> Option<String> {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b'+' => {
                out.push(' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hi = hex_digit(bytes[i + 1])?;
                let lo = hex_digit(bytes[i + 2])?;
                out.push((hi * 16 + lo) as char);
                i += 3;
            }
            b => {
                out.push(b as char);
                i += 1;
            }
        }
    }
    Some(out)
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::RpcParams;

    fn req(method: &str, path: &str) -> RpcRequest {
        RpcRequest {
            jsonrpc: "2.0".into(),
            id: 1,
            method: method.into(),
            params: RpcParams {
                path: path.into(),
                body: None,
            },
        }
    }

    #[tokio::test]
    async fn dispatches_known_method() {
        let mut d = Dispatcher::new();
        d.register("GET", "/api/ping", |_body| {
            Box::pin(async { Ok((200, serde_json::json!({"pong": true}))) })
        });
        let resp = d.dispatch(1, req("GET", "/api/ping")).await;
        let r = resp.result.unwrap();
        assert_eq!(r.status, 200);
        assert_eq!(r.body["pong"], serde_json::json!(true));
    }

    #[tokio::test]
    async fn unknown_method_is_not_found() {
        let d = Dispatcher::new();
        let resp = d.dispatch(2, req("GET", "/nope")).await;
        let e = resp.error.unwrap();
        assert_eq!(e.code, codes::NOT_FOUND);
    }

    #[tokio::test]
    async fn get_and_put_on_same_path_coexist() {
        // The v0.3 fix: previously the dispatcher only keyed on
        // path, so a second register on the same path silently
        // overwrote the first. With (method, path) as the key,
        // GET and PUT handlers can both be registered.
        let mut d = Dispatcher::new();
        d.register("GET", "/api/router/roles", |_body| {
            Box::pin(async { Ok((200, serde_json::json!({"op": "list", "roles": []}))) })
        });
        d.register("PUT", "/api/router/roles", |_body| {
            Box::pin(async { Ok((200, serde_json::json!({"op": "update", "ok": true}))) })
        });
        let list = d.dispatch(1, req("GET", "/api/router/roles")).await;
        let upd = d.dispatch(2, req("PUT", "/api/router/roles")).await;
        assert_eq!(list.result.unwrap().body["op"], "list");
        assert_eq!(upd.result.unwrap().body["op"], "update");
    }

    #[tokio::test]
    async fn method_is_case_insensitive() {
        let mut d = Dispatcher::new();
        d.register("get", "/api/ping", |_body| {
            Box::pin(async { Ok((200, serde_json::json!({"ok": true}))) })
        });
        // Lowercase 'get' is normalized to GET on register;
        // dispatch with uppercase GET should still find it.
        let resp = d.dispatch(1, req("GET", "/api/ping")).await;
        assert_eq!(resp.result.unwrap().status, 200);
    }

    /// v0.4.21 (event 000064 follow-up): dispatch should strip
    /// `?query` from the path so handlers registered on the bare
    /// path still match. Query parameters are merged into the
    /// body so handlers can read them via `body.get(...)`.
    #[tokio::test]
    async fn dispatch_strips_query_string() {
        let mut d = Dispatcher::new();
        d.register("GET", "/api/tasks", |body| {
            Box::pin(async move {
                let wf_id = body.get("wf_id").and_then(|v| v.as_str()).unwrap_or("");
                Ok((200, serde_json::json!({"wf_id": wf_id})))
            })
        });
        let resp = d.dispatch(1, req("GET", "/api/tasks?wf_id=abc123")).await;
        let r = resp.result.expect("ok");
        assert_eq!(r.status, 200);
        assert_eq!(r.body.get("wf_id").and_then(|v| v.as_str()), Some("abc123"));
    }

    /// v0.4.30 (audit 000130): the trailing-wildcard
    /// placeholder `{name+}` absorbs one or more path segments
    /// into a single `/`-joined value. Necessary because
    /// Flowntier secret names live under
    /// `flowntier/<id>` (e.g. `flowntier/minimax`) which
    /// contains a literal `/`.
    #[tokio::test]
    async fn wildcard_placeholder_absorbs_slash() {
        let mut d = Dispatcher::new();
        d.register("PUT", "/api/settings/secrets/{name+}", |body| {
            Box::pin(async move {
                let name = body.get("name").and_then(|v| v.as_str()).unwrap_or("");
                Ok((200, serde_json::json!({"name": name})))
            })
        });
        // Two-segment name arrives intact.
        let resp = d
            .dispatch(1, req("PUT", "/api/settings/secrets/flowntier/minimax"))
            .await;
        let r = resp.result.expect("ok");
        assert_eq!(
            r.body.get("name").and_then(|v| v.as_str()),
            Some("flowntier/minimax")
        );
    }

    /// v0.4.30: `{name+}` also works in the middle of a path
    /// when followed by a literal suffix (e.g. `/reveal`).
    /// The wildcard is greedy but must leave enough segments
    /// for the literal suffix.
    #[tokio::test]
    async fn wildcard_placeholder_then_literal_suffix() {
        let mut d = Dispatcher::new();
        d.register("GET", "/api/settings/secrets/{name+}/reveal", |body| {
            Box::pin(async move {
                let name = body.get("name").and_then(|v| v.as_str()).unwrap_or("");
                Ok((200, serde_json::json!({"name": name})))
            })
        });
        let resp = d
            .dispatch(
                2,
                req("GET", "/api/settings/secrets/flowntier/minimax/reveal"),
            )
            .await;
        let r = resp.result.expect("ok");
        assert_eq!(
            r.body.get("name").and_then(|v| v.as_str()),
            Some("flowntier/minimax")
        );
    }

    /// v0.4.31 (audit 000130, follow-up): the trailing-wildcard
    /// PUT route for `{name+}` against a 2-segment name like
    /// `flowntier/minimax`. Reproduces the chairman's reported
    /// "no handler registered for PUT
    /// /api/settings/secrets/flowntier/minimax" symptom.
    #[tokio::test]
    async fn wildcard_put_flowntier_namespace() {
        let mut d = Dispatcher::new();
        d.register("PUT", "/api/settings/secrets/{name+}", |body| {
            Box::pin(async move {
                let name = body.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let value = body.get("value").and_then(|v| v.as_str()).unwrap_or("");
                Ok((
                    200,
                    serde_json::json!({
                        "name": name,
                        "value_len": value.len(),
                    }),
                ))
            })
        });
        let resp = d
            .dispatch(1, req("PUT", "/api/settings/secrets/flowntier/minimax"))
            .await;
        assert!(
            resp.error.is_none(),
            "expected success, got error: {:?}",
            resp.error
        );
        let r = resp.result.expect("ok");
        assert_eq!(
            r.body.get("name").and_then(|v| v.as_str()),
            Some("flowntier/minimax"),
        );
    }
}
