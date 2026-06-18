"""Deterministic MockProvider for tests.

Returns canned responses based on the request content. Supports
two modes:

* **Scripted** — the test pre-registers a list of (matcher, response)
  pairs. The first match wins.
* **Auto** — if no match, generates a deterministic JSON response
  derived from the input hash. Useful for smoke tests where the
  exact content doesn't matter.

Cost and token usage are configurable per response.
"""

from __future__ import annotations

import hashlib
import re
from collections.abc import AsyncIterator
from dataclasses import dataclass, field
from typing import Any

from aco_runtime_lib.providers.base import (
    ChatMessage,
    ChatRequest,
    ChatResponse,
    FinishReason,
    Provider,
    Usage,
)


@dataclass(slots=True)
class ScriptedResponse:
    """One canned response in the mock script."""

    matcher: str  # a regex applied to the concatenated user/system text
    content: str
    input_tokens: int = 100
    output_tokens: int = 100
    cost_usd: float = 0.0
    finish_reason: FinishReason = FinishReason.STOP


class MockProvider(Provider):
    """Deterministic provider for tests."""

    id: str = "mock"
    capabilities: frozenset[str] = frozenset(
        {"chat", "stream", "tool_call", "json_mode"}
    )

    def __init__(self) -> None:
        self._script: list[ScriptedResponse] = []
        self._default_content: str = '{"ok": true}'
        self._calls: list[ChatRequest] = []

    # ── Test helpers ──────────────────────────────────────────────

    def when(self, matcher: str, content: str, **kwargs: Any) -> None:
        """Append a scripted response."""
        self._script.append(ScriptedResponse(matcher=matcher, content=content, **kwargs))

    def set_default(self, content: str) -> None:
        """Set the fallback response when no matcher hits."""
        self._default_content = content

    @property
    def calls(self) -> list[ChatRequest]:
        """Every request the provider has received (read-only copy)."""
        return list(self._calls)

    # ── Provider impl ────────────────────────────────────────────

    async def chat(self, req: ChatRequest) -> ChatResponse:
        self._calls.append(req)
        prompt = _flatten_messages(req.messages)
        for entry in self._script:
            if re.search(entry.matcher, prompt):
                return _build_response(req, entry)
        # Fallback: deterministic auto-response.
        return _build_response(
            req,
            ScriptedResponse(
                matcher="",
                content=_auto_response(prompt),
                input_tokens=len(prompt) // 4,
                output_tokens=len(self._default_content) // 4,
            ),
        )

    async def stream(self, req: ChatRequest) -> AsyncIterator[str]:
        # Yield the canned response as one chunk (no real streaming
        # in mock mode).
        response = await self.chat(req)
        yield response.content

    def context_window(self, model: str) -> int:
        return 128_000


def _flatten_messages(messages: list[ChatMessage]) -> str:
    parts: list[str] = []
    for m in messages:
        parts.append(f"[{m.role}] {m.content}")
    return "\n".join(parts)


def _build_response(req: ChatRequest, scripted: ScriptedResponse) -> ChatResponse:
    return ChatResponse(
        id=f"mock-{_hash(req)}",
        model=req.model,
        content=scripted.content,
        finish_reason=scripted.finish_reason,
        usage=Usage(
            input_tokens=scripted.input_tokens,
            output_tokens=scripted.output_tokens,
            cached_tokens=0,
            cost_usd=scripted.cost_usd,
        ),
    )


def _hash(req: ChatRequest) -> str:
    h = hashlib.sha256()
    h.update(req.model.encode())
    for m in req.messages:
        h.update(m.role.encode())
        h.update(m.content.encode())
    return h.hexdigest()[:12]


def _auto_response(prompt: str) -> str:
    """Deterministic fallback when no matcher hits."""
    digest = hashlib.sha256(prompt.encode()).hexdigest()[:8]
    return (
        '{"status":"ok","auto":true,"digest":"' + digest + '",'
        '"summary":"auto-generated mock response"}'
    )
