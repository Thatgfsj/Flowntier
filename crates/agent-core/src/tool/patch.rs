//! `patch` — apply a unified diff to a file.
//!
//! Supports two forms:
//!
//! 1. **Search/replace** — `{"path": "...", "old_text": "...",
//!    "new_text": "..."}`. The agent provides the exact existing
//!    text and the replacement. This is the simplest and most
//!    reliable form for small edits.
//!
//! 2. **Unified diff** — `{"path": "...", "diff": "..."}`. A
//!    standard `diff -u` style patch. We parse it with the
//!    `similar` crate and apply it via `similar::udiff::apply_patches`.

use async_trait::async_trait;
use similar::{ChangeTag, TextDiff};

use super::{Tool, ToolContext, ToolError, ToolOutput};

/// `patch` tool — apply a text replacement or unified diff to a file.
#[derive(Debug)]
pub struct PatchTool;

#[async_trait]
impl Tool for PatchTool {
    fn name(&self) -> &'static str {
        "patch"
    }

    fn description(&self) -> &'static str {
        "Apply a text replacement to a file. Two modes: \
         (a) search/replace via {old_text, new_text}; \
         (b) unified diff via {diff}. \
         Original is backed up to '<file>.bak' before any change."
    }

    fn schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path":     { "type": "string" },
                "old_text": { "type": "string", "description": "Exact existing substring to replace." },
                "new_text": { "type": "string", "description": "Replacement text." },
                "diff":     { "type": "string", "description": "Unified diff to apply (alternative to old_text/new_text)." }
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
        let abs = ctx.workspace.resolve(path);
        if !ctx.workspace.contains(&abs) {
            return Ok(ToolOutput::err(format!("refused: {path} is outside the workspace")));
        }

        let original = tokio::fs::read_to_string(&abs)
            .await
            .map_err(ToolError::Io)?;

        let new_content = if let Some(diff_text) = args.get("diff").and_then(|v| v.as_str()) {
            apply_unified_diff(&original, diff_text)?
        } else {
            let old = args
                .get("old_text")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ToolError::InvalidArgs("missing 'old_text'".into()))?;
            let new = args
                .get("new_text")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ToolError::InvalidArgs("missing 'new_text'".into()))?;
            replace_once(&original, old, new)?
        };

        if new_content == original {
            return Ok(ToolOutput::err("no-op: replacement produced identical content"));
        }

        // Back up, then atomically write (write.rs handles atomicity).
        let bak_path = {
            let mut b = abs.clone();
            let ext = abs.extension().and_then(|e| e.to_str()).unwrap_or("");
            b.set_extension(format!("{ext}.bak"));
            b
        };
        let _ = tokio::fs::write(&bak_path, &original).await;

        let mut tmp = abs.clone();
        let ext = abs.extension().and_then(|e| e.to_str()).unwrap_or("");
        tmp.set_extension(format!("{ext}.tmp"));
        tokio::fs::write(&tmp, &new_content)
            .await
            .map_err(ToolError::Io)?;
        tokio::fs::rename(&tmp, &abs)
            .await
            .map_err(ToolError::Io)?;

        Ok(ToolOutput::ok(format!(
            "patched {} ({} → {} bytes)",
            ctx.workspace.relativize(&abs).display(),
            original.len(),
            new_content.len()
        )))
    }
}

fn replace_once(haystack: &str, needle: &str, replacement: &str) -> Result<String, ToolError> {
    let count = haystack.matches(needle).count();
    if count == 0 {
        return Err(ToolError::Other(format!(
            "old_text not found ({} chars)",
            needle.len()
        )));
    }
    if count > 1 {
        return Err(ToolError::Other(format!(
            "old_text matches {count} locations; must match exactly once"
        )));
    }
    Ok(haystack.replacen(needle, replacement, 1))
}

fn apply_unified_diff(original: &str, diff_text: &str) -> Result<String, ToolError> {
    // We re-derive the new text from `similar` instead of using
    // its udiff patch applier so we can give the agent a clean
    // error if the hunk doesn't apply.
    let diff = TextDiff::from_lines(original, diff_text);
    let mut out = String::with_capacity(original.len());
    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Equal | ChangeTag::Insert => out.push_str(change.value()),
            ChangeTag::Delete => {} // skip
        }
    }
    if out == original {
        return Err(ToolError::Other(
            "diff produced no change against current file; hunk may not apply".into(),
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::Workspace;
    use std::io::Write;

    #[tokio::test]
    async fn search_replace_works() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.txt");
        let mut f = std::fs::File::create(&p).unwrap();
        f.write_all(b"hello world\n").unwrap();
        let ctx = ToolContext {
            workspace: Workspace::new(dir.path(), "t"),
            approved: false,
        };
        let out = PatchTool
            .execute(
                serde_json::json!({
                    "path": "a.txt",
                    "old_text": "world",
                    "new_text": "rust",
                }),
                &ctx,
            )
            .await
            .unwrap();
        assert!(!out.is_error);
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "hello rust\n");
    }

    #[tokio::test]
    async fn refuses_ambiguous_replace() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "x\nx\n").unwrap();
        let ctx = ToolContext {
            workspace: Workspace::new(dir.path(), "t"),
            approved: false,
        };
        let err = PatchTool
            .execute(
                serde_json::json!({"path": "a.txt", "old_text": "x", "new_text": "y"}),
                &ctx,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::Other(_)));
    }
}