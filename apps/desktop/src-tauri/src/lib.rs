//! Tauri app glue for the desktop shell.
//!
//! Architecture: React → invoke() → Rust → Windows named pipe → pipe-server
//! Frontend NEVER touches HTTP. The webview loads the dev server URL only.
//! There is no `127.0.0.1:7317` HTTP server anymore — all RPC and event
//! streaming travel over `\\.\pipe\flowntier_runtime` (RPC) and
//! `\\.\pipe\flowntier_runtime_events` (server-push events).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::Manager;
use tauri_core::logging;
use tauri_core::AppState;

mod commands;
mod sidecar;
mod util;

pub use sidecar::{graceful_kill_sidecar, SidecarHandle};
use sidecar::{spawn_runtime_sidecar, LOGGING_GUARD};
pub use util::{civil_from_days, unix_secs_to_iso8601};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let handle = app.handle().clone();

            // Bring up logging FIRST so every subsequent log line
            // (including the panic hook) lands in the file. We need
            // a data_dir to know where to write — derive it the
            // same way AppState::build does.
            if let Some(data_dir) = storage::Repository::default_data_dir() {
                let _ = std::fs::create_dir_all(&data_dir);
                let _ = LOGGING_GUARD.set(logging::init_logging(&data_dir));
            } else {
                eprintln!(
                    "[flowntier] cannot determine data_dir; logs will only go to stderr"
                );
            }

            tauri::async_runtime::block_on(async move {
                spawn_runtime_sidecar(&handle);
                match AppState::build().await {
                    Ok(state) => {
                        handle.manage(state);
                    }
                    Err(e) => {
                        // Show a native error dialog so the user
                        // gets a clear message instead of a silent
                        // process exit. The dialog plugin handles
                        // Windows MessageBox / macOS NSAlert /
                        // Linux GTK dialog automatically.
                        tracing::error!("failed to build AppState: {e}");
                        eprintln!("[flowntier] failed to build AppState: {}", e);

                        let log_path = storage::Repository::default_data_dir()
                            .map(|d| d.join("logs").display().to_string())
                            .unwrap_or_else(|| "(unavailable)".into());
                        let body = format!(
                            "Flowntier failed to start:\n\n{e}\n\nDiagnostic logs:\n{log_path}\n\n\
                             Please open a bug report at:\n  https://github.com/Thatgfsj/Flowntier/issues\n\n\
                             (Click OK to close.)"
                        );

                        // tauri-plugin-dialog's message() is async;
                        // we block on it inside the async setup so
                        // the user sees the dialog before exit.
                        use tauri_plugin_dialog::DialogExt;
                        let _ = handle
                            .dialog()
                            .message(body)
                            .title("Flowntier — startup failed")
                            .kind(tauri_plugin_dialog::MessageDialogKind::Error)
                            .blocking_show();

                        std::process::exit(1);
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::health::health_check,
            commands::secrets::list_secrets,
            commands::secrets::save_secret,
            commands::secrets::delete_secret,
            commands::secrets::reveal_secret,
            commands::secrets::seed_secrets,
            commands::providers::list_providers,
            commands::providers::toggle_provider,
            commands::router::list_router_roles,
            commands::router::list_router_models,
            commands::router::update_router_roles,
            commands::router::get_role_resolve_status,
            commands::router::get_quota_status,
            commands::router::reset_quota,
            commands::router::get_role_quota_status,
            commands::plugins::list_plugins,
            commands::plugins::invoke_plugin,
            commands::providers::fetch_provider_models,
            commands::providers::add_custom_provider,
            commands::providers::remove_custom_provider,
            commands::providers::disable_provider_model,
            commands::providers::enable_provider_model,
            commands::providers::list_disabled_models,
            commands::router::set_model_override,
            commands::router::clear_model_override,
            commands::workflow::start_workflow_cmd,
            commands::workflow::get_workflow,
            commands::workflow::cancel_workflow,
            commands::workflow::run_agent_task,
            commands::workflow::run_workflow,
            commands::system::draw_i_ching,
            commands::logs::log_frontend_error,
            commands::logs::log_webview_console,
            commands::prefs::kv_get,
            commands::prefs::kv_set,
            commands::prefs::load_sample_workflow,
            commands::prefs::first_run_complete,
            commands::health::rpc_version,
            commands::system::wipe_all_data,
            commands::logs::search_log,
            commands::workspace::get_workdir,
            commands::workspace::set_workdir,
            commands::workspace::set_workdir_with_nwt,
            commands::workspace::clear_workdir,
            commands::system::get_diagnostics,
            commands::workspace::get_runtime_workspace,
            commands::workspace::get_workspace_tree,
            commands::logs::get_recent_errors,
            commands::workflow::get_workflow_status,
            commands::workspace::get_workspace_file,
            commands::workspace::write_workspace_file,
            commands::checkpoints::create_checkpoint,
            commands::checkpoints::list_checkpoints,
            commands::checkpoints::restore_checkpoint,
            commands::checkpoints::get_mcp_status,
            commands::checkpoints::rescan_mcp,
            commands::logs::get_log_tail,
            commands::logs::clear_log_file,
            commands::logs::get_log_file_path,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        // event 000109: RunEvent::ExitRequested handler — the
        // root-cause fix for the stale-sidecar-blocks-next-launch
        // bug that the event 000083→000105 chain was patching
        // around with belt-and-braces taskkill.
        //
        // Before this fix, the desktop process could exit
        // (user closed the window, OS sent SIGTERM, etc.) while
        // the spawned `flowntier_runtime.exe` sidecar was still
        // alive with its named-pipe handles open. Next launch
        // would then hit `ERROR_ACCESS_DENIED` / `pipe busy`
        // because the kernel still held those handles.
        //
        // `RunEvent::ExitRequested` fires when the OS / window
        // manager asks the desktop to quit. We:
        //   1. Try to kill the sidecar by the PID we stored in
        //      `SidecarHandle` at spawn time (root-cause).
        //   2. As a belt-and-braces fallback (covers cases
        //      where PID is None, or the sidecar was spawned
        //      by a different process), also call
        //      `taskkill /f /im flowntier_runtime.exe`.
        //   3. Close stdin / stdout / stderr handles by
        //      dropping the rx task (we don't need to wait
        //      for the kill to land — the OS will reap the
        //      child shortly after we return from this handler
        //      and the desktop process is on its way out).
        //
        // We do NOT call `.prevent_exit()` — the user asked
        // us to quit, so we quit. We just make sure the
        // sidecar goes with us.
        .run(|_app_handle, event| {
            if let tauri::RunEvent::ExitRequested { .. } = &event {
                tracing::info!(
                    target: "flowntier_shell",
                    "event 000109: ExitRequested — killing sidecar before quit"
                );
                graceful_kill_sidecar();
            }
        });
}
