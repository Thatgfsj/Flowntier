"""Agent base class.

An agent is an async callable bound to a role and a model. It
receives a context dict and returns a result dict. The state machine
in `workflow/state_machine.py` is the only owner of state mutation.
"""

from __future__ import annotations

import enum
from abc import ABC, abstractmethod
from collections.abc import Awaitable, Callable
from dataclasses import dataclass
from typing import Any


class AgentRole(enum.StrEnum):
    """Roles defined in `PROJECT_SPEC.md` §3."""

    CHIEF = "agent:chief"
    CRITIC_A = "agent:critic:a"
    CRITIC_B = "agent:critic:b"
    WORKER = "agent:worker"
    PLANNER = "agent:planner"
    REPORTER = "agent:reporter"
    MERGER = "agent:merger"
    FINAL_REVIEWER = "agent:final_reviewer"


@dataclass(frozen=True, slots=True)
class AgentResult:
    """What an agent returns to the state machine."""

    role: AgentRole
    data: dict[str, Any]


class Agent(ABC):
    """Base class. Subclass and implement `run`."""

    role: AgentRole

    @abstractmethod
    async def run(self, ctx: dict[str, Any]) -> AgentResult:
        """Execute the agent. Called by the state machine."""
        raise NotImplementedError


# An agent is just a function in the simplest case; this alias lets
# callers pass plain callables.
AgentFactory = Callable[[], Agent]
AgentRunner = Callable[[dict[str, Any]], Awaitable[AgentResult]]
