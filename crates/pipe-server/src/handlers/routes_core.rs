//! Core routes: ping, version handshake, provider list, run_task.

use super::*;

pub(super) fn register(d: &mut Dispatcher, state: Arc<ServerState>) {
    // Health check.
    let s1 = state.clone();
    d.register("GET", "/api/ping", move |_body| {
        let _ = &s1;
        Box::pin(async {
            Ok((
                200,
                json!({
                    "ok": true,
                    "runtime": "flowntier-rs",
                    "version": env!("CARGO_PKG_VERSION"),
                }),
            ))
        })
    });

    // Version handshake (v0.4). Returns the sidecar's version +
    // a min_compatible field the Tauri shell compares against the
    // shell's expected version. If sidecar < min_compatible, the
    // shell shows a drift banner.
    //
    // The shell learns about the sidecar through:
    //   - this endpoint (read by the desktop shell on startup)
    //   - the CARGO_PKG_VERSION env var (written by the desktop
    //     shell's build script and read by the spawned sidecar
    //     process). For v0.4 we only have the endpoint; the
    //     cross-process var is a v0.5 nicety.
    d.register("GET", "/api/rpc/version", |_body| {
        Box::pin(async {
            Ok((
                200,
                json!({
                    "sidecar": env!("CARGO_PKG_VERSION"),
                    "min_compatible": "0.4.0",
                    "build": "rust",
                }),
            ))
        })
    });

    // List providers — v0.4 reads the 9 built-in presets from
    // `providers::PRESETS` and joins them with the `provider`
    // table for per-preset overrides (enabled, default_model,
    // base_url) plus a `has_secret` flag from the secret store.
    // The result also includes any custom_provider rows.
    let s2 = state.clone();
    d.register("GET", "/api/providers", move |body| {
        let s = s2.clone();
        Box::pin(async move { list_providers(body, s).await })
    });

    // Run a workflow / task envelope. This is the central
    // entry point that the Tauri client will use to talk to
    // the agent loop. For now we require the caller to pass
    // a fully-formed provider spec — the router (W3 follow-up)
    // will resolve names to providers.
    let s3 = state.clone();
    d.register("POST", "/api/run_task", move |body| {
        let state = s3.clone();
        Box::pin(async move { run_task(body, state).await })
    });
}
