"""ACO AI runtime library.

This is the production code path for the workflow engine. The
`apps/runtime` sidecar is a thin HTTP shell around it.

See `docs/ARCHITECTURE.md` §4 and `docs/WORKFLOW_SPEC.md` §4.
"""

from aco_runtime_lib.event_bus import EventBus, WfEvent
from aco_runtime_lib.workflow import (
    LogEntry,
    ResumableWorkflow,
    State,
    StateMachine,
    Transition,
    WorkflowCtx,
    WorkflowLog,
    TERMINAL_STATES,
    find_resumable,
)

__version__ = "0.2.0"

__all__ = [
    "EventBus",
    "LogEntry",
    "ResumableWorkflow",
    "State",
    "StateMachine",
    "Transition",
    "TERMINAL_STATES",
    "WfEvent",
    "WorkflowCtx",
    "WorkflowLog",
    "__version__",
    "find_resumable",
]
