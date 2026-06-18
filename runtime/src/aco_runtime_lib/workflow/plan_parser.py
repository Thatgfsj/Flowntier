"""Markdown plan → DAG parser. Phase 2.1.

Converts a Chief's plan document (the 8-section Markdown emitted
by `PlannerAgent` per `prompts/planner.md`) into a `ParsedPlan`
AST. Pure Python, no LLM calls, deterministic.

See `docs/PROPOSALS/phase2-1-plan-parser.md` for the RFC and
`docs/TASK_GRAPH.md` §2 for the data model (this is the Python
mirror of the Rust struct in `crates/tauri-core`).

Algorithm: two-pass Markdown walker.
1. Section splitter — state machine over `## ` headers.
2. Per-section parser — strict or lenient per the table below.

| Section             | Parser                    | Strict? |
|---------------------|---------------------------|---------|
| `Goal`              | `parse_prose`             | lenient |
| `Architecture`      | `parse_prose`             | lenient |
| `Task Graph`        | `parse_task_table`        | strict  |
| `APIs / Interfaces` | `parse_apis`              | lenient |
| `Data Model`        | `parse_data_model`        | lenient |
| `Acceptance Criteria` | `parse_acceptance_list` | strict  |
| `Risks`             | `parse_risks`             | lenient |
| `Out of Scope`      | `parse_bullets`           | lenient |
"""
from __future__ import annotations

import re
import warnings
from collections.abc import Iterable
from dataclasses import dataclass, field


# ── Errors & warnings ──────────────────────────────────────────


class PlanParseError(Exception):
    """Strict-mode rejection. The Chief must revise.

    Carries enough context for the repair loop to tell the model
    exactly what to fix.
    """

    def __init__(
        self,
        section: str,
        kind: str,
        message: str,
        line: int | None = None,
    ) -> None:
        self.section = section
        self.kind = kind
        self.line = line
        super().__init__(
            f"[{section}{f' L{line}' if line else ''}] {kind}: {message}"
        )


class PlanParseWarning(UserWarning):
    """Lenient-mode: skipped something, parser kept going."""


# ── AST dataclasses (mirror of TASK_GRAPH.md §2) ───────────────


@dataclass(frozen=True)
class ApiEndpoint:
    method: str
    path: str
    auth: str = ""
    notes: str = ""


@dataclass(frozen=True)
class SchemaChange:
    name: str
    change_type: str  # "add" | "modify" | "remove"
    description: str


@dataclass(frozen=True)
class AcceptanceCriterion:
    id: str
    description: str
    test: str | None = None
    automated: bool = False


@dataclass(frozen=True)
class Risk:
    name: str
    description: str
    mitigation: str | None = None


@dataclass(frozen=True)
class TaskNode:
    id: str
    title: str
    owner_role: str
    depends_on: tuple[str, ...]
    est_tokens: int


@dataclass(frozen=True)
class Edge:
    from_: str
    to: str
    kind: str  # "Hard" | "Soft" (always "Hard" in 2.1; see RFC §10.3)


@dataclass
class ParsedPlan:
    title: str
    goal: str = ""
    architecture: str = ""
    nodes: list[TaskNode] = field(default_factory=list)
    edges: list[Edge] = field(default_factory=list)
    apis: list[ApiEndpoint] = field(default_factory=list)
    data_model: list[SchemaChange] = field(default_factory=list)
    acceptance: list[AcceptanceCriterion] = field(default_factory=list)
    risks: list[Risk] = field(default_factory=list)
    out_of_scope: list[str] = field(default_factory=list)


# ── Top-level entry point ──────────────────────────────────────


def parse_plan(md: str) -> ParsedPlan:
    """Parse a Markdown plan doc into a `ParsedPlan`.

    Raises `PlanParseError` on strict-mode failure. Lenient-mode
    issues emit `PlanParseWarning` and are stored in the AST as
    empty collections.
    """
    sections = _split_sections(md)
    title = _extract_title(md)
    nodes = _parse_task_table(sections.get("task graph", ""))
    edges = _derive_edges(nodes)
    return ParsedPlan(
        title=title,
        goal=_parse_prose(sections.get("goal", "")),
        architecture=_parse_prose(sections.get("architecture", "")),
        nodes=nodes,
        edges=edges,
        apis=_parse_apis(sections.get("apis / interfaces", "")),
        data_model=_parse_data_model(sections.get("data model", "")),
        acceptance=_parse_acceptance_list(
            sections.get("acceptance criteria", "")
        ),
        risks=_parse_risks(sections.get("risks", "")),
        out_of_scope=_parse_bullets(sections.get("out of scope", "")),
    )


