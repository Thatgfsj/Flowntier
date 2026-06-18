"""Tests for `workflow.plan_parser`. Phase 2.1.

The 16 fixtures from `docs/PROPOSALS/phase2-1-plan-parser.md` §8
are split into three groups:

* **happy** — full 8-section plans; must parse without error
* **strict** — strict-mode rejections; each must raise the
  named `PlanParseError` with the right section/kind
* **lenient** — lenient sections drop to empty + warning

Run::

    uv run pytest tests/test_plan_parser.py -v
"""
from __future__ import annotations

import warnings
from pathlib import Path

import pytest

from aco_runtime_lib.workflow import (
    ParsedPlan,
    PlanParseError,
    PlanParseWarning,
    parse_plan,
)


# ── Happy-path fixtures ───────────────────────────────────────


def test_feature_crud_full_plan() -> None:
    md = """\
# Plan: Add User CRUD Endpoints
## Goal
Provide RESTful CRUD for users.

## Architecture
- API layer (FastAPI)
- Service layer
- Postgres repository

## Task Graph
| ID | Title | Owner Role | Depends On | Est. Tokens |
|----|-------|------------|------------|-------------|
| T1 | Schema migration | Database | — | 800 |
| T2 | Repository | Backend | T1 | 1500 |
| T3 | Service + validation | Backend | T2 | 1200 |
| T4 | API routes | Backend | T3 | 1500 |
| T5 | E2E tests | QA | T4 | 2000 |

## APIs / Interfaces
| Method | Path | Auth | Notes |
|--------|------|------|-------|
| POST | /users | JWT | Create |
| GET | /users/{id} | JWT | Read |

## Data Model
| Column | Type | Notes |
|--------|------|-------|
| email | TEXT NOT NULL | Unique |
| created_at | TIMESTAMPTZ | server-set |

## Acceptance Criteria
1. POST /users returns 201 + user object
2. GET /users/{id} returns 200 for existing user
3. POST /users with duplicate email returns 409

## Risks
- **Race on unique email**: two concurrent inserts. Mitigated by DB unique constraint.
- **PII leakage**: email in logs. Mitigated by log redaction.

## Out of Scope
- Bulk import
- Email verification flow
"""
    p = parse_plan(md)
    assert p.title == "Plan: Add User CRUD Endpoints"
    assert len(p.nodes) == 5
    assert {n.id for n in p.nodes} == {"T1", "T2", "T3", "T4", "T5"}
    assert {(e.from_, e.to) for e in p.edges} == {
        ("T1", "T2"), ("T2", "T3"), ("T3", "T4"), ("T4", "T5"),
    }
    assert len(p.acceptance) == 3
    assert len(p.risks) == 2
    assert all(r.mitigation for r in p.risks)
    assert len(p.apis) == 2
    assert p.apis[0].method == "POST"
    assert len(p.data_model) == 2


def test_bugfix_minimal_plan() -> None:
    md = """\
# Plan: Fix NPE in login
## Goal
Stop the NullPointerException on login when user.tenant is None.

## Architecture
Single-file change in auth/login.py + unit test.

## Task Graph
| ID | Title | Owner Role | Depends On | Est. Tokens |
|----|-------|------------|------------|-------------|
| T1 | Add regression test | QA | — | 600 |
| T2 | Fix null check | Backend | T1 | 400 |

## Acceptance Criteria
1. Test fails before fix, passes after.

## Risks
- **Hidden callers**: other code may rely on the old crash.

## Out of Scope
- Rewriting the auth flow.
"""
    p = parse_plan(md)
    assert len(p.nodes) == 2
    assert len(p.acceptance) == 1
    assert len(p.risks) == 1


