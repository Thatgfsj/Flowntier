//! End-to-end tests: bring up the pipe server in-process, open a
//! client connection, send a JSON-RPC request, and verify the
//! response.
//!
//! The suite is split into focused submodules under tests/e2e_pipe/
//! so no file exceeds the 1000-line ceiling. Shared helpers live in
//! common; the #[test] / #[tokio::test] inventory is unchanged by this
//! split (pure code movement).
//!
//! The modules use explicit `#[path]` attributes because this crate root
//! sits directly under `tests/`: a plain `mod x;` would resolve to
//! `tests/x.rs` instead of `tests/e2e_pipe/x.rs`.

#[path = "e2e_pipe/basic.rs"]
mod basic;
#[path = "e2e_pipe/common.rs"]
mod common;
#[path = "e2e_pipe/disabled_models.rs"]
mod disabled_models;
#[path = "e2e_pipe/http_bridge.rs"]
mod http_bridge;
#[cfg(windows)]
#[path = "e2e_pipe/production_pipe.rs"]
mod production_pipe;
#[path = "e2e_pipe/providers.rs"]
mod providers;
#[path = "e2e_pipe/quota.rs"]
mod quota;
#[path = "e2e_pipe/router_config.rs"]
mod router_config;
#[path = "e2e_pipe/workflow_verdict.rs"]
mod workflow_verdict;