# ── Pass 1: section splitter ──────────────────────────────────


_KNOWN_SECTIONS: frozenset[str] = frozenset(
    {
        "goal",
        "architecture",
        "task graph",
        "apis / interfaces",
        "data model",
        "acceptance criteria",
        "risks",
        "out of scope",
    }
)


_HEADER_RE = re.compile(r"^#{1,2}\s+(.+?)\s*$", re.MULTILINE)


def _split_sections(md: str) -> dict[str, str]:
    """Split the doc by `## <name>` headers into {name_lower: body}.

    The first `# Title` line is ignored (handled by `_extract_title`).
    Unknown `##` headers raise `PlanParseError(unknown_section)`.
    """
    sections: dict[str, list[str]] = {}
    current: str | None = None
    line_no = 0
    for line in md.splitlines():
        line_no += 1
        header_match = re.match(r"^##\s+(.+?)\s*$", line)
        if header_match:
            name = header_match.group(1).strip().lower()
            if name not in _KNOWN_SECTIONS:
                raise PlanParseError(
                    section=name,
                    kind="unknown_section",
                    message=(
                        f"unknown section header; known: "
                        f"{sorted(_KNOWN_SECTIONS)}"
                    ),
                    line=line_no,
                )
            current = name
            sections[current] = []
            continue
        if current is not None:
            sections[current].append(line)
    return {name: "\n".join(body).strip() for name, body in sections.items()}


_TITLE_RE = re.compile(r"^#\s+(.+?)\s*$", re.MULTILINE)


def _extract_title(md: str) -> str:
    m = _TITLE_RE.search(md)
    if not m:
        raise PlanParseError(
            section="(top)",
            kind="missing_title",
            message="plan must start with `# Plan: <title>`",
        )
    return m.group(1).strip()


# ── Per-section parsers ────────────────────────────────────────


def _parse_prose(body: str) -> str:
    """Strip and return free text. Lenient — never raises."""
    return body.strip()


# Task Graph (strict) ──────────────────────────────────────────


_TASK_HEADER_RE = re.compile(
    r"^\|\s*ID\s*\|\s*Title\s*\|\s*Owner Role\s*\|\s*"
    r"Depends On\s*\|\s*Est\.?\s*Tokens?\s*\|",
    re.IGNORECASE | re.MULTILINE,
)

_VALID_OWNER_ROLES: frozenset[str] = frozenset(
    {
        "backend",
        "frontend",
        "database",
        "devops",
        "qa",
        "docs",
        "security",
        "other",
    }
)