def test_refactor_no_acceptance_warns() -> None:
    """`refactor.md` template has no Acceptance Criteria. Should
    parse but warn under lenient rules. The strict rule says
    empty acceptance is an error — so this is actually expected to
    raise. We test both: with section present-but-empty → strict
    error; with section omitted entirely → error too."""
    md = """\
# Plan: Refactor payment module
## Goal
Clean up the payment module.

## Architecture
Split into payment/charge.py and payment/refund.py.

## Task Graph
| ID | Title | Owner Role | Depends On | Est. Tokens |
|----|-------|------------|------------|-------------|
| T1 | Split charge logic | Backend | — | 1200 |
| T2 | Split refund logic | Backend | T1 | 800 |
| T3 | Update imports | Backend | T1,T2 | 400 |

## Risks
- **Import cycles**: new files may cycle. Mitigated by dependency review.

## Out of Scope
- Behavioral changes.
"""
    # No Acceptance Criteria section at all → strict error
    with pytest.raises(PlanParseError) as exc:
        parse_plan(md)
    assert exc.value.section == "Acceptance Criteria"
    assert exc.value.kind == "empty"


def test_greenfield_fanout() -> None:
    md = """\
# Plan: Greenfield SaaS scaffold
## Goal
Bootstrap a SaaS app: auth, billing, settings.

## Architecture
Monorepo; NestJS API; Next.js front; Postgres.

## Task Graph
| ID | Title | Owner Role | Depends On | Est. Tokens |
|----|-------|------------|------------|-------------|
| T1 | Repo scaffold | DevOps | — | 500 |
| T2 | Auth service | Backend | T1 | 2000 |
| T3 | Billing service | Backend | T1 | 2500 |
| T4 | Settings service | Backend | T1 | 1500 |
| T5 | Front shell | Frontend | T1 | 2000 |
| T6 | CI | DevOps | T1 | 800 |
| T7 | Deploy | DevOps | T2,T3,T4,T5,T6 | 600 |

## Acceptance Criteria
1. Auth, billing, settings, front, CI, deploy all green.

## Risks
- **Scope creep**: greenfield invites extras. Mitigated by frozen v0 spec.

## Out of Scope
- Custom themes.
"""
    p = parse_plan(md)
    assert len(p.nodes) == 7
    fanout = next(
        n for n in p.nodes if n.id == "T7"
    )
    assert set(fanout.depends_on) == {"T2", "T3", "T4", "T5", "T6"}


def test_minimax_avatar_real_fixture() -> None:
    """The real plan captured from validate_minimax.py on
    2026-06-18: 12 tasks, 15 ac, 9 risks, ~10,616 chars. We use a
    representative trimmed version here; the full file is in
    `.validation/outputs/task_1_planning.json` (preview 500 chars).
    """
    md = """\
# Plan: User Avatar Upload Endpoint
## Goal
Add a POST /users/<id>/avatar endpoint with validation, resize, S3.

## Architecture
Web/API → validation → image-processing → S3 → DB update.

## Task Graph
| ID | Title | Owner Role | Depends On | Est. Tokens |
|----|-------|------------|------------|-------------|
| T1 | Design API contract | Backend | — | 800 |
| T2 | Endpoint skeleton | Backend | T1 | 1000 |
| T3 | MIME validation | Backend | T2 | 600 |
| T4 | Image resize (256x256) | Backend | T2 | 1200 |
| T5 | S3 upload | Backend | T4 | 1500 |
| T6 | Update user record | Backend | T5 | 500 |
| T7 | Integration tests | QA | T3,T5,T6 | 1800 |
| T8 | E2E tests | QA | T7 | 1500 |
| T9 | Docs | Docs | T7 | 600 |
| T10 | Load test (50 RPS) | QA | T8 | 1200 |
| T11 | Security review | Security | T7 | 1500 |
| T12 | Deploy | DevOps | T10,T11 | 500 |

## Acceptance Criteria
1. PNG upload returns 200 + 256x256 URL
2. JPEG upload returns 200 + 256x256 URL
3. Non-image body returns 400
4. image/gif returns 415
5. 6MB body returns 413
6. Missing auth returns 401
7. Wrong-user auth returns 403
8. Nonexistent user returns 404
9. EXIF stripped from stored object
10. Cache-Control immutable
11. Re-upload bumps avatar_version
12. CloudTrail-verified IAM role
13. p95 < 400 ms at 50 RPS
14. S3 outage → 503 with stable error code
15. ≥85% unit-test coverage

## Risks
- **Pixel-bomb DoS**: huge decompressed image. Mitigated by pixel-count cap.
- **MIME spoofing**: trusting Content-Type. Mitigated by magic-byte sniff.
- **S3 cost growth**: every upload adds object. Mitigated by lifecycle rule.
- **Auth bypass via path**: /users/<id> not equal to caller. Mitigated by middleware.
- **EXIF leakage**: GPS in metadata. Mitigated by re-encode.
- **Image lib CVEs**: Pillow/libvips CVEs. Mitigated by pinning + isolation.
- **CDN cache poisoning**: wrong Cache-Control. Mitigated by immutable.
- **Partial-write inconsistency**: S3 ok + DB fail. Mitigated by S3-first ordering.
- **Cold-start latency**: serverless cold. Mitigated by provisioned concurrency.

## Out of Scope
- Old-version sweeper
- GET /users/<id>/avatar (CDN-direct)
- Image filters
- WebP/AVIF/HEIC
- Multi-part uploads
- Account-deletion cascade
"""
    p = parse_plan(md)
    assert len(p.nodes) == 12
    assert len(p.acceptance) == 15
    assert len(p.risks) == 9
    assert all(r.mitigation for r in p.risks)


