//! Acceptance test: drive MiniMax through the full agent loop
//! with a multi-step task that exercises every tool (bash, read,
//! write, patch, grep). Every event is printed in detail so we
//! can spot hallucinated tool calls vs real ones.
//!
//! The test:
//! 1. Sends a task to MiniMax-Text-01 via OpenAiProvider::compat
//! 2. Lets the agent loop call write/read/bash/patch/grep freely
//! 3. Prints every event as it happens
//! 4. After Done, lists every file actually created on disk

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
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,agent_core=debug")),
        )
        .init();

    let api_key = std::env::var("MINIMAX_API_KEY")
        .map_err(|_| anyhow::anyhow!("MINIMAX_API_KEY env var required"))?;
    let base_url =
        std::env::var("MINIMAX_BASE_URL").unwrap_or_else(|_| "https://api.minimaxi.com/v1".into());
    let model = std::env::var("MINIMAX_MODEL").unwrap_or_else(|_| "MiniMax-Text-01".into());

    let workspace = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("aco-acceptance"));
    std::fs::create_dir_all(&workspace)?;

    eprintln!("\n╔══════════════════════════════════════════════════════════");
    eprintln!("║ ACO v0.3 acceptance — MiniMax + agent-core");
    eprintln!("║ workspace: {}", workspace.display());
    eprintln!("║ provider: openai_compat / {} / {}", base_url, model);
    eprintln!("╚══════════════════════════════════════════════════════════\n");

    let provider: Arc<dyn agent_core::Provider> = Arc::new(OpenAiProvider::compat(
        base_url.clone(),
        model.clone(),
        api_key,
    ));
    let tools = Arc::new(ToolRegistry::with_builtins());
    let ws = Workspace::new(workspace.clone(), "acceptance");
    let agent = Agent::new(
        agent_core::prompt::Role::Worker,
        provider,
        tools,
        ws,
        AgentConfig::default(),
    );

    let task = match std::env::args().nth(2) {
        Some(p) => {
            std::fs::read_to_string(&p).map_err(|e| anyhow::anyhow!("read task file {p}: {e}"))?
        }
        None => default_task(&workspace),
    };

    eprintln!("=== TASK ===\n{task}\n");

    let mut rx = agent.run(&task);
    let mut transcript = String::new();
    let mut tool_call_count = 0usize;
    let mut tool_results: Vec<(String, bool, String, u64)> = Vec::new();
    let mut started: std::collections::HashMap<String, serde_json::Value> = Default::default();

    while let Some(ev) = timeout(Duration::from_secs(180), rx.recv())
        .await
        .map_err(|_| anyhow::anyhow!("agent timed out after 180s"))?
    {
        match ev {
            AgentEvent::TextDelta {
                delta,
                agent_display,
                ..
            } => {
                transcript.push_str(&delta);
                eprint!("[{agent_display}] {delta}");
            }
            AgentEvent::ToolStarted {
                call,
                agent_display,
                ..
            } => {
                tool_call_count += 1;
                eprintln!(
                    "\n  ⚙ [{tool_call_count:02}] [{agent_display}] START {} (id={})\n       args = {}",
                    call.name,
                    call.id,
                    serde_json::to_string(&call.args).unwrap_or_default()
                );
                started.insert(call.id.clone(), call.args.clone());
            }
            AgentEvent::ToolFinished {
                tool_call_id,
                preview,
                is_error,
                elapsed_ms,
                ..
            } => {
                let mark = if is_error { "✗" } else { "✓" };
                eprintln!(
                    "  {mark} END  id={tool_call_id} in {elapsed_ms}ms\n       preview = {preview}"
                );
                tool_results.push((tool_call_id, is_error, preview, elapsed_ms));
            }
            AgentEvent::Done {
                status, summary, ..
            } => {
                eprintln!("\n=== DONE: {status}");
                if let Some(s) = summary {
                    eprintln!("    summary: {s}");
                }
                eprintln!();
                break;
            }
            _ => {}
        }
    }

    eprintln!("\n=== SUMMARY ===");
    eprintln!("  text delta: {} chars", transcript.len());
    eprintln!("  tool calls: {}", tool_call_count);
    eprintln!(
        "  tool errors: {}",
        tool_results.iter().filter(|(_, e, _, _)| *e).count()
    );
    eprintln!();

    eprintln!("=== FILES ON DISK ===");
    list_files(&workspace, 0)?;

    Ok(())
}

fn list_files(dir: &std::path::Path, depth: usize) -> std::io::Result<()> {
    if depth > 4 {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let e = entry?;
        let path = e.path();
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let indent = "  ".repeat(depth + 1);
        if path.is_dir() {
            if name.starts_with("node_modules")
                || name.starts_with(".git")
                || name.starts_with("target")
            {
                eprintln!("{indent}{name}/ (skipped)");
                continue;
            }
            eprintln!("{indent}{name}/");
            list_files(&path, depth + 1)?;
        } else {
            let meta = e.metadata()?;
            eprintln!("{indent}{name} ({} bytes)", meta.len());
        }
    }
    Ok(())
}

fn default_task(workspace: &std::path::Path) -> String {
    format!(
        "在当前工作目录（{path}）下创建一个极简用户管理后端，要求：

1. 用 bash 工具创建子目录 backend/
2. 用 write 工具创建 backend/package.json，dependencies 包含 express@4.18.2 和 better-sqlite3@11.3.0
3. 用 write 工具创建 backend/db.js：
   - 连接文件 backend/users.db
   - 如果表不存在就建表 users (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL, email TEXT NOT NULL UNIQUE, role TEXT DEFAULT 'user', created_at TEXT DEFAULT CURRENT_TIMESTAMP)
   - 导出 db 实例
4. 用 write 工具创建 backend/server.js，监听 127.0.0.1:4400，实现：
   - GET /api/users        列表，返回 {{users:[...]}}
   - GET /api/users/:id    单个用户，找不到返回 404
   - POST /api/users       body {{name,email,role?}}，返回新用户
   - PUT /api/users/:id    body {{name?,email?,role?}}，返回更新后的用户
   - DELETE /api/users/:id 删除，返回 {{deleted: id}}
   - 全部用 better-sqlite3 prepared statements
   - 用 cors() 中间件允许 http://localhost:5500
5. 用 bash 工具跑：cd backend && npm install --no-audit --no-fund
6. 用 bash 工具后台启动：cd backend && node server.js > server.log 2>&1 &
7. 睡 3 秒后用 bash 跑：curl http://127.0.0.1:4400/api/users
8. 再用 bash 跑：curl -X POST http://127.0.0.1:4400/api/users -H 'Content-Type: application/json' -d '{{\"name\":\"Alice\",\"email\":\"alice@test.com\"}}'
9. 再 curl 一次 list，确认新用户出现

完成后回复一句话总结（不超过 100 字）：几个文件、npm install 是否成功、curl 看到什么。",
        path = workspace.display()
    )
}
