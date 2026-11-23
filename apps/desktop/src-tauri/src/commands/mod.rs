//! Tauri command implementations, grouped by domain.
//!
//! lib.rs only wires the app builder and the generate_handler list;
//! each submodule owns one command domain.

pub(crate) mod checkpoints;
pub(crate) mod health;
pub(crate) mod logs;
pub(crate) mod plugins;
pub(crate) mod prefs;
pub(crate) mod providers;
pub(crate) mod router;
pub(crate) mod secrets;
pub(crate) mod system;
pub(crate) mod workflow;
pub(crate) mod workspace;