def test_determinism() -> None:
    """Two byte-identical inputs → byte-identical AST."""
    md = """\
# Plan: A
## Goal
g
## Architecture
a
## Task Graph
| ID | Title | Owner Role | Depends On | Est. Tokens |
|----|-------|------------|------------|-------------|
| T1 | x | Backend | — | 100 |
## Acceptance Criteria
1. ok
## Risks
- **r**: d. Mitigated by m.
## Out of Scope
- x
"""
    p1 = parse_plan(md)
    p2 = parse_plan(md)
    assert p1 == p2  # dataclasses __eq__


# ── Strict-mode error fixtures ────────────────────────────────


def test_unknown_section_raises() -> None:
    md = """\
# Plan: X
## Goal
g
## Mystery Section
body
## Acceptance Criteria
1. ok
"""
    with pytest.raises(PlanParseError) as exc:
        parse_plan(md)
    assert exc.value.section == "mystery section"
    assert exc.value.kind == "unknown_section"


def test_missing_task_graph_header_raises() -> None:
    md = """\
# Plan: X
## Goal
g
## Task Graph
This is not a table, just prose.
## Acceptance Criteria
1. ok
"""
    with pytest.raises(PlanParseError) as exc:
        parse_plan(md)
    assert exc.value.section == "Task Graph"
    assert exc.value.kind == "missing_header"


def test_bad_column_count_raises() -> None:
    md = """\
# Plan: X
## Goal
g
## Task Graph
| ID | Title | Owner Role | Depends On | Est. Tokens |
|----|-------|------------|------------|-------------|
| T1 | only three cells | Backend |
## Acceptance Criteria
1. ok
"""
    with pytest.raises(PlanParseError) as exc:
        parse_plan(md)
    assert exc.value.section == "Task Graph"
    assert exc.value.kind == "bad_column_count"


def test_unknown_owner_role_raises() -> None:
    md = """\
# Plan: X
## Goal
g
## Task Graph
| ID | Title | Owner Role | Depends On | Est. Tokens |
|----|-------|------------|------------|-------------|
| T1 | t | Wizard | — | 100 |
## Acceptance Criteria
1. ok
"""
    with pytest.raises(PlanParseError) as exc:
        parse_plan(md)
    assert exc.value.section == "Task Graph"
    assert exc.value.kind == "unknown_owner_role"


def test_bad_tokens_raises() -> None:
    md = """\
# Plan: X
## Goal
g
## Task Graph
| ID | Title | Owner Role | Depends On | Est. Tokens |
|----|-------|------------|------------|-------------|
| T1 | t | Backend | — | ~1000 |
## Acceptance Criteria
1. ok
"""
    with pytest.raises(PlanParseError) as exc:
        parse_plan(md)
    assert exc.value.section == "Task Graph"
    assert exc.value.kind == "bad_tokens"


