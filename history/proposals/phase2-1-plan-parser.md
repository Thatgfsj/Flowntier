# RFC: Plan Parser (Phase 2.1)

> Markdown → DAG: parse a Chief's plan document into the `Plan` AST.

**Status:** Proposed
**Author:** Thatgfsj
**Date:** 2026-06-18
**Phase:** 2.1 (4 days)
**Related:** [TASK_GRAPH.md](../TASK_GRAPH.md) (data model) ·
[WORKFLOW_SPEC.md](../WORKFLOW_SPEC.md) (workflow lifecycle) ·
[PROMPT_GUIDE.md](../PROMPT_GUIDE.md) (planner prompt)

---

## 1. Problem

The Chief's Planner agent already emits Markdown plans in the
8-section structure (validated by `validate_minimax.py` against
the real MiniMax M3 API on 2026-06-18 — 8/8 sections, 12 tasks,
10,616 chars). But `runtime/src/aco_runtime_lib/agents/planner.py`
only extracts the task table via a single regex
(`_extract_task_table`). Everything else — Goal text,
Architecture, APIs, Data Model, Acceptance Criteria, Risks,
Out of Scope — is discarded.

Phase 2 needs the **full** plan AST in the runtime to:

* render the graph in React Flow (Phase 2.4)
* run acceptance criteria as automated checks (Phase 2.2)
* display structured Risks / Out-of-Scope in the final delivery

---

## 2. Goals & Non-goals

**Goals**

1. Convert a Markdown plan doc into the `Plan` struct from
   [TASK_GRAPH §2](../TASK_GRAPH.md) — sections, edges, deliverables,
   acceptance, risks, out-of-scope.
2. **Strict by default** — unknown sections or malformed tables
   are a `PlanParseError`, not silent `None`. The Chief learns from
   the error and re-emits.
3. **Lenient fallback** for prose-heavy sections (Goal,
   Architecture): missing or malformed → empty string + warning,
   never a hard failure.
4. Pure-Python, no LLM calls, no IO. Deterministic: same input →
   same `Plan` AST.
5. Test matrix: every plan template in
   [TASK_GRAPH §9](../TASK_GRAPH.md) (`feature-crud.md`,
   `bugfix.md`, `refactor.md`, `greenfield-app.md`) is a fixture,
   plus the 5 real plans captured from `validate_minimax.py`
   runs land in `tests/fixtures/plans/`.

**Non-goals** (out of scope for 2.1)

* Validation (cycles, budget) — Phase 2.2.
* React Flow UI — Phase 2.4.
* Plan diffing — v0.3.
* Conditional edges — v0.3.

---

## 3. Input Contract

The Planner's system prompt ([`planner.py`](../..//runtime/src/aco_runtime_lib/agents/planner.py)
`PLANNER_SYSTEM_PROMPT`) already locks the structure:

```
# Plan: <one-line title>
## Goal
<free text>
## Architecture
<free text>
## Task Graph
| ID | Title | Owner Role | Depends On | Est. Tokens |
|----|-------|------------|------------|-------------|
| T1 | ...   | Backend    | —          | 1200        |
## APIs / Interfaces
<free text, code blocks, tables>
## Data Model
<free text, tables>
## Acceptance Criteria
1. <criterion text>
## Risks
- **<name>**: <description>. Mitigated by <mitigation>.
## Out of Scope
- <item>
```

Section order is fixed. Section headers are matched
case-insensitively after stripping trailing whitespace.

---

## 4. AST Output

Maps directly to the `Plan` struct in TASK_GRAPH §2:

```python
@dataclass
class ParsedPlan:
    title: str                          # "# Plan: X"
    goal: str                           # ## Goal body
    architecture: str                   # ## Architecture body
    nodes: list[TaskNode]               # ## Task Graph rows
    edges: list[Edge]                   # derived from depends_on
    apis: list[ApiEndpoint]             # ## APIs / Interfaces tables + code
    data_model: list[SchemaChange]      # ## Data Model tables
    acceptance: list[AcceptanceCriterion]
    risks: list[Risk]
    out_of_scope: list[str]
```

The full type defs live in TASK_GRAPH §2. This RFC does **not**
change them.

---

## 5. Parsing Algorithm

The parser is a **two-pass** Markdown walker:

### Pass 1: Section splitter

Walk the doc line-by-line. State machine over `#`/`##` headers.
Accumulate body lines into the current section. Stop when the
next `##` of the same level is seen, or EOF.

```python
def split_sections(md: str) -> dict[str, str]:
    """Returns {section_name_lower: body_text}."""
```

Unknown `##` headers → `PlanParseError(sec="<name>", line=N,
kind="unknown_section")`. The plan is rejected; the Chief sees
the error and re-emits.

### Pass 2: Per-section parser

Each section has a focused parser:

