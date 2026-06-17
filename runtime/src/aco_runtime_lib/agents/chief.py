"""Chief Agent — the orchestrator.

In Phase 1, the Chief is a thin loop:

1. Send the user request to the provider.
2. Parse the response (JSON: either a `USER_QUERY` or a plan).
3. Emit a `WfEvent` on the bus for the UI to surface.
4. The state machine handles the transition; the Chief just
   produces the next decision.

See `docs/AGENT_PROTOCOL.md` §3, `prompts/chief_agent.md`.
"""

from __future__ import annotations

import json
import re
from dataclasses import dataclass
from typing import Any

from aco_runtime_lib.agents.base import Agent, AgentResult, AgentRole
from aco_runtime_lib.event_bus import EventBus
from aco_runtime_lib.providers.base import ChatMessage, ProviderError
from aco_runtime_lib.providers.router import ModelRouter


@dataclass(slots=True)
class ChiefOutput:
    """The Chief's response to a planning step."""

    kind: str  # "user_query" | "plan" | "summary"
    payload: dict[str, Any]


class ChiefAgent(Agent):
    """The Chief Agent. Phase 1: minimal logic — the real value is in
    the prompt + the response parsing.
    """

    role = AgentRole.CHIEF

    def __init__(
        self,
        router: ModelRouter,
        bus: EventBus,
        system_prompt: str = "You are the Chief Agent of ACO.",
    ) -> None:
        self._router = router
        self._bus = bus
        self._system_prompt = system_prompt

    async def run(self, ctx: dict[str, Any]) -> AgentResult:
        provider, ref = self._router.pick("chief")
        request_text = ctx.get("user_request", "")
        messages = [
            ChatMessage(role="system", content=self._system_prompt),
            ChatMessage(role="user", content=request_text),
        ]
        try:
            response = await provider.chat(
                _make_request(ref.model_id, messages, max_tokens=2048)
            )
        except ProviderError as e:
            return AgentResult(
                role=self.role,
                data={"error": "provider_error", "message": str(e), "retryable": e.retryable},
            )
        await self._bus.publish(
            _token_usage_event(
                self.role,
                provider.id,
                ref.model_id,
                response.usage.input_tokens,
                response.usage.output_tokens,
                response.usage.cost_usd,
            )
        )
        output = _parse_chief_response(response.content)
        return AgentResult(
            role=self.role,
            data={"kind": output.kind, "payload": output.payload, "raw": response.content},
        )


# ── Helpers ──────────────────────────────────────────────────────


def _make_request(model: str, messages: list[ChatMessage], max_tokens: int) -> Any:
    from aco_runtime_lib.providers.base import ChatRequest

    return ChatRequest(
        model=model,
        messages=messages,
        max_tokens=max_tokens,
        temperature=0.4,
    )


def _parse_chief_response(text: str) -> ChiefOutput:
    """Tolerantly extract a JSON object from the model's text.

    The Chief often wraps JSON in prose or fences; we scan for the
    last top-level JSON object in the text.
    """
    obj = _extract_last_json(text)
    if obj is None:
        return ChiefOutput(kind="summary", payload={"text": text})
    kind = obj.get("kind") or obj.get("type") or "summary"
    return ChiefOutput(kind=str(kind), payload=obj)


_JSON_OBJECT_RE = re.compile(r"\{.*\}", re.DOTALL)


def _extract_last_json(text: str) -> dict[str, Any] | None:
    matches = list(_JSON_OBJECT_RE.finditer(text))
    for m in reversed(matches):
        candidate = m.group(0)
        try:
            parsed = json.loads(candidate)
        except json.JSONDecodeError:
            continue
        if isinstance(parsed, dict):
            return parsed
    return None


def _token_usage_event(
    role: AgentRole,
    provider: str,
    model: str,
    input_tokens: int,
    output_tokens: int,
    cost_usd: float | None,
) -> Any:
    from aco_runtime_lib.event_bus import WfEvent

    return WfEvent(
        kind="token_usage",
        ts=_now_iso(),
        agent_id=role,
        provider=provider,
        model=model,
        input_tokens=input_tokens,
        output_tokens=output_tokens,
        cached_tokens=0,
        cost_usd=cost_usd,
    )


def _now_iso() -> str:
    from datetime import UTC, datetime

    return datetime.now(UTC).isoformat(timespec="milliseconds").replace("+00:00", "Z")
