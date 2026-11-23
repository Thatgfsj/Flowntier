//! Secret storage commands (add/remove/reveal provider API keys).

use crate::sidecar::pipe_request;

#[tauri::command]
pub async fn list_secrets() -> Result<serde_json::Value, String> {
    pipe_request("GET", "/api/settings/secrets", None).await
}

#[tauri::command]
pub async fn save_secret(name: String, value: String) -> Result<serde_json::Value, String> {
    // 1) Persist to keychain via runtime.
    pipe_request(
        "PUT",
        &format!("/api/settings/secrets/{}", name),
        Some(serde_json::json!({ "value": value })),
    )
    .await?;

    // 2) Best-effort seed to os.environ. The keychain write already
    //    succeeded; we MUST NOT fail the user-visible save on a seed
    //    hiccup. Surface the warning so the UI can show "saved, but
    //    click Re-inject to retry the seed".
    let warning: Option<String> = match pipe_request(
        "POST",
        "/api/settings/secrets/seed",
        Some(serde_json::json!({})),
    )
    .await
    {
        Ok(_) => None,
        Err(e) => {
            eprintln!("[save_secret] seed post failed (non-fatal): {}", e);
            Some(format!("seed failed: {}", e))
        }
    };

    Ok(serde_json::json!({
        "saved": true,
        "warning": warning,
    }))
}

#[tauri::command]
pub async fn delete_secret(name: String) -> Result<(), String> {
    // The runtime returns 204 on success and 404 when the key was never
    // set; both are treated as success from the UI's perspective.
    match pipe_request("DELETE", &format!("/api/settings/secrets/{}", name), None).await {
        Ok(_) => Ok(()),
        Err(e) if e.contains("HTTP 404") => Ok(()),
        Err(e) => Err(e),
    }
}

#[tauri::command]
pub async fn reveal_secret(name: String) -> Result<String, String> {
    // v0.4.13: still returns String at the Tauri IPC boundary
    // (Tauri's serde_json serialization will clone it anyway),
    // but we explicitly null out the local pipe-response buffer
    // after extraction to minimise time-on-heap in this process.
    let data = pipe_request(
        "POST",
        &format!("/api/settings/secrets/{}/reveal", name),
        None,
    )
    .await?;
    let value = data["value"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "no value".to_string())?;
    // We can't zeroize the IPC payload (already serialized), but
    // we can drop the `data` reference now.
    drop(data);
    Ok(value)
}

#[tauri::command]
pub async fn seed_secrets() -> Result<Vec<String>, String> {
    let data = pipe_request(
        "POST",
        "/api/settings/secrets/seed",
        Some(serde_json::json!({})),
    )
    .await?;
    let seeded = data["seeded"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    Ok(seeded)
}
