//! Probe MiniMax over OpenAI-compat. Tries `/v1/chat/completions`
//! (the standard path) and prints whatever comes back. Useful
//! to figure out whether MiniMax accepts OpenAI-compat or only
//! Anthropic.
//!
//! Run with:
//!   MINIMAX_API_KEY=... cargo run -p agent-core --example probe_minimax --release

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> anyhow::Result<()> {
    let api_key = std::env::var("MINIMAX_API_KEY")
        .map_err(|_| anyhow::anyhow!("MINIMAX_API_KEY env var required"))?;

    for (label, base, path, body) in [
        (
            "openai-compat /v1/chat/completions",
            "https://api.minimaxi.com/v1",
            "/chat/completions",
            r#"{"model":"MiniMax-Text-01","max_tokens":32,"stream":false,"messages":[{"role":"user","content":"用中文一句话回答：1+1=?"}]}"#,
        ),
        (
            "openai-compat /v1/text/chatcompletion_v2",
            "https://api.minimaxi.com/v1",
            "/text/chatcompletion_v2",
            r#"{"model":"MiniMax-Text-01","max_tokens":32,"stream":false,"messages":[{"role":"user","content":"用中文一句话回答：1+1=?"}]}"#,
        ),
        (
            "anthropic /v1/messages",
            "https://api.minimaxi.com",
            "/v1/messages",
            r#"{"model":"MiniMax-Text-01","max_tokens":32,"messages":[{"role":"user","content":"用中文一句话回答：1+1=?"}]}"#,
        ),
    ] {
        let url = format!("{base}{path}");
        eprintln!("\n--- {label}");
        eprintln!("→ {url}");
        let resp = reqwest::Client::new()
            .post(&url)
            .header("authorization", format!("Bearer {api_key}"))
            .header("content-type", "application/json")
            .header("accept", "application/json")
            .body(body.to_string())
            .timeout(std::time::Duration::from_secs(20))
            .send()
            .await;
        match resp {
            Ok(r) => {
                let status = r.status();
                let txt = r.text().await.unwrap_or_default();
                let snippet: String = txt.chars().take(400).collect();
                eprintln!("← HTTP {status}");
                eprintln!("{snippet}{}", if txt.chars().count() > 400 { "…" } else { "" });
            }
            Err(e) => eprintln!("← ERR {e}"),
        }
    }
    Ok(())
}