//! Standalone E2E tests — zero deps on the lib crate to avoid
//! the Tauri DLL load issue (STATUS_ENTRYPOINT_NOT_FOUND).
//!
//! Chairman's directive ("端对端测试, 简单中等困难, 并且摸清边界情况").
//!
//! These tests mirror the FS-layer code paths in lib.rs's
//! nwt_init_workspace and search_log. They re-implement the
//! same algorithms inline (the lib functions are pub but
//! linking them drags in the full Tauri DLL chain on Windows).
//!
//! If the algorithms change in lib.rs, these tests must be
//! updated to match.

mod e2e_nwt;
