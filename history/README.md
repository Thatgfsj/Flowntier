# `history/` — v0.2 时代的设计文档与 Python harness

This directory preserves the **v0.2.x-era** Flowntier docs and
Python harness files, archived for paper / historical reference.
Files in here describe the **pre-rename** state of the project
(when it was still branded "Agent Company OS" / "ACO" and shipped
a Python FastAPI runtime + Claude Code CLI sidecar).

## Layout

```
history/
├── README.md                        ← this file
├── docs/                            ← 12 design docs from v0.2.x era
│   ├── CONFIG.md
│   ├── DEPENDENCIES.md
│   ├── ISSUES_GRAPH.md
│   ├── PLUGIN_SPEC.md
│   ├── PROMPT_Guide.md
│   ├── PROVIDER_SPEC.md
│   ├── SECURITY.md
│   ├── STORAGE_SPEC.md
│   ├── TASK_GRAPH.md
│   ├── UI_GUIDELINES.md
│   ├── V03_DELETIONS.md              ← "what we deleted when going v0.3"
│   └── WORKFLOW_SPEC.md
├── proposals/                        ← 2 design proposals from v0.2 era
│   ├── phase2-1-plan-parser.md
│   └── phase2-4-react-flow-ui.md
├── python-harness/                   ← 6 Python files + dogfooding data
│   ├── critic_a_smoke.py
│   ├── dogfood.py
│   ├── e2e_smoke.py
│   ├── fix_c3.py
│   ├── gen_flowntier_icon.py        ← was gen_aco_icon.py; renamed
│   ├── patch_settings.py
│   ├── dogfooding/                   ← captured dogfood runs
│   └── outputs/                      ← captured harness outputs
└── release-notes/                   ← 4 release notes, v0.2.2 → v0.3.0
    ├── release_notes_v0.2.2.md       ← was .validation/release_notes.md
    ├── release_notes_v0.2.3.md
    ├── release_notes_v0.2.5.md
    └── release_notes_v0.3.md
```

## Why this exists

When the project renamed from **Agent Company OS → Flowntier** in
v0.3.0, the *active* docs and code moved forward, but a
substantial body of v0.2.x material (Python runtime harness, the
early design RFCs, pre-rename release notes) was kept around for
reference. Stuffing them under `history/` makes the active repo
clean without losing data — important for:

- **Paper writing**: prior-art comparisons and "what we changed"
  evidence for the v0.3 migration writeup.
- **Audit trail**: what the project actually shipped in v0.2.5
  vs v0.3.0.
- **Python harness reproducibility**: the `python-harness/`
  scripts still run (against the v0.2.5 server) if you build a
  Python venv with the right deps, even though they're not used
  in production anymore.

## Brand references inside

Files in this directory **still mention "Agent Company OS" / "ACO"
in their body text**. That's intentional — they describe the
v0.2.x state of the project, where the brand was Agent Company
OS. Rewriting them would falsify the historical record.

If you want the *spirit* of the brand consistent when reading
across docs, mentally substitute "Agent Company OS" → "Flowntier
(pre-rename)" and "ACO" → "the original abbreviation of
Flowntier".

## Where the active v0.3+ docs live

- `README.md` (root) — current project README
- `docs/` (root) — 13 *active* design docs that ship with v0.3.0:
  ACCEPTANCE_v0.3{,_LEDGER}, AGENT_PROTOCOL, ARCHITECTURE, FAQ,
  INSTALLER, ROADMAP, TECH_STACK, UPGRADE_v0.3, ACCEPTANCE_v0.4
- `crates/agent-core/src/prompt/mod.rs` — current 6 role system
  prompts (主理 / 计划 / 实施 / 找茬 / 审查 / 汇报)
- `.validation/release_notes_v0.3.md` lives in `history/` because
  it's the release note for the version that did the rename.