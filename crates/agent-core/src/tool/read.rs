//! `read` — read a file (optionally a line range) from the workspace.

use async_trait::async_trait;
use std::path::Path;

use super::{Tool, ToolContext, ToolError, ToolOutput};

/// `read` tool — print a file (or a line range) with line numbers.
#[derive(Debug)]
pub struct ReadTool;

#[async_trait]
impl Tool for ReadTool {
    fn name(&self) -> &'static str {
        "read"
    }

    fn description(&self) -> &'static str {
        "Read a file's content, optionally restricted to a 1-based line range. \
         Returns the file with line numbers. Path is relative to the workspace root."
    }

    fn schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "File path, relative to the workspace root." },
                "start_line": { "type": "integer", "minimum": 1, "description": "First line to read (1-based, inclusive)." },
                "end_line":   { "type": "integer", "minimum": 1, "description": "Last line to read (1-based, inclusive)." }
            },
            "required": ["path"],
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
        let start_line = args.get("start_line").and_then(|v| v.as_u64()).map(|n| n as usize);
        let end_line = args.get("end_line").and_then(|v| v.as_u64()).map(|n| n as usize);

        let abs = ctx.workspace.resolve(path);
        if !ctx.workspace.contains(&abs) {
            return Ok(ToolOutput::err(format!("refused: {path} is outside the workspace")));
        }
        read(&abs, start_line, end_line).await
    }
}

async fn read(abs: &Path, start: Option<usize>, end: Option<usize>) -> Result<ToolOutput, ToolError> {
    let raw = tokio::fs::read_to_string(abs).await.map_err(ToolError::Io)?;
    let total_lines = raw.lines().count();
    let s = start.unwrap_or(1).max(1);
    let e = end.unwrap_or(total_lines).min(total_lines.max(s));
    let mut out = String::new();
    for (idx, line) in raw.lines().enumerate() {
        let n = idx + 1;
        if n < s { continue; }
        if n > e { break; }
        out.push_str(&format!("{n:>5}  {line}\n"));
    }
    if out.is_empty() {
        out.push_str("(empty file)\n");
    }
    out.push_str(&format!("\n--- {total_lines} lines total ---"));
    Ok(ToolOutput::ok(out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::Workspace;
    use std::io::Write;

    #[tokio::test]
    async fn reads_with_line_numbers() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.txt");
        let mut f = std::fs::File::create(&p).unwrap();
        writeln!(f, "one").unwrap();
        writeln!(f, "two").unwrap();
        writeln!(f, "three").unwrap();
        let ctx = ToolContext {
            workspace: Workspace::new(dir.path(), "t"),
            approved: false,
        };
        let out = ReadTool
            .execute(serde_json::json!({"path": "a.txt", "start_line": 2, "end_line": 2}), &ctx)
            .await
            .unwrap();
        assert!(out.content.contains("2  two"));
        assert!(!out.content.contains("one"));
        assert!(!out.content.contains("three"));
    }
}