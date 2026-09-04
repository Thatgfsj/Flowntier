//! `grep` — search for a pattern inside files under the workspace.
//!
//! Implementation note: we walk with `ignore` (gitignore-aware)
//! and match line-by-line in-process. For repos larger than
//! ~50k files, swap in `ripgrep` via `tokio::process` — but the
//! in-process version keeps the dependency surface small and
//! avoids the portable-pty story for v0.3.

use async_trait::async_trait;
use regex::Regex;
use std::path::Path;

use super::{Tool, ToolContext, ToolError, ToolOutput};

/// `grep` tool — search for a regex inside workspace files.
#[derive(Debug)]
pub struct GrepTool;

#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &'static str {
        "grep"
    }

    fn description(&self) -> &'static str {
        "Search for a regex pattern in workspace files (gitignore-aware). \
         Returns matches as `path:line: text`. Caps at 200 matches."
    }

    fn schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string" },
                "include":  { "type": "string", "description": "Glob limiting files (e.g. '*.rs')." },
                "max":     { "type": "integer", "minimum": 1, "maximum": 200, "default": 200 }
            },
            "required": ["pattern"],
            "additionalProperties": false
        })
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        if !ctx.capabilities.read {
            return Ok(ToolOutput::err("refused: read capability disabled (grep)"));
        }
        let pattern = args
            .get("pattern")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("missing 'pattern'".into()))?;
        let include_glob = args.get("include").and_then(|v| v.as_str());
        let max = args
            .get("max")
            .and_then(|v| v.as_u64())
            .unwrap_or(200)
            .min(200) as usize;

        let re = Regex::new(pattern).map_err(|e| ToolError::InvalidArgs(e.to_string()))?;
        let glob_matcher = include_glob
            .map(|g| globset::Glob::new(g).map(|g| g.compile_matcher()))
            .transpose()
            .map_err(|e| ToolError::InvalidArgs(e.to_string()))?;

        let walker = ignore::WalkBuilder::new(&ctx.workspace.root)
            .standard_filters(true)
            .build();

        let mut hits = 0usize;
        let mut out = String::new();
        for entry in walker.flatten() {
            if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                continue;
            }
            let p = entry.path();
            if let Some(g) = &glob_matcher {
                if !g.is_match(p) {
                    continue;
                }
            }
            let rel = ctx.workspace.relativize(p);
            if let Ok(s) = std::fs::read_to_string(p) {
                for (idx, line) in s.lines().enumerate() {
                    if re.is_match(line) {
                        out.push_str(&format!("{}:{}: {}\n", rel.display(), idx + 1, line));
                        hits += 1;
                        if hits >= max {
                            out.push_str(&format!("\n…(truncated at {max} matches)"));
                            return Ok(ToolOutput::ok(out));
                        }
                    }
                }
            }
            let _ = Path::new(""); // silence unused import on some builds
        }

        if hits == 0 {
            Ok(ToolOutput::ok("no matches"))
        } else {
            out.push_str(&format!("\n--- {hits} match(es) ---"));
            Ok(ToolOutput::ok(out))
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::ToolContext;
    use crate::workspace::Workspace;

    fn ctx(dir: &std::path::Path) -> ToolContext {
        ToolContext {
            workspace: Workspace::new(dir, "grep-test"),
            approved: true,
            ..Default::default()
        }
    }

    fn write(dir: &std::path::Path, name: &str, content: &str) {
        std::fs::write(dir.join(name), content).unwrap();
    }

    #[tokio::test]
    async fn finds_simple_pattern() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "a.txt", "hello world\nhello rust\nbye\n");
        write(dir.path(), "b.txt", "nope\n");
        let out = GrepTool
            .execute(serde_json::json!({"pattern": "hello"}), &ctx(dir.path()))
            .await
            .unwrap();
        assert!(!out.is_error);
        assert!(
            out.content.contains("a.txt:1: hello world"),
            "got: {}",
            out.content
        );
        assert!(out.content.contains("a.txt:2: hello rust"));
        assert!(!out.content.contains("b.txt"));
        assert!(out.content.contains("2 match(es)"));
    }

    #[tokio::test]
    async fn respects_include_glob() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "a.rs", "let x = 1;\n");
        write(dir.path(), "b.txt", "let x = 2;\n");
        let out = GrepTool
            .execute(
                serde_json::json!({"pattern": "let x", "include": "*.rs"}),
                &ctx(dir.path()),
            )
            .await
            .unwrap();
        assert!(out.content.contains("a.rs"));
        assert!(!out.content.contains("b.txt"));
    }

    #[tokio::test]
    async fn no_match_returns_no_matches_message() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "a.txt", "hi\n");
        let out = GrepTool
            .execute(
                serde_json::json!({"pattern": "this_does_not_appear"}),
                &ctx(dir.path()),
            )
            .await
            .unwrap();
        assert!(!out.is_error);
        assert!(out.content.contains("no matches"));
    }

    #[tokio::test]
    async fn respects_max_cap() {
        let dir = tempfile::tempdir().unwrap();
        // 5 matching lines, max=2 → truncates
        write(dir.path(), "a.txt", "x\nx\nx\nx\nx\n");
        let out = GrepTool
            .execute(
                serde_json::json!({"pattern": "x", "max": 2}),
                &ctx(dir.path()),
            )
            .await
            .unwrap();
        assert!(out.content.contains("truncated at 2"));
    }

    #[tokio::test]
    async fn refuses_when_capability_off() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "a.txt", "hello\n");
        let mut c = ctx(dir.path());
        c.capabilities.read = false;
        let out = GrepTool
            .execute(serde_json::json!({"pattern": "hello"}), &c)
            .await
            .unwrap();
        assert!(out.is_error);
        assert!(out.content.contains("capability disabled"));
    }

    #[tokio::test]
    async fn invalid_regex_is_clean_error() {
        let dir = tempfile::tempdir().unwrap();
        let err = GrepTool
            .execute(serde_json::json!({"pattern": "["}), &ctx(dir.path()))
            .await
            .expect_err("invalid regex must return Err(InvalidArgs), not Ok");
        assert!(
            matches!(err, crate::tool::ToolError::InvalidArgs(_)),
            "expected InvalidArgs, got {err:?}"
        );
    }
}
