//! Workflow records and their persistence.

use super::*;

/// Status of a workflow in the `workflows` table.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WorkflowStatus {
    /// Active workflow.
    Active,
    /// Successfully completed.
    Done,
    /// Failed unrecoverably.
    Failed,
    /// User/system aborted.
    Aborted,
}

impl WorkflowStatus {
    /// String form used in the database.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "ACTIVE",
            Self::Done => "DONE",
            Self::Failed => "FAILED",
            Self::Aborted => "ABORTED",
        }
    }
}

/// A workflow row.
///
/// Note: derives `PartialEq` but not `Eq` because `total_cost_usd`
/// is `Option<f64>`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Workflow {
    /// Workflow id (ULID).
    pub id: String,
    /// Unix epoch seconds.
    pub created_at: i64,
    /// Unix epoch seconds.
    pub updated_at: i64,
    /// Current state.
    pub state: String,
    /// Current phase.
    pub phase: String,
    /// Original user request.
    pub user_request: String,
    /// Plan document (Markdown), if any.
    pub plan_doc: Option<String>,
    /// Final delivery summary.
    pub summary: Option<String>,
    /// Terminal status.
    pub final_status: Option<WorkflowStatus>,
    /// Total input tokens used.
    pub total_input_tokens: i64,
    /// Total output tokens used.
    pub total_output_tokens: i64,
    /// Total cost in USD.
    pub total_cost_usd: Option<f64>,
}

impl Repository {
    /// Create a new workflow row.
    pub async fn create_workflow(&self, wf: &Workflow) -> Result<(), StorageError> {
        sqlx::query(
            r#"
            INSERT INTO workflows
              (id, created_at, updated_at, state, phase, user_request,
               plan_doc, summary, final_status, total_input_tokens,
               total_output_tokens, total_cost_usd)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&wf.id)
        .bind(wf.created_at)
        .bind(wf.updated_at)
        .bind(&wf.state)
        .bind(&wf.phase)
        .bind(&wf.user_request)
        .bind(&wf.plan_doc)
        .bind(&wf.summary)
        .bind(wf.final_status.map(|s| s.as_str().to_string()))
        .bind(wf.total_input_tokens)
        .bind(wf.total_output_tokens)
        .bind(wf.total_cost_usd)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Idempotently ensure a `workflows` row exists for the given
    /// id. Used by the run_task handler (event 000064 follow-up)
    /// when a chat-derived `wf_chat_*` id doesn't have a
    /// corresponding workflow yet — without this, the tasks table
    /// FK constraint blocks the subsequent INSERT and the
    /// dashboard stays at "0/0 完成".
    ///
    /// `INSERT OR IGNORE` makes the call safe under concurrent
    /// invocations: the first caller creates the row, the rest
    /// no-op. Real (non-synthetic) wf_ids that already point at
    /// a workflows row also no-op.
    pub async fn ensure_workflow_row(
        &self,
        id: &str,
        user_request: &str,
        final_status: &str,
    ) -> Result<(), StorageError> {
        let now = chrono::Utc::now().timestamp();
        sqlx::query(
            r#"
            INSERT OR IGNORE INTO workflows
              (id, created_at, updated_at, state, phase, user_request,
               summary, final_status)
            VALUES (?, ?, ?, 'ACTIVE', '1-requirement', ?, ?, ?)
            "#,
        )
        .bind(id)
        .bind(now)
        .bind(now)
        .bind(user_request)
        .bind(Some(user_request))
        .bind(Some(final_status))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Update the state and phase of a workflow.
    pub async fn update_workflow_state(
        &self,
        id: &str,
        state: &str,
        phase: &str,
    ) -> Result<(), StorageError> {
        let now = chrono::Utc::now().timestamp();
        sqlx::query("UPDATE workflows SET state = ?, phase = ?, updated_at = ? WHERE id = ?")
            .bind(state)
            .bind(phase)
            .bind(now)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// v0.4.22 (event 000069): persist the final workflow
    /// summary so /api/workflow/{wf_id}/status can return it
    /// after the orchestrator's delivery phase finishes.
    pub async fn set_workflow_summary(&self, id: &str, summary: &str) -> Result<(), StorageError> {
        let now = chrono::Utc::now().timestamp();
        sqlx::query("UPDATE workflows SET summary = ?, updated_at = ? WHERE id = ?")
            .bind(summary)
            .bind(now)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Persist the workflow planning doc (JSON) so /api/workflow/{wf_id}/status
    /// and get_workflow IPC command can return the structured plan.
    pub async fn set_workflow_plan_doc(
        &self,
        id: &str,
        plan_doc: &str,
    ) -> Result<(), StorageError> {
        let now = chrono::Utc::now().timestamp();
        sqlx::query("UPDATE workflows SET plan_doc = ?, updated_at = ? WHERE id = ?")
            .bind(plan_doc)
            .bind(now)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Fetch a workflow by id.
    pub async fn get_workflow(&self, id: &str) -> Result<Option<Workflow>, StorageError> {
        let row: Option<WorkflowRow> = sqlx::query_as("SELECT * FROM workflows WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(Into::into))
    }
}

#[derive(sqlx::FromRow)]
struct WorkflowRow {
    id: String,
    created_at: i64,
    updated_at: i64,
    state: String,
    phase: String,
    user_request: String,
    plan_doc: Option<String>,
    summary: Option<String>,
    final_status: Option<String>,
    total_input_tokens: i64,
    total_output_tokens: i64,
    total_cost_usd: Option<f64>,
}

impl From<WorkflowRow> for Workflow {
    fn from(r: WorkflowRow) -> Self {
        Self {
            id: r.id,
            created_at: r.created_at,
            updated_at: r.updated_at,
            state: r.state,
            phase: r.phase,
            user_request: r.user_request,
            plan_doc: r.plan_doc,
            summary: r.summary,
            final_status: r.final_status.as_deref().and_then(workflow_status_from_str),
            total_input_tokens: r.total_input_tokens,
            total_output_tokens: r.total_output_tokens,
            total_cost_usd: r.total_cost_usd,
        }
    }
}

fn workflow_status_from_str(s: &str) -> Option<WorkflowStatus> {
    match s {
        "ACTIVE" => Some(WorkflowStatus::Active),
        "DONE" => Some(WorkflowStatus::Done),
        "FAILED" => Some(WorkflowStatus::Failed),
        "ABORTED" => Some(WorkflowStatus::Aborted),
        _ => None,
    }
}
