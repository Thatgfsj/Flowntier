//! Orchestrated planner — demonstrate v0.3 multi-role agent flow.
//!
//! Pattern: one Chief (orchestrator) routes to one Planner, who
//! produces a plan, then one Worker executes it. All sharing the
//! same event stream so a UI can show the whole conversation.
//!
//! Run with:
//!   MINIMAX_API_KEY=... cargo run -p agent-core --example orchestrated_planner --release
//!
//! This example is useful as a smoke test of the agent loop when
//! you do not have a real ChatZone UI handy: it constructs three
//! `Agent`s from the same provider, runs each in turn, and prints
//! the events as they flow.
//!
//! Note: this example wires the three agents sequentially. In a
//! production flow the Chief would call the Planner and Worker
//! over IPC or through the pipe-server; for a single-process
//! smoke we just call them in sequence in the same runtime.

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
    let base_url =
        std::env::var("MINIMAX_BASE_URL").unwrap_or_else(|_| "https://api.minimaxi.com/v1".into());
    let model = std::env::var("MINIMAX_MODEL").unwrap_or_else(|_| "MiniMax-M3".into());

    let workspace_root = std::env::args()
        .nth(1)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("aco-orchestrated"));
    std::fs::create_dir_all(&workspace_root)?;

    let provider: Arc<dyn agent_core::Provider> = Arc::new(OpenAiProvider::compat(
        base_url.clone(),
        model.clone(),
        api_key,
    ));
    let tools = Arc::new(ToolRegistry::with_builtins());

    // Phase 1: Chief receives the task, routes it.
    eprintln!("\n=== Phase 1: Chief (orchestrator) ===");
    let chief = Agent::new(
        agent_core::prompt::Role::Chief,
        provider.clone(),
        tools.clone(),
        Workspace::new(workspace_root.clone(), "orchestrated"),
        AgentConfig::default(),
    );
    let task = "Plan a 1-page summary of /api/users endpoint (read-only). \
                Then ask a Worker to write it as backend/SUMMARY.md. \
                Then summarize in 2 sentences.";
    drain_events(chief.run(task)).await?;

    // Phase 2: Planner produces the actual plan (just to show the
    // event flow; the Chief would normally do this via a tool call).
    eprintln!("\n=== Phase 2: Planner ===");
    let planner = Agent::new(
        agent_core::prompt::Role::Planner,
        provider.clone(),
        tools.clone(),
        Workspace::new(workspace_root.clone(), "orchestrated"),
        AgentConfig::default(),
    );
    drain_events(planner.run("List the files in the current directory using bash `ls`.")).await?;

    // Phase 3: Worker executes.
    eprintln!("\n=== Phase 3: Worker ===");
    let worker = Agent::new(
        agent_core::prompt::Role::Worker,
        provider.clone(),
        tools.clone(),
        Workspace::new(workspace_root.clone(), "orchestrated"),
        AgentConfig::default(),
    );
    drain_events(
        worker.run("Use bash to confirm `node --version` is installed; print the output."),
    )
    .await?;

    Ok(())
}

async fn drain_events(
    mut rx: tokio::sync::mpsc::UnboundedReceiver<AgentEvent>,
) -> anyhow::Result<()> {
    while let Some(ev) = timeout(Duration::from_secs(120), rx.recv())
        .await
        .map_err(|_| anyhow::anyhow!("agent timed out"))?
    {
        match &ev {
            AgentEvent::TextDelta {
                delta,
                agent_display,
                ..
            } => {
                eprint!("[{agent_display}] {delta}");
            }
            AgentEvent::ToolStarted {
                call,
                agent_display,
                ..
            } => {
                eprintln!(
                    "\n  ⚙ [{agent_display}] {} id={} args={}",
                    call.name,
                    call.id,
                    serde_json::to_string(&call.args).unwrap_or_default()
                );
            }
            AgentEvent::ToolFinished {
                tool_call_id,
                is_error,
                elapsed_ms,
                ..
            } => {
                let mark = if *is_error { "✗" } else { "✓" };
                eprintln!("  {mark} END id={tool_call_id} in {elapsed_ms}ms");
            }
            AgentEvent::Done {
                status, summary, ..
            } => {
                eprintln!("\n→ DONE: {status}");
                if let Some(s) = summary {
                    eprintln!("  summary: {s}");
                }
                return Ok(());
            }
            _ => {}
        }
    }
    anyhow::bail!("event stream ended without Done")
}
