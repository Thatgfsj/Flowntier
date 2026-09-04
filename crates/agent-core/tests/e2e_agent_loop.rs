//! End-to-end test of the agent loop.
//!
//! Drives [`Agent::run`] with a hand-written `ScriptedProvider`
//! that emits a pre-canned SSE stream, and asserts the resulting
//! event stream and side effects.
//!
//! Concretely we verify:
//!  - the loop streams text deltas as they arrive,
//!  - the loop executes a `bash` tool call and surfaces
//!    `ToolStarted` + `ToolFinished` events,
//!  - the loop terminates with `Done` when the model emits no
//!    further tool calls,
//!  - a second-iteration loop works (model asks, then "stops").

use std::sync::Arc;

use agent_core::provider::{ChatStream, Provider, ProviderError, StreamChunk};
use agent_core::tool::ToolRegistry;
use agent_core::workspace::Workspace;
use agent_core::{Agent, AgentConfig, AgentEvent};
use async_trait::async_trait;
use futures::stream;
use futures::Stream;
use std::pin::Pin;
use tokio::sync::mpsc;
use tokio::time::{timeout, Duration};
use tokio_util::sync::CancellationToken;

/// A scripted provider: each call yields the next script
/// from the queue. Once the queue is empty, returns a single
/// empty `Text` chunk + `Done` so the loop terminates cleanly.
struct ScriptedProvider {
    model: String,
    /// One script per *call*. The agent loop may call the
    /// provider many times in a single run; each call draws
    /// one entry.
    scripts: std::sync::Mutex<Vec<Vec<ScriptStep>>>,
}

enum ScriptStep {
    Text(String),
    ToolCall {
        id: String,
        name: &'static str,
        args: serde_json::Value,
    },
    Done,
}

impl std::fmt::Debug for ScriptedProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScriptedProvider")
            .field("model", &self.model)
            .finish()
    }
}

impl ScriptedProvider {
    /// Build with a single script that the provider replays every
    /// call (use when the test wants the same response forever).
    fn repeat(model: &str, script: Vec<ScriptStep>) -> Self {
        Self {
            model: model.into(),
            scripts: std::sync::Mutex::new(vec![script]),
        }
    }

    /// Build with a per-call script (use when the test wants the
    /// loop to terminate after N provider calls).
    fn scripted(model: &str, scripts: Vec<Vec<ScriptStep>>) -> Self {
        Self {
            model: model.into(),
            scripts: std::sync::Mutex::new(scripts),
        }
    }

    fn next_script(&self) -> Vec<ScriptStep> {
        let mut g = self.scripts.lock().unwrap();
        if g.is_empty() {
            // Default: emit empty text + done → loop sees no tool
            // calls and emits Done.
            vec![ScriptStep::Text(String::new()), ScriptStep::Done]
        } else {
            g.remove(0)
        }
    }
}

#[async_trait]
impl Provider for ScriptedProvider {
    fn id(&self) -> &'static str {
        "scripted"
    }
    fn model_id(&self) -> &str {
        &self.model
    }

    async fn stream_chat(
        &self,
        _messages: &[agent_core::message::Message],
        _tools: &[serde_json::Value],
        _cancel: CancellationToken,
    ) -> Result<ChatStream, ProviderError> {
        let script = self.next_script();
        let chunks: Vec<Result<StreamChunk, ProviderError>> = script
            .into_iter()
            .map(|step| match step {
                ScriptStep::Text(s) => Ok(StreamChunk::Text { delta: s }),
                ScriptStep::ToolCall { id, name, args } => Ok(StreamChunk::ToolUse {
                    call: agent_core::message::ToolCall {
                        id,
                        name: name.into(),
                        args,
                    },
                }),
                ScriptStep::Done => Ok(StreamChunk::Done {
                    reason: "stop".into(),
                }),
            })
            .collect();

        let s: Pin<Box<dyn Stream<Item = Result<StreamChunk, ProviderError>> + Send>> =
            Box::pin(stream::iter(chunks));
        Ok(s)
    }
}

/// Drain events from the agent until `Done` is seen, or 5 s.
async fn collect_until_done(mut rx: mpsc::UnboundedReceiver<AgentEvent>) -> Vec<AgentEvent> {
    let mut out = Vec::new();
    loop {
        match timeout(Duration::from_secs(5), rx.recv()).await {
            Ok(Some(ev)) => {
                let done = matches!(ev, AgentEvent::Done { .. });
                out.push(ev);
                if done {
                    break;
                }
            }
            Ok(None) => break,
            Err(_) => panic!("agent loop did not emit Done within 5 s; got: {out:#?}"),
        }
    }
    out
}