def _parse_task_table(body: str) -> list[TaskNode]:
    """Parse the `## Task Graph` Markdown table into `TaskNode`s.

    Strict — raises `PlanParseError` on:
    * missing header row
    * wrong column count
    * bad ID format
    * unknown owner role
    * non-numeric est_tokens
    * duplicate IDs
    """
    if not body.strip():
        raise PlanParseError(
            section="Task Graph",
            kind="empty",
            message="Task Graph section is empty; a plan needs at "
            "least one task",
        )
    header_match = _TASK_HEADER_RE.search(body)
    if not header_match:
        raise PlanParseError(
            section="Task Graph",
            kind="missing_header",
            message=(
                "expected header `| ID | Title | Owner Role | "
                "Depends On | Est. Tokens |`"
            ),
        )

    # Strip header + separator rows, keep data rows
    rows: list[list[str]] = []
    for line in body.splitlines():
        if not line.strip().startswith("|"):
            continue
        if re.match(r"^\|[\s\-|]+\|\s*$", line):
            continue  # the `|---|---|...` separator
        if header_match.start() <= body.find(line) <= header_match.end():
            continue
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        if len(cells) != 5:
            raise PlanParseError(
                section="Task Graph",
                kind="bad_column_count",
                message=(
                    f"expected 5 columns (ID | Title | Owner Role | "
                    f"Depends On | Est. Tokens), got {len(cells)}"
                ),
            )
        rows.append(cells)

    if not rows:
        raise PlanParseError(
            section="Task Graph",
            kind="empty",
            message="Task Graph has a header but no data rows",
        )

    nodes: list[TaskNode] = []
    seen_ids: set[str] = set()
    for cells in rows:
        tid, title, role, deps_raw, tokens_raw = cells
        if not re.fullmatch(r"T\d+", tid):
            raise PlanParseError(
                section="Task Graph",
                kind="bad_id",
                message=(
                    f"task ID {tid!r} must match ^T\\d+$ "
                    "(e.g. T1, T12)"
                ),
            )
        if tid in seen_ids:
            raise PlanParseError(
                section="Task Graph",
                kind="duplicate_id",
                message=f"task ID {tid} appears more than once",
            )
        seen_ids.add(tid)
        role_lower = role.lower()
        if role_lower not in _VALID_OWNER_ROLES:
            raise PlanParseError(
                section="Task Graph",
                kind="unknown_owner_role",
                message=(
                    f"owner role {role!r} not in "
                    f"{sorted(_VALID_OWNER_ROLES)}"
                ),
            )
        tokens_clean = tokens_raw.replace(",", "").strip()
        if not tokens_clean.isdigit():
            raise PlanParseError(
                section="Task Graph",
                kind="bad_tokens",
                message=f"est. tokens {tokens_raw!r} must be an integer",
            )
        if deps_raw.strip() in {"—", "-", "none", ""}:
            deps: tuple[str, ...] = ()
        else:
            deps = tuple(
                d.strip()
                for d in deps_raw.split(",")
                if d.strip() and d.strip() not in {"—", "-"}
            )
            for d in deps:
                if not re.fullmatch(r"T\d+", d):
                    raise PlanParseError(
                        section="Task Graph",
                        kind="bad_dep",
                        message=(
                            f"dependency {d!r} must match ^T\\d+$ "
                            "or be — / - / none"
                        ),
                    )
        nodes.append(
            TaskNode(
                id=tid,
                title=title,
                owner_role=role_lower,
                depends_on=deps,
                est_tokens=int(tokens_clean),
            )
        )

    # Validate that every dep points to a known node
    node_ids = {n.id for n in nodes}
    for n in nodes:
        for d in n.depends_on:
            if d not in node_ids:
                raise PlanParseError(
                    section="Task Graph",
                    kind="missing_dependency",
                    message=(
                        f"task {n.id} depends on {d!r}, but no such "
                        f"task exists in this plan"
                    ),
                )
    return nodes


def _derive_edges(nodes: Iterable[TaskNode]) -> list[Edge]:
    """Convert depends_on → Edge list. Always Hard edges in 2.1."""
    return [
        Edge(from_=dep, to=n.id, kind="Hard")
        for n in nodes
        for dep in n.depends_on
    ]


# APIs / Interfaces (lenient) ──────────────────────────────────


_FENCE_RE = re.compile(r"^```", re.MULTILINE)
_API_HEADER_RE = re.compile(
    r"^\|\s*(?:Method|Verb)\s*\|\s*Path\s*\|",
    re.IGNORECASE | re.MULTILINE,
)


def _parse_apis(body: str) -> list[ApiEndpoint]:
    """Find GFM tables whose first column is Method/Verb and second
    is Path. Lenient — non-table bodies and prose endpoints are
    silently dropped, no warning.
    """
    out: list[ApiEndpoint] = []
    if not body.strip() or not _API_HEADER_RE.search(body):
        return out
    # Walk line-by-line; collect rows that follow an API table header
    in_table = False
    for line in body.splitlines():
        if _API_HEADER_RE.match(line):
            in_table = True
            continue
        if in_table and line.startswith("|") and "---" in line:
            continue
        if in_table and line.startswith("|"):
            cells = [c.strip() for c in line.strip().strip("|").split("|")]
            if len(cells) >= 2 and cells[0] and cells[1]:
                out.append(
                    ApiEndpoint(
                        method=cells[0],
                        path=cells[1],
                        auth=cells[2] if len(cells) > 2 else "",
                        notes=cells[3] if len(cells) > 3 else "",
                    )
                )
        elif in_table and not line.startswith("|"):
            in_table = False
    return out


# Data Model (lenient) ─────────────────────────────────────────