| Section             | Parser                    | Strict? |
|---------------------|---------------------------|---------|
| `Goal`              | `parse_prose` (strip)     | lenient |
| `Architecture`      | `parse_prose` (strip)     | lenient |
| `Task Graph`        | `parse_task_table`        | strict  |
| `APIs / Interfaces` | `parse_apis`              | lenient |
| `Data Model`        | `parse_data_model`        | lenient |
| `Acceptance Criteria` | `parse_acceptance_list` | strict  |
| `Risks`             | `parse_risks`             | lenient |
| `Out of Scope`      | `parse_bullets`           | lenient |

**Strict** parsers raise `PlanParseError` on the first malformed
input (missing required columns, non-numeric tokens, unknown
status, etc.). **Lenient** parsers log a `PlanParseWarning` and
return an empty list — never block parse.

### 5.1 `parse_task_table`

Input: a GFM Markdown table with the 5 columns from §3.

```python
_TASK_TABLE_HEADER_RE = re.compile(
    r"^\|\s*ID\s*\|\s*Title\s*\|\s*Owner Role\s*\|\s*Depends On\s*\|\s*Est\. Tokens\s*\|",
    re.IGNORECASE | re.MULTILINE,
)
```

Parse strategy:

1. Find the header row. If absent → `PlanParseError(sec="Task Graph",
   kind="missing_header")`.
2. Split body into rows; skip the `|---|---|...|` separator.
3. For each row, regex-split by `|` (trimming), validate column
   count = 5.
4. `ID` must match `^T\d+$` (or be a ULID for sub-tasks —
   see Open Question §10.1).
5. `Owner Role` must be one of `Backend | Frontend | Database |
   DevOps | QA | Docs | Security | Other` (case-insensitive).
6. `Depends On` is a comma-separated list of `T<n>` refs or `—`/`none`/`-`
   for empty.
7. `Est. Tokens` must be `\d+` (commas allowed, stripped before
   parse).

Output: `list[TaskNode]`. Edges are derived:

```python
for node in nodes:
    for dep in node.depends_on:
        edges.append(Edge(from_=dep, to_=node.id, kind=EdgeKind.HARD))
```

### 5.2 `parse_apis`

The `## APIs / Interfaces` section in real outputs mixes prose,
tables, and fenced code blocks. Strategy:

1. Split body on fenced code fences (` ``` `). Each fenced block is
   a candidate schema/response example.
2. Inside non-fenced text, find GFM tables. Each table whose first
   column is `Method` (or `Verb`) and second is `Path` is treated
   as an endpoint table. Each row → `ApiEndpoint(method, path,
   auth, body, response)`.
3. Prose and unused tables → dropped, no warning (lenient).

### 5.3 `parse_data_model`

Two shapes in the wild:

* Bullet list of column changes (`- \`col\`: \`TYPE NULL\` — note`)
* GFM table of schema changes (`| Column | Type | Notes |`)

Strategy: try the table first (regex match on header), fall back
to bullet parsing. Unknown shapes → warning.

### 5.4 `parse_acceptance_list`

Acceptance Criteria must be a numbered list (`1.`, `2.`, ...). Each
item is one criterion. Empty list → error (a plan with no
acceptance is unusable).

```python
_ACCEPTANCE_RE = re.compile(r"^\s*\d+\.\s+(.+?)\s*$", re.MULTILINE)
```

Output: `list[AcceptanceCriterion(id=f"ac-{i+1}", description=match,
test=None, automated=False)]`. The `test` field is filled in by a
separate pass (Phase 2.2 validator; out of scope for 2.1).

### 5.5 `parse_risks`

Input lines match `- **<name>**: <body>. Mitigated by <mit>.` The
parser splits on the first colon, then on the first `Mitigated by`
(case-insensitive) to separate description and mitigation. If the
colon / mitigation are missing → store as `description` only,
`mitigation=None`, and emit a warning.

### 5.6 `parse_bullets` (Out of Scope)

Just `- <item>` lines. Empty → warning, not error.

---

## 6. Error Model

```python
class PlanParseError(Exception):
    """Strict-mode rejection. The Chief must revise."""

class PlanParseWarning(UserWarning):
    """Lenient-mode: skipped something, parser kept going."""
```

`PlanParseError` carries:

```python
@dataclass
class PlanParseError(Exception):
    section: str          # "Task Graph", "Acceptance Criteria", ...
    line: int | None      # 1-based; None if unknown
    kind: str             # "missing_header" | "bad_column_count" |
                          # "unknown_owner_role" | "bad_tokens" |
                          # "unknown_section" | "empty_acceptance" |
                          # "duplicate_id" | "missing_dependency"
    message: str
```

The runtime catches `PlanParseError` and:

1. Logs it at WARN with the full error.
2. Sends the error back to the Planner agent in the repair loop
   (Chief → "Task Graph parse failed: bad_column_count on line 47.
   Expected 5 columns, got 4. Revise.").
3. Bumps `plan_revision_count`. If > 3 → workflow → `FAILED`.

---

## 7. Module Layout

```
runtime/src/aco_runtime_lib/workflow/plan_parser.py
├── PlanParseError, PlanParseWarning
├── parse_plan(md: str) -> ParsedPlan
├── _split_sections(md: str) -> dict[str, str]
├── _parse_prose(body: str) -> str
├── _parse_task_table(body: str) -> list[TaskNode]
├── _parse_apis(body: str) -> list[ApiEndpoint]
├── _parse_data_model(body: str) -> list[SchemaChange]
├── _parse_acceptance_list(body: str) -> list[AcceptanceCriterion]
├── _parse_risks(body: str) -> list[Risk]
└── _parse_bullets(body: str) -> list[str]
```

Single file, ~400 LOC. No external deps beyond stdlib + the
existing TASK_GRAPH dataclasses.

---

## 8. Test Matrix

`runtime/tests/test_plan_parser.py`:

| Fixture                                 | Sections   | Expectation               |
|-----------------------------------------|------------|---------------------------|
| `feature-crud.md`                       | all 8      | 4 nodes, edges right      |
| `bugfix.md`                             | all 8      | 3 nodes, 1 ac             |
| `refactor.md`                           | all 8      | 5 nodes, 0 ac (warn)      |
| `greenfield-app.md`                     | all 8      | 8 nodes, fan-out          |
| `minimax-avatar-2026-06-18.md`          | all 8      | 12 nodes, 15 ac, 9 risks  |
| `bad-unknown-section.md`               | 9th section| `PlanParseError unknown_section` |
| `bad-task-missing-header.md`            | Goal+others| `PlanParseError missing_header` |
| `bad-task-bad-column-count.md`          | all 8      | `PlanParseError bad_column_count` |
| `bad-task-unknown-role.md`              | all 8      | `PlanParseError unknown_owner_role` |
| `bad-tokens.md`                         | all 8      | `PlanParseError bad_tokens` |
| `bad-duplicate-id.md`                   | all 8      | `PlanParseError duplicate_id` |
| `bad-missing-dep.md`                    | all 8      | `PlanParseError missing_dependency` |
| `empty-acceptance.md`                   | all 8      | `PlanParseError empty_acceptance` |
| `lenient-no-risks.md`                   | all 8      | ok, warning, risks=[]     |
| `lenient-prose-only-goal.md`            | Goal=prose | ok, others default        |
| `determinism-1.md` / `determinism-2.md` | identical | byte-identical AST       |

Target: ≥ 30 unit tests, ≥ 95% line coverage on
`plan_parser.py`. Snapshot tests for the 5 real plan fixtures
(Python `assertParsedPlanEqual`).

---

## 9. Acceptance Criteria

1. `parse_plan(feature_crud_fixture)` returns 4 `TaskNode`s with
   the correct edges and ≥ 4 acceptance criteria.
2. Each strict-mode error fixture raises the named
   `PlanParseError` with the expected `section`, `line`, `kind`.
3. Each lenient fixture parses to a `ParsedPlan` and emits the
   expected `PlanParseWarning` (asserted via `pytest.warns`).
4. Two byte-identical plan inputs → two ASTs equal by
   `dataclasses.replace` diff.
5. The real `minimax-avatar-2026-06-18.md` fixture (10,616 chars,
   8/8 sections) parses to exactly 12 tasks, 15 acceptance
   criteria, 9 risks.
6. `mypy --strict` passes on `plan_parser.py`.
7. `ruff check` passes.

---

## 10. Open Questions

1. **ULID vs `T<n>` IDs.** The current Planner outputs `T1, T2, ...`.
   TASK_GRAPH §2 says ULIDs. Should the parser canonicalize
   `T1` → ULID during parse, or accept both? *Proposed: accept
   both in v0.2, normalize to ULID at lock time (Phase 2.3
   scheduler).*
2. **API endpoint extraction fuzziness.** Real plans mix prose and
   tables; an "endpoint" might be described in a sentence. How
   aggressive should `_parse_apis` be? *Proposed: lenient — only
   parse tables, drop prose endpoints, no warning.*
3. **Soft edges.** TASK_GRAPH §2 defines `Soft` edges. The current
   Planner doesn't emit them. *Proposed: never parse soft edges
   in 2.1; defer to 2.3 scheduler (or 3.x when Soft is
   user-expressible).*

---

## 11. Migration / Rollout

1. Add `plan_parser.py` + tests + fixtures.
2. Wire `PlannerAgent.run` to call `parse_plan(plan_md)` and
   store the full `ParsedPlan` in the workflow state alongside
   the existing `plan_md` + `tasks`.
3. The existing `_extract_task_table` keeps working for backwards
   compat; mark deprecated in v0.2, removed in v0.3.
4. Phase 2.2 (validator) consumes `ParsedPlan` directly.
5. Phase 2.4 (React Flow) consumes `ParsedPlan.nodes` +
   `ParsedPlan.edges`.

---

## 12. Effort & Dependencies

* Estimate: 4 days (per Phase 2 plan).
* Blocked by: nothing (Phase 1 closed).
* Blocks: 2.2 (validator), 2.4 (UI graph).
* Risk: LLM planner output drift. Mitigation: snapshot tests on
  every captured plan; tighten the Planner prompt if drift is
  detected (out of scope for 2.1, but tracked in
  [PROMPT_GUIDE.md](../PROMPT_GUIDE.md)).

---

**RFC ends.**