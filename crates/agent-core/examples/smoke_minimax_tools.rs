//! End-to-end smoke: drive MiniMax with a task that REQUIRES a
//! tool call (write a file). Verifies the whole agent loop:
//! stream → tool_use → bash/write → ToolFinished → Done.
//!
//! Run with:
//!   MINIMAX_BASE_URL=https://api.minimaxi.com/v1 cargo run -p agent-core --example smoke_minimax_tools --release

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use agent_core::provider::openai::OpenAiProvider;
use agent_core::tool::ToolRegistry;
use agent_core::workspace::Workspace;
use agent_core::{Agent, AgentConfig, AgentEvent};
use tokio::time::timeout;

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let api_key = std::env::var("MINIMAX_API_KEY")
        .map_err(|_| anyhow::anyhow!("MINIMAX_API_KEY env var required"))?;
    let base_url = std::env::var("MINIMAX_BASE_URL")
        .unwrap_or_else(|_| "https://api.minimaxi.com/v1".into());
    let model = std::env::var("MINIMAX_MODEL").unwrap_or_else(|_| "MiniMax-Text-01".into());

    let workspace_dir = std::env::temp_dir().join("aco-minimax-smoke");
    std::fs::create_dir_all(&workspace_dir)?;
    let marker = workspace_dir.join("created-by-minimax.txt");

    // Clean state.
    let _ = std::fs::remove_file(&marker);

    let provider: Arc<dyn agent_core::Provider> = Arc::new(OpenAiProvider::compat(
        base_url.clone(),
        model.clone(),
        api_key,
    ));

    let tools = Arc::new(ToolRegistry::with_builtins());
    let ws = Workspace::new(workspace_dir.clone(), "minimax-smoke");
    let agent = Agent::new(
        agent_core::prompt::Role::Worker,
        provider,
        tools,
        ws,
        AgentConfig::default(),
    );

    let task = format!(
        "请用 write 工具在当前工作目录下创建一个文件 '{}'，内容是 'created by minimax agent-core (v0.3)'。不要用 bash，直接调 write。完成后回 'done'。",
        marker.file_name().unwrap().to_string_lossy()
    );

    eprintln!("→ task: {task}");
    eprintln!("→ provider: openai_compat / {base_url} / {model}");
    eprintln!("→ workspace: {}", workspace_dir.display());

    let mut rx = agent.run(&task);
    let mut text_buf = String::new();
    let mut tools_used: Vec<(String, bool, u64)> = Vec::new();

    while let Some(ev) = timeout(Duration::from_secs(120), rx.recv())
        .await
        .map_err(|_| anyhow::anyhow!("agent timed out after 120s"))?
    {
        match &ev {
            AgentEvent::TextDelta { delta, .. } => {
                text_buf.push_str(delta);
                eprint!("{delta}");
            }
            AgentEvent::ToolStarted { call, .. } => {
                eprintln!(
                    "\n  ⚙ tool START: {} id={} args={}",
                    call.name,
                    call.id,
                    serde_json::to_string(&call.args).unwrap_or_default()
                );
            }
            AgentEvent::ToolFinished { tool_call_id, preview, is_error, elapsed_ms, .. } => {
                let mark = if *is_error { "✗" } else { "✓" };
                eprintln!(
                    "  {mark} tool END  id={tool_call_id} in {elapsed_ms}ms: {preview}"
                );
                // Best-effort pair with the started event name.
                tools_used.push((tool_call_id.clone(), *is_error, *elapsed_ms));
            }
            AgentEvent::Done { status, summary, .. } => {
                eprintln!("\n→ status: {status}");
                if let Some(s) = summary {
                    eprintln!("→ summary: {s}");
                }
                anyhow::ensure!(status == "DONE", "expected DONE, got {status}");
                eprintln!(
                    "\n=== transcript ({} chars, {} tool calls) ===",
                    text_buf.len(),
                    tools_used.len()
                );
                eprintln!("{text_buf}");
                eprintln!("=== /transcript ===");

                // Verify the file was actually created.
                if marker.exists() {
                    let contents = std::fs::read_to_string(&marker).unwrap_or_default();
                    eprintln!("✓ file exists at {}", marker.display());
                    eprintln!("  content: {contents:?}");
                } else {
                    anyhow::bail!("✗ file NOT created at {}", marker.display());
                }
                return Ok(());
            }
            _ => {}
        }
    }
    anyhow::bail!("event stream ended without a Done event")
}

#[allow(dead_code)]
fn _suppress_unused_warning_for_path(p: PathBuf) -> PathBuf { p }