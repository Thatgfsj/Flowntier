//! Secret store routes.

use super::*;

pub(super) fn register(d: &mut Dispatcher, state: Arc<ServerState>) {
    // ── Secrets: v0.4 real persistence (OS keystore + AES-GCM).
    // The Tauri shell writes a plaintext via PUT
    // /api/settings/secrets/{name}; the pipe server encrypts
    // with the OS keystore DEK and stores ciphertext + nonce
    // in SQLite. The plaintext is never returned to the UI
    // (only metadata). The agent loop calls GET
    // /api/settings/secrets/{name}/reveal to fetch the plaintext
    // at runtime.
    let list_state = state.clone();
    d.register("GET", "/api/settings/secrets", move |_body| {
        let s = list_state.clone();
        Box::pin(async move {
            match s.secrets.list().await {
                Ok(list) => Ok((200, json!({ "secrets": list, "count": list.len() }))),
                Err(e) => Ok((500, json!({ "error": format!("list_secrets: {e}") }))),
            }
        })
    });

    let put_state = state.clone();
    // v0.4.30 (audit 000130): `{name+}` so the trailing
    // wildcard placeholder can absorb the `/` in secret
    // names that live under the `flowntier/<id>` namespace.
    d.register("PUT", "/api/settings/secrets/{name+}", move |body| {
        let s = put_state.clone();
        Box::pin(async move {
            let name = body
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "missing 'name' in path".to_string())?
                .to_string();
            let value = body
                .get("value")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "missing 'value' in body".to_string())?
                .to_string();
            if value.len() > 4096 {
                return Ok((400, json!({ "error": "secret value exceeds 4 KiB cap" })));
            }
            if name.is_empty() || name.len() > 128 {
                return Ok((400, json!({ "error": "secret name must be 1..=128 chars" })));
            }
            match s.secrets.put(&name, &value).await {
                Ok(()) => Ok((200, json!({ "saved": true, "name": name }))),
                Err(e) => Ok((500, json!({ "error": format!("put_secret: {e}") }))),
            }
        })
    });

    let del_state = state.clone();
    d.register("DELETE", "/api/settings/secrets/{name+}", move |body| {
        let s = del_state.clone();
        Box::pin(async move {
            let name = body
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "missing 'name' in path".to_string())?
                .to_string();
            match s.secrets.delete(&name).await {
                Ok(removed) => Ok((200, json!({ "deleted": removed, "name": name }))),
                Err(e) => Ok((500, json!({ "error": format!("delete_secret: {e}") }))),
            }
        })
    });

    // Internal-only: returns plaintext. The Tauri shell's IPC
    // bridge should NOT expose this to the webview. Used by
    // the agent loop when making provider requests.
    let reveal_state = state.clone();
    d.register("GET", "/api/settings/secrets/{name+}/reveal", move |body| {
        let s = reveal_state.clone();
        Box::pin(async move {
            let name = body
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "missing 'name' in path".to_string())?
                .to_string();
            match s.secrets.reveal(&name).await {
                Ok(value) => Ok((200, json!({ "name": name, "value": value.as_str() }))),
                Err(crate::secrets::SecretStoreError::NotFound(_)) => {
                    Ok((404, json!({ "error": "not found", "name": name })))
                }
                Err(e) => Ok((500, json!({ "error": format!("reveal: {e}") }))),
            }
        })
    });

    // v0.4: trigger the legacy plaintext migration if a
    // v0.3-era secrets.json exists at <data_dir>/secrets.json.
    let seed_state = state.clone();
    d.register("POST", "/api/settings/secrets/seed", move |_body| {
        let s = seed_state.clone();
        Box::pin(async move {
            let legacy = s.secrets.data_dir().join("secrets.json");
            match s.secrets.migrate_legacy_plaintext(&legacy).await {
                Ok(n) => Ok((
                    200,
                    json!({
                        "seeded": n,
                        "source": legacy.display().to_string(),
                    }),
                )),
                Err(e) => Ok((500, json!({ "error": format!("seed: {e}") }))),
            }
        })
    });
}
