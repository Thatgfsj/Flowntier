//! Storage layer: SQLite + FTS5 for Flowntier.
//!
//! All database access goes through this crate. No raw SQL elsewhere.
//!
//! See `docs/STORAGE_SPEC.md` for the full schema.

#![forbid(unsafe_code)]
#![allow(missing_docs)]
#![allow(clippy::type_complexity, clippy::doc_overindented_list_items)]

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use thiserror::Error;

mod models;
mod repository;
mod secrets;
mod tasks;
mod usage;
mod workflows;

pub use models::*;
pub use repository::*;
pub use secrets::*;
pub use tasks::*;
pub use usage::*;
pub use workflows::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn create_and_fetch_workflow() {
        let repo = Repository::open_in_memory().await.expect("open");
        let now = chrono::Utc::now().timestamp();
        let wf = Workflow {
            id: "wf_01TEST".into(),
            created_at: now,
            updated_at: now,
            state: "REQ_RECEIVED".into(),
            phase: "1-requirement".into(),
            user_request: "Add /login".into(),
            plan_doc: None,
            summary: None,
            final_status: None,
            total_input_tokens: 0,
            total_output_tokens: 0,
            total_cost_usd: None,
        };
        repo.create_workflow(&wf).await.expect("create");
        let fetched = repo.get_workflow("wf_01TEST").await.expect("fetch");
        assert_eq!(fetched, Some(wf));
    }
}
