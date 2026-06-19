"""ACO AI runtime library.

This is the production code path for the workflow engine. The
`apps/runtime` sidecar is a thin HTTP shell around it.

See `docs/ARCHITECTURE.md` §4 and `docs/WORKFLOW_SPEC.md` §4.
"""

from aco_runtime_lib.event_bus import EventBus, WfEvent
from aco_runtime_lib.workflow import (
    TERMINAL_STATES,
    LogEntry,
    ResumableWorkflow,
    State,
    StateMachine,
    Transition,
    WorkflowCtx,
    WorkflowLog,
    find_resumable,
)

__version__ = "0.2.2"

__all__ = [
    "TERMINAL_STATES",
    "EventBus",
    "LogEntry",
    "ResumableWorkflow",
    "State",
    "StateMachine",
    "Transition",
    "WfEvent",
    "WorkflowCtx",
    "WorkflowLog",
    "__version__",
    "find_resumable",
]
