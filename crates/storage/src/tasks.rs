//! Task rows.

use super::*;

/// A single task row, the unit of work the chairman sees in
/// the dashboard's "任务列表 0/0 完成" panel. Today the schema
/// supports the lifecycle (queued/running/done/failed) but
/// the agent loop never persisted rows into it (event 000064).
/// We now write a single row per run_task so the dashboard
/// shows real progress.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Task {
    pub id: String,
    pub wf_id: String,
    pub parent_id: Option<String>,
    pub title: String,
    pub status: String, // "queued" | "running" | "done" | "failed"
    pub assigned_to: Option<String>,
    pub model: Option<String>,
    pub repair_count: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cost_usd: Option<f64>,
    pub files_modified: Option<String>,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub result: Option<String>,
}

impl Repository {
    /// Read a single (role, model) row. Returns None if absent.
    /// v0.4.21 (event 000064): insert a task row. Used by the
    /// run_task handler to persist a single row per chat call so
    /// the dashboard's "任务列表 0/0 完成" panel shows real
    /// progress. The agent loop historically only wrote
    /// AgentEvent streams; the task table was always empty.
    pub async fn create_task(&self, t: &Task) -> Result<(), StorageError> {
        sqlx::query(
            r#"
            INSERT INTO tasks
              (id, wf_id, parent_id, title, status, assigned_to, model,
               repair_count, input_tokens, output_tokens, cost_usd,
               files_modified, started_at, finished_at, result)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&t.id)
        .bind(&t.wf_id)
        .bind(&t.parent_id)
        .bind(&t.title)
        .bind(&t.status)
        .bind(&t.assigned_to)
        .bind(&t.model)
        .bind(t.repair_count)
        .bind(t.input_tokens)
        .bind(t.output_tokens)
        .bind(t.cost_usd)
        .bind(&t.files_modified)
        .bind(t.started_at)
        .bind(t.finished_at)
        .bind(&t.result)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// List every task for a workflow, ordered by started_at.
    /// Returned shape is the public Task struct. The dashboard
    /// uses this to render the "任务列表" panel.
    pub async fn list_tasks_for(&self, wf_id: &str) -> Result<Vec<Task>, StorageError> {
        let rows: Vec<(
            String,
            String,
            Option<String>,
            String,
            String,
            Option<String>,
            Option<String>,
            i64,
            i64,
            i64,
            Option<f64>,
            Option<String>,
            Option<i64>,
            Option<i64>,
            Option<String>,
        )> = sqlx::query_as(
            "SELECT id, wf_id, parent_id, title, status, assigned_to,
                    model, repair_count, input_tokens, output_tokens,
                    cost_usd, files_modified, started_at, finished_at,
                    result
             FROM tasks WHERE wf_id = ? ORDER BY started_at, id",
        )
        .bind(wf_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(
                    id,
                    wf,
                    parent,
                    title,
                    status,
                    assigned,
                    model,
                    rep,
                    in_t,
                    out_t,
                    cost,
                    files,
                    started,
                    finished,
                    result,
                )| Task {
                    id,
                    wf_id: wf,
                    parent_id: parent,
                    title,
                    status,
                    assigned_to: assigned,
                    model,
                    repair_count: rep,
                    input_tokens: in_t,
                    output_tokens: out_t,
                    cost_usd: cost,
                    files_modified: files,
                    started_at: started,
                    finished_at: finished,
                    result,
                },
            )
            .collect())
    }

    /// v0.4.21 (event 000064): tasks table was always empty
    /// before this fix. Exposed for the dashboard's "任务列表"
    /// summary: 0/0 → 0/0 was a bug, not a feature.
    pub async fn count_tasks(&self, wf_id: &str) -> Result<(i64, i64), StorageError> {
        let row: (i64, i64) = sqlx::query_as(
            "SELECT
               SUM(CASE WHEN status = 'done' THEN 1 ELSE 0 END),
               COUNT(*)
             FROM tasks WHERE wf_id = ?",
        )
        .bind(wf_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }
}
