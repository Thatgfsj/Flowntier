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
                if !g.is_match(p) { continue; }
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