//! Tests for audit 000132 — cascading cleanup of `role_overrides`
//! when a provider's API key disappears.
//!
//! Root cause recap: the user removed the API key for a built-in
//! preset (e.g. Xiaomi MiMo) but `role_overrides.default_model`
//! still pinned a workflow role to `mimo:mimo-v1`. The
//! orchestrator kept resolving that role, hit an empty keychain
//! reveal, and surfaced a 503. The user could never see the
//! "role not configured" hint because the assignment was never
//! cleared.
//!
//! The fix is two storage primitives:
//!   - `Repository::clear_role_overrides_for_provider(provider)` —
//!     scan every role_overrides row and rewrite any whose
//!     `default_model` or any element of `fallback_chain` starts
//!     with `<provider>:`.
//!   - `Repository::set_provider_enabled(id, bool)` — flip the
//!     `enabled` flag on a built-in provider row.
//!
//! These tests exercise both primitives in isolation (and one
//! integration scenario that mirrors the real bug).

use storage::Repository;

async fn fresh_repo() -> Repository {
    Repository::open_in_memory().await.expect("open in-memory")
}

#[tokio::test]
async fn audit_000132_clears_default_model_when_provider_referenced() {
    let repo = fresh_repo().await;

    // Two roles, both pinned at mimo. chief's default is the
    // exact field that resolve_role reads first.
    repo.upsert_role_override("agent:chief", "mimo:mimo-v1", &[])
        .await
        .unwrap();
    repo.upsert_role_override("agent:worker", "mimo:mimo-v1", &[])
        .await
        .unwrap();
    // critic:a is bound to a different provider and must stay
    // untouched — proves we don't blanket-clear the table.
    repo.upsert_role_override("agent:critic:a", "minimax:MiniMax-Text-01", &[])
        .await
        .unwrap();

    let n = repo
        .clear_role_overrides_for_provider("mimo")
        .await
        .unwrap();
    assert_eq!(n, 2, "two rows reference mimo, both must be rewritten");

    // chief: default cleared (empty string), fallback_chain []
    let chief = repo
        .get_role_override("agent:chief")
        .await
        .unwrap()
        .expect("chief row still present (we update, not delete)");
    assert_eq!(
        chief.default_model, "",
        "chief.default_model must be cleared"
    );
    assert!(chief.fallback_chain.is_empty());

    // worker: same as chief
    let worker = repo
        .get_role_override("agent:worker")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(worker.default_model, "");
    assert!(worker.fallback_chain.is_empty());

    // critic:a: untouched
    let critic = repo
        .get_role_override("agent:critic:a")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(critic.default_model, "minimax:MiniMax-Text-01");
}

#[tokio::test]
async fn audit_000132_clears_fallback_chain_only() {
    let repo = fresh_repo().await;

    // chief's primary is minimax (still valid), but its fallback
    // chain contains a stale mimo reference. This is the exact
    // shape of the production bug for users who set up a fallback
    // ladder before rotating providers.
    repo.upsert_role_override(
        "agent:chief",
        "minimax:MiniMax-Text-01",
        &["mimo:mimo-v1".to_string(), "minimax:MiniMax-M3".to_string()],
    )
    .await
    .unwrap();

    let n = repo
        .clear_role_overrides_for_provider("mimo")
        .await
        .unwrap();
    assert_eq!(n, 1, "chain element matched, row was rewritten");

    let chief = repo
        .get_role_override("agent:chief")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        chief.default_model, "minimax:MiniMax-Text-01",
        "primary stays — only the chain entry pointing at mimo is removed"
    );
    assert_eq!(
        chief.fallback_chain,
        vec!["minimax:MiniMax-M3".to_string()],
        "the mimo entry was filtered, the minimax entry remains"
    );
}

#[tokio::test]
async fn audit_000132_clears_both_default_and_chain() {
    let repo = fresh_repo().await;

    // chief has mimo as BOTH default AND in the chain — should
    // still produce a single touched row but fully de-mimo'd.
    repo.upsert_role_override(
        "agent:chief",
        "mimo:mimo-v1",
        &["mimo:mimo-v1".to_string(), "minimax:MiniMax-M3".to_string()],
    )
    .await
    .unwrap();

    let n = repo
        .clear_role_overrides_for_provider("mimo")
        .await
        .unwrap();
    assert_eq!(n, 1);

    let chief = repo
        .get_role_override("agent:chief")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(chief.default_model, "");
    assert_eq!(chief.fallback_chain, vec!["minimax:MiniMax-M3".to_string()]);
}

