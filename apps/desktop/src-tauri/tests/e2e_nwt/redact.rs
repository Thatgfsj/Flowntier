//! redact E2E tests.

use super::helpers::redact_secrets;

#[test]
fn e2e_boundary_redact_bearer_token() {
    let line = "2026-06-27 ERROR GET /v1/chat 401 Authorization: Bearer sk-supersecret123";
    let r = redact_secrets(line);
    assert!(
        !r.contains("sk-supersecret123"),
        "bearer token must be redacted, got: {r}"
    );
    assert!(r.contains("Bearer <redacted>"));
}

#[test]
fn e2e_boundary_redact_sk_prefix() {
    let line = "loaded OPENAI_API_KEY=sk-abc123def456ghi789jkl012mno";
    let r = redact_secrets(line);
    assert!(
        !r.contains("sk-abc123def456ghi789jkl012mno"),
        "sk- prefix must be redacted, got: {r}"
    );
    assert!(r.contains("sk-<redacted>"));
}

#[test]
fn e2e_boundary_redact_env_api_key() {
    let line = r#"{"OPENAI_API_KEY":"sk-abc123def456","ANTHROPIC_API_KEY":"sk-ant-xyz"}"#;
    let r = redact_secrets(line);
    // The KEY= pattern should redact the value. After redaction
    // the raw sk-abc value must not appear.
    assert!(
        !r.contains("sk-abc123def456"),
        "env API key value must be redacted, got: {r}"
    );
    // And the prefix should remain redacted too (Pattern 2).
    assert!(
        !r.contains("sk-ant-xyz"),
        "anthropic key must be redacted, got: {r}"
    );
}

#[test]
fn e2e_boundary_redact_preserves_non_secrets() {
    let line = "GET /api/users 200 in 12ms";
    let r = redact_secrets(line);
    assert_eq!(r, line, "non-secret lines must be unchanged");
}

#[test]
fn e2e_boundary_redact_stripe_style() {
    let line = "stripe key: sk_live_abcdef1234567890XYZ";
    let r = redact_secrets(line);
    assert!(
        !r.contains("abcdef1234567890XYZ"),
        "stripe live key body must be redacted, got: {r}"
    );
    assert!(r.contains("sk_live_<redacted>"));
}

#[test]
fn e2e_boundary_redact_password_kv() {
    let line = "DB_PASSWORD=hunter2 REDIS_PASSWORD=secret123";
    let r = redact_secrets(line);
    assert!(
        !r.contains("hunter2"),
        "DB_PASSWORD value must be redacted, got: {r}"
    );
    assert!(
        !r.contains("secret123"),
        "REDIS_PASSWORD value must be redacted, got: {r}"
    );
}
