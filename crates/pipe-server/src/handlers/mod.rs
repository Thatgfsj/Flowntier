//! Built-in RPC handlers.
//!
//! This module wires a handful of common endpoints so the Tauri
//! client has something to talk to. New handlers are added as
//! the Python routes get ported.

use agent_core::mcp::McpServer;
use agent_core::provider::openai::OpenAiProvider;
use agent_core::tool::ToolRegistry;
use agent_core::workspace::Workspace;
use agent_core::{message::parse_chat_history, Agent, AgentConfig, AgentEvent, Message};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::warn;
use zeroize::Zeroizing;

use crate::dispatcher::Dispatcher;
use crate::secrets::SecretStore;
use crate::tools_registry;
use crate::workspace_toml::PermissionMode;

mod providers_read;
mod providers_write;
mod routes_core;
mod routes_logs;
mod routes_onboarding;
mod routes_plugins;
mod routes_providers;
mod routes_router;
mod routes_secrets;
mod routes_system;
mod routes_workflow;
mod routes_workspace_files;
mod run_task;
mod state;

pub use routes_router::resolve_role_for_orchestrator;
pub use routes_router::ResolvedRole;
pub use state::*;

use providers_read::{list_models, list_providers, patch_provider};
use providers_write::{
    add_custom_provider, delete_custom_provider, delete_model_override, disable_model,
    enable_model, put_model_override, refresh_stale_caches,
};
use routes_router::resolve_role;
use run_task::run_task;
/// Register every built-in handler on d.
pub fn register_all(d: &mut Dispatcher, state: ServerState) {
    let state = Arc::new(state);
    routes_core::register(d, state.clone());
    routes_onboarding::register(d, state.clone());
    routes_workspace_files::register(d, state.clone());
    routes_system::register(d, state.clone());
    routes_router::register_resolve(d, state.clone());
    routes_secrets::register(d, state.clone());
    routes_router::register_roles_models(d, state.clone());
    routes_router::register_resolve_placeholder(d, state.clone());
    routes_providers::register(d, state.clone());
    routes_workflow::register_run(d, state.clone());
    routes_workflow::register_status(d, state.clone());
    routes_workflow::register_cancel_v1(d, state.clone());
    routes_router::register_put_roles(d, state.clone());
    routes_plugins::register_plugins(d, state.clone());
    routes_plugins::register_i_ching(d, state.clone());
    routes_workflow::register_cancel_v2(d, state.clone());
    routes_logs::register(d, state.clone());

    // v0.4.20: stash dispatcher handle so the quota scheduler
    // (Phase-2) can dispatch internal retry requests.
    state.set_dispatcher(Arc::new(d.clone()));
}
