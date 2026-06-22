//! Smoke test: drive a real MiniMax chat completion via the
//! embedded agent. Runs `bash` end-to-end through the agent
//! loop, prints every event, asserts `Done` arrives.
//!
//! Run with:
//!   MINIMAX_API_KEY=...  MINIMAX_BASE_URL=https://api.minimaxi.com/anthropic \
//!     cargo run -p agent-core --example smoke_minimax --release

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
        .unwrap_or_else(|_| "https://api.minimaxi.com/anthropic".into());
    let model = std::env::var("MINIMAX_MODEL").unwrap_or_else(|_| "MiniMax-Text-01".into());

    let provider: Arc<dyn agent_core::Provider> = Arc::new(OpenAiProvider::compat(
        base_url.clone(),
        model.clone(),
        api_key,
    ));

    // First: do a raw non-streaming call so we can see exactly what
    // shape MiniMax returns. This isolates "agent loop" bugs from
    // "provider parsing" bugs.
    eprintln!("\n=== raw non-streaming probe ===");
    let probe_body = serde_json::json!({
        "model": model,
        "max_tokens": 64,
        "stream": false,
        "messages": [{"role":"user","content":"用中文一句话回答：1+1=?"}],
    });
    let probe_url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let resp = reqwest::Client::new()
        .post(&probe_url)
        .bearer_auth(std::env::var("MINIMAX_API_KEY").unwrap())
        .header("content-type", "application/json")
        .json(&probe_body)
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await?;
    eprintln!("← HTTP {}", resp.status());
    let txt = resp.text().await.unwrap_or_default();
    let snippet: String = txt.chars().take(800).collect();
    eprintln!("{snippet}{}", if txt.chars().count() > 800 { "…" } else { "" });
    eprintln!("=== /probe ===\n");

    let tools = Arc::new(ToolRegistry::with_builtins());
    let ws = Workspace::new(std::env::temp_dir(), "minimax-smoke");
    let agent = Agent::new(
        agent_core::prompt::Role::Worker,
        provider,
        tools,
        ws,
        AgentConfig::default(),
    );

    let task = "用中文一句话告诉我今天应该做什么（不要调用任何工具）。";
    eprintln!("→ task: {task}");
    eprintln!("→ provider: openai_compat / {base_url} / {model}");

    let mut rx = agent.run(task);
    let mut text_buf = String::new();
    let mut tool_count = 0usize;

    while let Some(ev) = timeout(Duration::from_secs(60), rx.recv())
        .await
        .map_err(|_| anyhow::anyhow!("agent timed out after 60s"))?
    {
        match &ev {
            AgentEvent::TextDelta { delta, .. } => {
                text_buf.push_str(delta);
                eprint!("{delta}");
            }
            AgentEvent::ToolStarted { call, .. } => {
                tool_count += 1;
                eprintln!("\n  ⚙ tool {}: {} {:?}", tool_count, call.name, call.args);
            }
            AgentEvent::ToolFinished { preview, is_error, elapsed_ms, .. } => {
                let mark = if *is_error { "✗" } else { "✓" };
                eprintln!("  {mark} done in {elapsed_ms}ms: {preview}");
            }
            AgentEvent::Done { status, summary, .. } => {
                eprintln!("\n→ status: {status}");
                if let Some(s) = summary {
                    eprintln!("→ summary: {s}");
                }
                anyhow::ensure!(status == "DONE", "expected DONE, got {status}");
                eprintln!("\n=== full transcript ({text_len} chars, {tool_count} tool calls) ===",
                    text_len = text_buf.len());
                eprintln!("{text_buf}");
                return Ok(());
            }
            _ => {}
        }
    }
    anyhow::bail!("event stream ended without a Done event")
}