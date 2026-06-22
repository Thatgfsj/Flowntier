//! `write` — write content to a file atomically (with `.bak`).

use async_trait::async_trait;
use std::path::PathBuf;
use tokio::io::AsyncWriteExt;

use super::{Tool, ToolContext, ToolError, ToolOutput};

/// `write` tool — overwrite or create a file (with `.bak` backup).
#[derive(Debug)]
pub struct WriteTool;

#[async_trait]
impl Tool for WriteTool {
    fn name(&self) -> &'static str {
        "write"
    }

    fn description(&self) -> &'static str {
        "Write content to a file, creating it if needed. \
         On overwrite, the previous content is saved as '<file>.bak'. \
         Path is relative to the workspace root."
    }

    fn schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path":    { "type": "string" },
                "content": { "type": "string", "description": "Full file contents to write." }
            },
            "required": ["path", "content"],
            "additionalProperties": false
        })
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("missing 'path'".into()))?;
        let content = args
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("missing 'content'".into()))?;

        let abs = ctx.workspace.resolve(path);
        if !ctx.workspace.contains(&abs) {
            return Ok(ToolOutput::err(format!("refused: {path} is outside the workspace")));
        }

        write_atomic(&abs, content).await?;
        let bytes = content.as_bytes().len();
        Ok(ToolOutput::ok(format!(
            "wrote {} bytes to {}",
            bytes,
            ctx.workspace.relativize(&abs).display()
        )))
    }
}

async fn write_atomic(abs: &PathBuf, content: &str) -> Result<(), ToolError> {
    if let Some(parent) = abs.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(ToolError::Io)?;
    }

    // Back up the existing file (if any) before overwriting.
    if tokio::fs::try_exists(abs).await.unwrap_or(false) {
        let mut bak = abs.clone();
        bak.set_extension(format!(
            "{}.bak",
            abs.extension().and_then(|e| e.to_str()).unwrap_or("")
        ));
        // ignore errors — backup is best-effort
        let _ = tokio::fs::copy(abs, &bak).await;
    }

    // Write to a sibling temp file in the same directory so the
    // final rename is on the same filesystem (atomic on POSIX,
    // near-atomic on Windows).
    let mut tmp = abs.clone();
    tmp.set_extension(format!(
        "{}.tmp",
        abs.extension().and_then(|e| e.to_str()).unwrap_or("")
    ));
    {
        let mut f = tokio::fs::File::create(&tmp).await.map_err(ToolError::Io)?;
        f.write_all(content.as_bytes()).await.map_err(ToolError::Io)?;
        f.sync_all().await.map_err(ToolError::Io)?;
    }
    tokio::fs::rename(&tmp, abs).await.map_err(ToolError::Io)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::Workspace;

    #[tokio::test]
    async fn writes_new_file() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ToolContext {
            workspace: Workspace::new(dir.path(), "t"),
            approved: false,
        };
        let out = WriteTool
            .execute(serde_json::json!({"path": "a.txt", "content": "hi"}), &ctx)
            .await
            .unwrap();
        assert!(!out.is_error);
        assert_eq!(std::fs::read_to_string(dir.path().join("a.txt")).unwrap(), "hi");
    }

    #[tokio::test]
    async fn overwrites_with_backup() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "old").unwrap();
        let ctx = ToolContext {
            workspace: Workspace::new(dir.path(), "t"),
            approved: false,
        };
        WriteTool
            .execute(serde_json::json!({"path": "a.txt", "content": "new"}), &ctx)
            .await
            .unwrap();
        assert_eq!(std::fs::read_to_string(dir.path().join("a.txt")).unwrap(), "new");
        let bak = dir.path().join("a.txt.bak");
        assert_eq!(std::fs::read_to_string(&bak).unwrap(), "old");
    }
}