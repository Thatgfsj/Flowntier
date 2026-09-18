//! Project rule-file discovery — v0.4.36.
//!
//! Modern coding agents converge on project-level rule files that
//! steer behavior without the user repeating themselves: `AGENTS.md`
//! is the de-facto standard (also read natively by VS Code's agent
//! host), `CLAUDE.md` is Claude Code's convention, and Flowntier's
//! own `.agent/` directory already carries rule markdown
//! (`architecture_rules.md` et al. — see this repo's `.agent/`).
//!
//! [`load_project_rules`] collects those files for a workspace
//! root, concatenates them (newest contribution first: root files
//! before `.agent/` details), byte-caps the result at
//! [`MAX_RULES_BYTES`] with a **char-boundary-safe** truncation
//! (a naive `&s[..cap]` panics when a CJK rule file is cut inside a
//! multi-byte sequence), and returns `None` when nothing exists so
//! the system prompt stays byte-identical for rule-less projects.
//!
//! There is deliberately no cache: rules are re-read per agent run
//! (the files are tiny) so an edit to `AGENTS.md` is visible on the
//! next task without a restart.

use std::path::{Path, PathBuf};

/// Cap on the concatenated rules blob. Generous enough for real
/// rule files, small enough that it can't blow the system prompt
/// budget.
pub const MAX_RULES_BYTES: usize = 24 * 1024;

/// Collect and concatenate project rule files for `root`.
///
/// Sources, deduplicated and sorted within each tier:
/// 1. `AGENTS.md`, `CLAUDE.md` (workspace root)
/// 2. `.agent/rules/*.md` (nested convention)
/// 3. `.agent/*.md` (Flowntier's own flat layout)
///
/// Returns `None` when no rule file exists. Individual unreadable
/// files are skipped (a broken file must not take down agent runs).
pub fn load_project_rules(root: &Path) -> Option<String> {
    let files = collect_rule_files(root);
    if files.is_empty() {
        return None;
    }
    let mut out = String::new();
    for path in &files {
        let Ok(content) = std::fs::read_to_string(path) else {
            continue;
        };
        if content.trim().is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str(&format!("# Rule file: {}\n\n", display_rel(path, root)));
        out.push_str(content.trim_end());
    }
    if out.is_empty() {
        return None;
    }
    if out.len() > MAX_RULES_BYTES {
        out = truncate_at_char_boundary(&out, MAX_RULES_BYTES);
        out.push_str("\n\n[flowntier: project rules truncated at 24 KiB]");
    }
    Some(out)
}

/// Char-boundary-safe truncation. `&s[..max]` panics when `max`
/// lands inside a multi-byte sequence (CJK rule files at the cap
/// boundary); walk back to the nearest boundary instead.
fn truncate_at_char_boundary(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut i = max;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    s[..i].to_string()
}

fn collect_rule_files(root: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = Vec::new();
    // Tier 1: root rule files, fixed order.
    for name in ["AGENTS.md", "CLAUDE.md"] {
        let p = root.join(name);
        if p.is_file() {
            files.push(p);
        }
    }
    // Tiers 2+3: sorted *.md from both .agent layouts, deduplicated
    // (a file can only be in one of the two dirs, but sort both so
    // output is deterministic).
    let mut agent_files: Vec<PathBuf> = Vec::new();
    for dir in [".agent/rules", ".agent"] {
        if let Ok(entries) = std::fs::read_dir(root.join(dir)) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_file() && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("md")) {
                    agent_files.push(p);
                }
            }
        }
    }
    agent_files.sort();
    agent_files.dedup();
    files.extend(agent_files);
    files
}

fn display_rel(path: &Path, root: &Path) -> String {
    // Normalize to forward slashes for display: the joined literal
    // ".agent/rules" keeps its `/` while read_dir appends `\` on
    // Windows, and prompts shouldn't carry mixed separators.
    path.strip_prefix(root)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("flowntier-rules-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn none_when_no_rule_files() {
        let root = temp_root("none");
        assert!(load_project_rules(&root).is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn agents_md_is_read_first() {
        let root = temp_root("agents");
        std::fs::write(root.join("AGENTS.md"), "be concise").unwrap();
        std::fs::create_dir_all(root.join(".agent/rules")).unwrap();
        std::fs::write(root.join(".agent/rules/style.md"), "no todos").unwrap();

        let rules = load_project_rules(&root).expect("rules");
        let agents_pos = rules.find("be concise").unwrap();
        let style_pos = rules.find("no todos").unwrap();
        assert!(agents_pos < style_pos, "root rules come before .agent");
        assert!(rules.contains("Rule file: AGENTS.md"));
        assert!(rules.contains("Rule file: .agent/rules/style.md"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn flat_agent_layout_also_matches() {
        // Flowntier's own .agent/ uses flat *.md, not .agent/rules/.
        let root = temp_root("flat");
        std::fs::create_dir_all(root.join(".agent")).unwrap();
        std::fs::write(root.join(".agent/architecture_rules.md"), "layered").unwrap();
        let rules = load_project_rules(&root).expect("rules");
        assert!(rules.contains("layered"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn truncation_is_char_boundary_safe() {
        // 8_000 CJK chars = 24_000 bytes of 3-byte chars; a cap at
        // 24_576 would land mid-char if sliced naively.
        let s: String = "汉".repeat(9_000);
        let cut = truncate_at_char_boundary(&s, MAX_RULES_BYTES);
        assert!(cut.len() <= MAX_RULES_BYTES);
        assert!(cut.is_char_boundary(cut.len()));
        // Same function must be a no-op under the cap.
        assert_eq!(truncate_at_char_boundary("short", 100), "short");
    }

    #[test]
    fn cap_note_appended_when_truncated() {
        let root = temp_root("cap");
        let big = "x".repeat(MAX_RULES_BYTES + 100);
        std::fs::write(root.join("AGENTS.md"), &big).unwrap();
        let rules = load_project_rules(&root).expect("rules");
        assert!(rules.contains("truncated at 24 KiB"));
        assert!(rules.len() < big.len() + 200);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn unreadable_files_are_skipped_not_fatal() {
        let root = temp_root("skip");
        std::fs::write(root.join("AGENTS.md"), "ok").unwrap();
        // A directory named like a rule file inside .agent — is_file()
        // filters it out, but keep the test honest about empty files.
        std::fs::create_dir_all(root.join(".agent")).unwrap();
        std::fs::write(root.join(".agent/empty.md"), "   ").unwrap();
        let rules = load_project_rules(&root).expect("rules");
        assert!(rules.contains("ok"));
        assert!(!rules.contains("empty"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
