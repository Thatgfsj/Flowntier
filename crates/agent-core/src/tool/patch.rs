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
        if !ctx.capabilities.write {
            return Ok(ToolOutput::err(
                "refused: write capability disabled (patch)",
            ));
        }
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("missing 'path'".into()))?;
        let abs = ctx.workspace.resolve(path);
        if let Err(e) = ctx.workspace.verify_path(&abs) {
            return Ok(ToolOutput::err(format!("refused: {e}")));
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
            return Ok(ToolOutput::err(
                "no-op: replacement produced identical content",
            ));
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
        tokio::fs::rename(&tmp, &abs).await.map_err(ToolError::Io)?;

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
    if count == 1 {
        return Ok(haystack.replacen(needle, replacement, 1));
    }
    if count > 1 {
        return Err(ToolError::Other(format!(
            "old_text matches {count} locations; must match exactly once. \
             Include more surrounding context to disambiguate"
        )));
    }

    // Direct match failed (count == 0). Try CRLF / LF normalization.
    // Windows files frequently have \r\n while LLM-generated edits use \n.
    if haystack.contains("\r\n") || needle.contains("\r\n") {
        let haystack_lf = haystack.replace("\r\n", "\n");
        let needle_lf = needle.replace("\r\n", "\n");
        let replacement_lf = replacement.replace("\r\n", "\n");

        let count_lf = haystack_lf.matches(&needle_lf).count();
        if count_lf == 1 {
            let replaced_lf = haystack_lf.replacen(&needle_lf, &replacement_lf, 1);
            return if haystack.contains("\r\n") {
                Ok(replaced_lf.replace('\n', "\r\n"))
            } else {
                Ok(replaced_lf)
            };
        }
        if count_lf > 1 {
            return Err(ToolError::Other(format!(
                "old_text matches {count_lf} locations (after CRLF normalization); must match exactly once. \
                 Include more surrounding context to disambiguate"
            )));
        }
    }

    Err(ToolError::Other(format!(
        "old_text not found in file (needle was {} chars); \
         check whitespace, indentation, line endings, or re-read the file first",
        needle.len()
    )))
}

fn apply_unified_diff(original: &str, diff_text: &str) -> Result<String, ToolError> {
    // Parse unified diff hunks (lines starting with @@)
    let lines: Vec<&str> = diff_text.lines().collect();
    let mut hunks: Vec<(String, String)> = Vec::new();
    let mut in_hunk = false;
    let mut old_chunk = String::new();
    let mut new_chunk = String::new();

    for line in lines {
        if line.starts_with("@@") {
            if in_hunk && (!old_chunk.is_empty() || !new_chunk.is_empty()) {
                hunks.push((old_chunk, new_chunk));
                old_chunk = String::new();
                new_chunk = String::new();
            }
            in_hunk = true;
            continue;
        }
        if !in_hunk {
            continue;
        }
        if let Some(rest) = line.strip_prefix('-') {
            old_chunk.push_str(rest);
            old_chunk.push('\n');
        } else if let Some(rest) = line.strip_prefix('+') {
            new_chunk.push_str(rest);
            new_chunk.push('\n');
        } else if let Some(rest) = line.strip_prefix(' ') {
            old_chunk.push_str(rest);
            old_chunk.push('\n');
            new_chunk.push_str(rest);
            new_chunk.push('\n');
        } else if line.is_empty() {
            old_chunk.push('\n');
            new_chunk.push('\n');
        }
    }
    if in_hunk && (!old_chunk.is_empty() || !new_chunk.is_empty()) {
        hunks.push((old_chunk, new_chunk));
    }

    if hunks.is_empty() {
        return Err(ToolError::Other(
            "no valid unified diff hunks found (expected @@ ... @@). Use old_text/new_text instead.".into(),
        ));
    }

    let mut current = original.to_string();
    for (old, new) in hunks {
        current = replace_once(&current, &old, &new)?;
    }
    Ok(current)
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
            ..Default::default()
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
    async fn search_replace_handles_crlf() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("crlf.txt");
        // File has Windows CRLF
        std::fs::write(&p, "line 1\r\nline 2\r\nline 3\r\n").unwrap();
        let ctx = ToolContext {
            workspace: Workspace::new(dir.path(), "t"),
            approved: false,
            ..Default::default()
        };
        // LLM sends LF only
        let out = PatchTool
            .execute(
                serde_json::json!({
                    "path": "crlf.txt",
                    "old_text": "line 2\n",
                    "new_text": "line two\n",
                }),
                &ctx,
            )
            .await
            .unwrap();
        assert!(!out.is_error);
        assert_eq!(
            std::fs::read_to_string(&p).unwrap(),
            "line 1\r\nline two\r\nline 3\r\n"
        );
    }

    #[tokio::test]
    async fn unified_diff_works() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("diff.txt");
        std::fs::write(&p, "fn main() {\n    println!(\"old\");\n}\n").unwrap();
        let ctx = ToolContext {
            workspace: Workspace::new(dir.path(), "t"),
            approved: false,
            ..Default::default()
        };
        let diff_str = "--- a/diff.txt\n+++ b/diff.txt\n@@ -1,3 +1,3 @@\n fn main() {\n-    println!(\"old\");\n+    println!(\"new\");\n }\n";
        let out = PatchTool
            .execute(
                serde_json::json!({
                    "path": "diff.txt",
                    "diff": diff_str,
                }),
                &ctx,
            )
            .await
            .unwrap();
        assert!(!out.is_error);
        assert_eq!(
            std::fs::read_to_string(&p).unwrap(),
            "fn main() {\n    println!(\"new\");\n}\n"
        );
    }

    #[tokio::test]
    async fn refuses_ambiguous_replace() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "x\nx\n").unwrap();
        let ctx = ToolContext {
            workspace: Workspace::new(dir.path(), "t"),
            approved: false,
            ..Default::default()
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