_SCHEMA_TABLE_HEADER_RE = re.compile(
    r"^\|\s*(?:Column|Field|Name)\s*\|\s*(?:Type|Kind)\s*\|",
    re.IGNORECASE | re.MULTILINE,
)


def _parse_data_model(body: str) -> list[SchemaChange]:
    """Parse schema-change tables or bullets. Lenient — unknown
    shapes are dropped with a warning."""
    out: list[SchemaChange] = []
    if not body.strip():
        return out
    if _SCHEMA_TABLE_HEADER_RE.search(body):
        in_table = False
        for line in body.splitlines():
            if _SCHEMA_TABLE_HEADER_RE.match(line):
                in_table = True
                continue
            if in_table and line.startswith("|") and "---" in line:
                continue
            if in_table and line.startswith("|"):
                cells = [
                    c.strip() for c in line.strip().strip("|").split("|")
                ]
                if len(cells) >= 2 and cells[0]:
                    out.append(
                        SchemaChange(
                            name=cells[0],
                            change_type="modify",
                            description=" | ".join(cells[1:]),
                        )
                    )
            elif in_table and not line.startswith("|"):
                in_table = False
    else:
        for line in body.splitlines():
            m = re.match(r"^\s*-\s*[`']?(\w+)[`']?\s*:\s*(.+)$", line)
            if m:
                out.append(
                    SchemaChange(
                        name=m.group(1),
                        change_type="add",
                        description=m.group(2).strip(),
                    )
                )
    if not out and body.strip():
        warnings.warn(
            "Data Model section present but no table or bullets matched",
            PlanParseWarning,
            stacklevel=2,
        )
    return out


# Acceptance Criteria (strict) ─────────────────────────────────


_ACCEPTANCE_RE = re.compile(r"^\s*(\d+)\.\s+(.+?)\s*$", re.MULTILINE)


def _parse_acceptance_list(body: str) -> list[AcceptanceCriterion]:
    """Parse the numbered list. Strict — empty list is an error."""
    if not body.strip():
        raise PlanParseError(
            section="Acceptance Criteria",
            kind="empty",
            message="Acceptance Criteria section is empty",
        )
    items = _ACCEPTANCE_RE.findall(body)
    if not items:
        raise PlanParseError(
            section="Acceptance Criteria",
            kind="malformed",
            message=(
                "expected a numbered list (`1. ...`, `2. ...`); "
                "got no items"
            ),
        )
    return [
        AcceptanceCriterion(id=f"ac-{idx}", description=text.strip())
        for idx, (_num, text) in enumerate(items, start=1)
    ]


# Risks (lenient) ──────────────────────────────────────────────


_RISK_RE = re.compile(
    r"^\s*-\s+\*\*(?P<name>[^*]+)\*\*\s*:\s*(?P<desc>.+?)(?:\.\s*"
    r"Mitigated by\s+(?P<mit>.+?))?\s*\.?\s*$",
    re.MULTILINE,
)


def _parse_risks(body: str) -> list[Risk]:
    out: list[Risk] = []
    for line in body.splitlines():
        m = re.match(
            r"^\s*-\s+\*\*(?P<name>[^*]+)\*\*\s*:\s*(?P<rest>.+?)\s*$",
            line,
        )
        if not m:
            continue
        name = m.group("name").strip()
        rest = m.group("rest").strip()
        # Split on " Mitigated by " (case-insensitive)
        m2 = re.split(r"\.\s*Mitigated by\s+", rest, maxsplit=1, flags=re.IGNORECASE)
        if len(m2) == 2:
            desc, mit = m2[0].strip(" ."), m2[1].strip(" .")
        else:
            desc, mit = rest.strip(" ."), None
        out.append(Risk(name=name, description=desc, mitigation=mit))
    if not out and body.strip():
        warnings.warn(
            "Risks section present but no `- **name**: desc` lines matched",
            PlanParseWarning,
            stacklevel=2,
        )
    return out


# Out of Scope (lenient) ───────────────────────────────────────


def _parse_bullets(body: str) -> list[str]:
    out: list[str] = []
    for line in body.splitlines():
        m = re.match(r"^\s*-\s+(.+?)\s*$", line)
        if m:
            out.append(m.group(1).strip())
    if not out and body.strip():
        warnings.warn(
            "Out of Scope section present but no `- item` lines matched",
            PlanParseWarning,
            stacklevel=2,
        )
    return out