#[tokio::test(flavor = "current_thread")]
async fn loop_streams_text_and_terminates_without_tools() {
    let provider = Arc::new(ScriptedProvider::repeat(
        "test-1",
        vec![
            ScriptStep::Text("Hi, ".into()),
            ScriptStep::Text("world!".into()),
            ScriptStep::Done,
        ],
    ));
    let tools = Arc::new(ToolRegistry::with_builtins());
    let ws = Workspace::new(std::env::temp_dir(), "tmp");
    let agent = Agent::new(
        agent_core::prompt::Role::Worker,
        provider,
        tools,
        ws,
        AgentConfig::default(),
    );

    let rx = agent.run("say hi");
    let events = collect_until_done(rx).await;

    let text_deltas: Vec<&str> = events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::TextDelta { delta, .. } => Some(delta.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(text_deltas, vec!["Hi, ", "world!"]);

    assert!(matches!(events.last(), Some(AgentEvent::Done { status, .. }) if status == "DONE"));
}

#[tokio::test(flavor = "current_thread")]
async fn loop_executes_bash_tool_and_finishes() {
    let tmp = tempfile::tempdir().unwrap();

    let provider = Arc::new(ScriptedProvider::scripted(
        "test-2",
        // call 1: emit bash tool call
        vec![vec![
            ScriptStep::Text("Let me run that.".into()),
            ScriptStep::ToolCall {
                id: "call_1".into(),
                name: "bash",
                args: serde_json::json!({"command": "echo from-test"}),
            },
            ScriptStep::Done,
        ]],
        // call 2 onwards: empty script → loop sees no tool_calls → Done
    ));
    let tools = Arc::new(ToolRegistry::with_builtins());
    let ws = Workspace::new(tmp.path(), "tmp");
    let agent = Agent::new(
        agent_core::prompt::Role::Worker,
        provider,
        tools,
        ws,
        AgentConfig::default(),
    );

    let rx = agent.run("echo something");
    let events = collect_until_done(rx).await;

    let saw_started = events
        .iter()
        .any(|e| matches!(e, AgentEvent::ToolStarted { call, .. } if call.name == "bash"));
    let saw_finished = events.iter().any(
        |e| matches!(e, AgentEvent::ToolFinished { tool_call_id, is_error, .. } if tool_call_id == "call_1" && !*is_error),
    );
    assert!(
        saw_started,
        "expected ToolStarted for bash; events: {events:#?}"
    );
    assert!(
        saw_finished,
        "expected ToolFinished for call_1; events: {events:#?}"
    );

    eprintln!("DEBUG events for bash test: {events:#?}");

    let (preview, is_err) = events
        .iter()
        .find_map(|e| match e {
            AgentEvent::ToolFinished {
                preview, is_error, ..
            } => Some((preview.clone(), *is_error)),
            _ => None,
        })
        .unwrap();
    assert!(!is_err, "bash should succeed; preview: {preview}");
    assert!(preview.contains("from-test"), "preview: {preview}");

    assert!(matches!(events.last(), Some(AgentEvent::Done { status, .. }) if status == "DONE"));
}

#[tokio::test(flavor = "current_thread")]
async fn tool_error_is_surfaced_and_does_not_crash_loop() {
    let provider = Arc::new(ScriptedProvider::scripted(
        "test-3",
        vec![vec![
            ScriptStep::ToolCall {
                id: "c".into(),
                name: "bash",
                args: serde_json::json!({}), // no 'command'
            },
            ScriptStep::Done,
        ]],
    ));
    let tools = Arc::new(ToolRegistry::with_builtins());
    let ws = Workspace::new(std::env::temp_dir(), "tmp");
    let agent = Agent::new(
        agent_core::prompt::Role::Worker,
        provider,
        tools,
        ws,
        AgentConfig::default(),
    );

    let rx = agent.run("x");
    let events = collect_until_done(rx).await;

    let (is_err, id) = events
        .iter()
        .find_map(|e| match e {
            AgentEvent::ToolFinished {
                is_error,
                tool_call_id,
                ..
            } => Some((*is_error, tool_call_id.clone())),
            _ => None,
        })
        .expect("expected a ToolFinished");
    assert!(is_err, "expected tool to error");
    assert_eq!(id, "c");
    assert!(matches!(events.last(), Some(AgentEvent::Done { .. })));
}

#[tokio::test(flavor = "current_thread")]
async fn read_and_write_tools_round_trip() {
    let tmp = tempfile::tempdir().unwrap();

    let provider = Arc::new(ScriptedProvider::scripted(
        "test-4",
        vec![vec![
            ScriptStep::ToolCall {
                id: "w".into(),
                name: "write",
                args: serde_json::json!({"path": "hello.txt", "content": "hi rust"}),
            },
            ScriptStep::ToolCall {
                id: "r".into(),
                name: "read",
                args: serde_json::json!({"path": "hello.txt"}),
            },
            ScriptStep::Done,
        ]],
    ));
    let tools = Arc::new(ToolRegistry::with_builtins());
    let ws = Workspace::new(tmp.path(), "tmp");
    let agent = Agent::new(
        agent_core::prompt::Role::Worker,
        provider,
        tools,
        ws,
        AgentConfig::default(),
    );

    let rx = agent.run("files");
    let events = collect_until_done(rx).await;

    let read_preview = events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::ToolFinished {
                tool_call_id,
                preview,
                ..
            } if tool_call_id == "r" => Some(preview.clone()),
            _ => None,
        })
        .next()
        .expect("read tool should have finished");
    assert!(
        read_preview.contains("hi rust"),
        "read preview was: {read_preview}"
    );

    let on_disk = std::fs::read_to_string(tmp.path().join("hello.txt")).unwrap();
    assert_eq!(on_disk, "hi rust");
}
/// Same tool call with the same args failing 3 times in a row
/// should ABORT the loop with `ABORTED_REPEAT` instead of letting
/// it eat the full max_iterations budget.
///
/// To force failures we script a tool call with arguments that
/// the bash tool will refuse on this platform: `bash` with
/// dangerous pattern.
#[tokio::test(flavor = "current_thread")]
async fn repeat_failure_aborts_before_max_iterations() {
    // The bash tool refuses `rm -rf /` when `approved` is false.
    // Repeat the SAME call 3 times and assert ABORTED_REPEAT.
    let provider = Arc::new(ScriptedProvider::scripted(
        "test-repeat",
        vec![
            // Provider call 1: bash rm -rf / (refused)
            vec![ScriptStep::ToolCall {
                id: "r1".into(),
                name: "bash",
                args: serde_json::json!({"command": "rm -rf /"}),
            }],
            // Provider call 2: same
            vec![ScriptStep::ToolCall {
                id: "r2".into(),
                name: "bash",
                args: serde_json::json!({"command": "rm -rf /"}),
            }],
            // Provider call 3: same — this should NOT be reached,
            // because ABORTED_REPEAT fires after the 2nd failure.
            vec![ScriptStep::ToolCall {
                id: "r3".into(),
                name: "bash",
                args: serde_json::json!({"command": "rm -rf /"}),
            }],
            // Provider call 4 (safety): empty script
            vec![ScriptStep::Done],
        ],
    ));
    let tools = Arc::new(ToolRegistry::with_builtins());
    let ws = Workspace::new(std::env::temp_dir(), "repeat-test");
    let agent = Agent::new(
        agent_core::prompt::Role::Worker,
        provider,
        tools,
        ws,
        AgentConfig {
            // Set high so we know it's the repeat detector, not the cap,
            // that bails us out.
            max_iterations: 20,
            repeat_abort_after: 3,
            ..Default::default()
        },
    );

    let rx = agent.run("force repeat failures");
    let events = collect_until_done(rx).await;

    // First we must see Done with status ABORTED_REPEAT.
    let last = events.last().expect("expected at least one event");
    let status = match last {
        AgentEvent::Done { status, .. } => status.clone(),
        other => panic!("expected Done as last event, got {other:?}"),
    };
    assert_eq!(status, "ABORTED_REPEAT", "events: {events:#?}");

    // We should have seen exactly 3 tool failures (and 0 successes).
    let failures: usize = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::ToolFinished { is_error: true, .. }))
        .count();
    assert_eq!(
        failures, 3,
        "expected exactly 3 failures, events: {events:#?}"
    );
}

/// Capability `read_only` makes the `write` tool refuse at the
/// gate. The agent loop should see is_error=true and surface it.
#[tokio::test(flavor = "current_thread")]
async fn read_only_capability_blocks_write_tool() {
    use agent_core::tool::{Capabilities, ToolContext};

    // We can't plumb a per-run ToolContext through the public API
    // yet (loop_.rs hard-codes a default ToolContext). So this
    // test exercises the gate at the ToolRegistry level directly,
    // which is what the loop also calls.
    let reg = ToolRegistry::with_builtins();
    let ws = Workspace::new(std::env::temp_dir(), "ro-test");
    let read_only_ctx = ToolContext {
        workspace: ws,
        approved: true,
        capabilities: Capabilities::read_only(),
        cancel: None,
    };

    let out = reg
        .execute(
            "write",
            serde_json::json!({"path": "should_not_exist.txt", "content": "no"}),
            &read_only_ctx,
        )
        .await
        .expect("execute should return");
    assert!(out.is_error, "write should be refused");
    assert!(out.content.contains("write capability"));
}
