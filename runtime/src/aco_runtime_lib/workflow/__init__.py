"""Workflow state machine package.

See `docs/WORKFLOW_SPEC.md` for the canonical state catalog and
transition table. The state names in this module are the source of
truth for the Python runtime; the Rust core mirrors them in
`crates/event-bus` (events) and `crates/storage` (DB rows).
"""

from aco_runtime_lib.workflow.orchestrator import (
    OrchestratorOptions,
    OrchestratorResult,
    WorkflowOrchestrator,
)
from aco_runtime_lib.workflow.persistence import (
    LogEntry,
    WorkflowLog,
    iter_entries_sync,
    last_entry,
)
from aco_runtime_lib.workflow.plan_parser import (
    AcceptanceCriterion,
    ApiEndpoint,
    Edge,
    ParsedPlan,
    PlanParseError,
    PlanParseWarning,
    Risk,
    SchemaChange,
    TaskNode,
    parse_plan,
)
from aco_runtime_lib.workflow.recovery import ResumableWorkflow, find_resumable
from aco_runtime_lib.workflow.state_machine import (
    TERMINAL_STATES,
    State,
    StateMachine,
    Transition,
    WorkflowCtx,
)

__all__ = [
    "AcceptanceCriterion",
    "ApiEndpoint",
    "Edge",
    "LogEntry",
    "OrchestratorOptions",
    "OrchestratorResult",
    "ParsedPlan",
    "PlanParseError",
    "PlanParseWarning",
    "ResumableWorkflow",
    "Risk",
    "SchemaChange",
    "State",
    "StateMachine",
    "TaskNode",
    "Transition",
    "WorkflowCtx",
    "WorkflowLog",
    "WorkflowOrchestrator",
    "find_resumable",
    "iter_entries_sync",
    "last_entry",
    "parse_plan",
]