def test_duplicate_id_raises() -> None:
    md = """\
# Plan: X
## Goal
g
## Task Graph
| ID | Title | Owner Role | Depends On | Est. Tokens |
|----|-------|------------|------------|-------------|
| T1 | a | Backend | — | 100 |
| T1 | b | Backend | — | 100 |
## Acceptance Criteria
1. ok
"""
    with pytest.raises(PlanParseError) as exc:
        parse_plan(md)
    assert exc.value.section == "Task Graph"
    assert exc.value.kind == "duplicate_id"


def test_missing_dependency_raises() -> None:
    md = """\
# Plan: X
## Goal
g
## Task Graph
| ID | Title | Owner Role | Depends On | Est. Tokens |
|----|-------|------------|------------|-------------|
| T1 | a | Backend | T99 | 100 |
## Acceptance Criteria
1. ok
"""
    with pytest.raises(PlanParseError) as exc:
        parse_plan(md)
    assert exc.value.section == "Task Graph"
    assert exc.value.kind == "missing_dependency"


def test_empty_acceptance_raises() -> None:
    md = """\
# Plan: X
## Goal
g
## Task Graph
| ID | Title | Owner Role | Depends On | Est. Tokens |
|----|-------|------------|------------|-------------|
| T1 | a | Backend | — | 100 |
## Acceptance Criteria
"""
    with pytest.raises(PlanParseError) as exc:
        parse_plan(md)
    assert exc.value.section == "Acceptance Criteria"
    assert exc.value.kind == "empty"


# ── Lenient-mode fixtures (warn + empty) ──────────────────────


def test_risks_section_present_but_no_bullets_warns() -> None:
    """A Risks section is present but has no `- **name**: desc` lines.
    Parser warns (PlanParseWarning) and returns []. An entirely
    absent Risks section is fine — no warning, just empty list."""
    md = """\
# Plan: X
## Goal
g
## Task Graph
| ID | Title | Owner Role | Depends On | Est. Tokens |
|----|-------|------------|------------|-------------|
| T1 | a | Backend | — | 100 |
## Acceptance Criteria
1. ok
## Risks
Just some prose, no bulleted risks.
"""
    with warnings.catch_warnings(record=True) as w:
        warnings.simplefilter("always")
        p = parse_plan(md)
    assert any(issubclass(x.category, PlanParseWarning) for x in w)
    assert p.risks == []


def test_absent_risks_section_no_warning() -> None:
    """No Risks section at all → fine, no warning, risks=[]."""
    md = """\
# Plan: X
## Goal
g
## Task Graph
| ID | Title | Owner Role | Depends On | Est. Tokens |
|----|-------|------------|------------|-------------|
| T1 | a | Backend | — | 100 |
## Acceptance Criteria
1. ok
"""
    with warnings.catch_warnings(record=True) as w:
        warnings.simplefilter("always")
        p = parse_plan(md)
    assert not any(issubclass(x.category, PlanParseWarning) for x in w)
    assert p.risks == []


def test_prose_only_goal_ok() -> None:
    md = """\
# Plan: X
## Goal
Just some prose, no special structure. Multiple lines.
Even bullet-looking things:
- but this is just text, not a real list.
## Architecture
Multi-line prose.
## Task Graph
| ID | Title | Owner Role | Depends On | Est. Tokens |
|----|-------|------------|------------|-------------|
| T1 | t | Backend | — | 100 |
## Acceptance Criteria
1. ok
## Out of Scope
- thing
"""
    p = parse_plan(md)
    assert "Multiple lines" in p.goal
    assert p.risks == []
    assert p.out_of_scope == ["thing"]


def test_apis_table_with_method_path() -> None:
    md = """\
# Plan: X
## Goal
g
## Task Graph
| ID | Title | Owner Role | Depends On | Est. Tokens |
|----|-------|------------|------------|-------------|
| T1 | t | Backend | — | 100 |
## APIs / Interfaces
| Method | Path | Auth | Body |
|--------|------|------|------|
| POST | /api/login | JWT | email+password |
| GET | /api/me | JWT | — |
## Acceptance Criteria
1. ok
"""
    p = parse_plan(md)
    assert len(p.apis) == 2
    assert p.apis[0].method == "POST"
    assert p.apis[0].path == "/api/login"
    assert p.apis[1].method == "GET"