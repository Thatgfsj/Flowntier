//! Local IPC server for Flowntier (v0.3+).
//!
//! Replaces the v0.2 Python pipe server with a pure-Rust
//! implementation that runs in the same process as the Tauri shell
//! (or as a standalone binary). See `history/docs/V03_DELETIONS.md`.
//!
//! Two channels:
//! - **RPC**: JSON-RPC 2.0 over newline-delimited JSON,
//!   request/response, one connection = one round-trip.
//! - **Events**: server-push; one JSON object per event.
//!
//! Wire format is unchanged from the Python implementation
//! so the Tauri Rust client (`apps/desktop/src-tauri`) does
//! not need any modifications.

pub mod dispatcher;
pub mod handlers;
pub mod i_ching;
pub mod protocol;
pub mod server;

pub use dispatcher::Dispatcher;
pub use handlers::{register_all, ServerState};
pub use protocol::{codes, RpcError, RpcParams, RpcRequest, RpcResponse, RpcResult, MAX_LINE};
pub use server::{Server, ServerConfig};