#[tokio::test]
async fn audit_000132_returns_zero_when_no_references() {
    let repo = fresh_repo().await;

    repo.upsert_role_override("agent:chief", "minimax:MiniMax-Text-01", &[])
        .await
        .unwrap();

    let n = repo
        .clear_role_overrides_for_provider("mimo")
        .await
        .unwrap();
    assert_eq!(n, 0, "no rows touched when nothing references the provider");

    // chief row is byte-identical
    let chief = repo
        .get_role_override("agent:chief")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(chief.default_model, "minimax:MiniMax-Text-01");
}

#[tokio::test]
async fn audit_000132_does_not_match_substring_providers() {
    let repo = fresh_repo().await;

    // Edge case: a provider id that contains another provider id
    // as a prefix must not collide. `mimo-2` is a different
    // provider from `mimo` and the cascade for `mimo` must not
    // touch rows that only reference `mimo-2:*`.
    repo.upsert_role_override("agent:chief", "mimo-2:v1", &[])
        .await
        .unwrap();

    let n = repo
        .clear_role_overrides_for_provider("mimo")
        .await
        .unwrap();
    assert_eq!(
        n, 0,
        "`mimo:` prefix must not match `mimo-2:v1` (no colon after mimo)"
    );

    let chief = repo
        .get_role_override("agent:chief")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(chief.default_model, "mimo-2:v1", "untouched");
}

#[tokio::test]
async fn audit_000132_set_provider_enabled_flips_flag() {
    let repo = fresh_repo().await;

    // migration 0003 pre-seeds provider rows for the 9 built-in
    // presets. mimo should be present and enabled.
    let before = repo
        .get_provider("mimo")
        .await
        .unwrap()
        .expect("mimo provider row must exist (pre-seeded by 0003)");
    assert!(before.enabled, "precondition: mimo is enabled by default");

    let flipped = repo.set_provider_enabled("mimo", false).await.unwrap();
    assert!(
        flipped,
        "set_provider_enabled must report a row was updated"
    );

    let after = repo.get_provider("mimo").await.unwrap().unwrap();
    assert!(!after.enabled, "mimo is now disabled");

    // Flipping back is supported.
    let flipped_back = repo.set_provider_enabled("mimo", true).await.unwrap();
    assert!(flipped_back);
    let final_state = repo.get_provider("mimo").await.unwrap().unwrap();
    assert!(final_state.enabled);
}

#[tokio::test]
async fn audit_000132_set_provider_enabled_no_op_for_unknown_id() {
    let repo = fresh_repo().await;

    // update returns 0 rows when the id is not in the table —
    // we MUST NOT auto-INSERT (that would silently create a
    // malformed provider row).
    let flipped = repo
        .set_provider_enabled("nope-not-a-real-provider", true)
        .await
        .unwrap();
    assert!(
        !flipped,
        "set_provider_enabled must report no row was touched"
    );
}

#[tokio::test]
async fn audit_000132_integration_secret_missing_triggers_cascade() {
    // Simulates the full user journey that triggered the bug:
    //   1. user had mimo configured with a default role
    //   2. user deletes the API key from Settings
    //   3. resolve_role runs and finds the key missing
    //      (callable directly here via the repo helpers; the
    //      keychain reveal stub is the absence of a secret row)
    //
    // We assert that AFTER the cascade (the production
    // `resolve_role` cascade handler in handlers.rs calls
    // these two repo methods), the role is no longer pinned
    // to mimo and the provider row reads disabled.

    let repo = fresh_repo().await;

    // Initial setup: chief pinned at mimo, mimo enabled.
    repo.upsert_role_override("agent:chief", "mimo:mimo-v1", &[])
        .await
        .unwrap();
    let before = repo.get_provider("mimo").await.unwrap().unwrap();
    assert!(before.enabled);

    // Production cascade handler:
    //   let n = repo.clear_role_overrides_for_provider("mimo").await?;
    //   let _ = repo.set_provider_enabled("mimo", false).await;
    let n = repo
        .clear_role_overrides_for_provider("mimo")
        .await
        .unwrap();
    assert_eq!(n, 1, "exactly chief row was rewritten");
    let _ = repo.set_provider_enabled("mimo", false).await.unwrap();

    // Post-cascade state: role assignment is gone, provider is
    // off. The next workflow run lands on the
    // `default_model.is_empty()` branch in resolve_role which
    // surfaces "open Settings → 角色 → 模型 分配" — the only
    // error message that points the user at a fixable action.
    let chief = repo
        .get_role_override("agent:chief")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(chief.default_model, "");

    let mimo = repo.get_provider("mimo").await.unwrap().unwrap();
    assert!(!mimo.enabled, "mimo provider row reflects 'not configured'");
}
