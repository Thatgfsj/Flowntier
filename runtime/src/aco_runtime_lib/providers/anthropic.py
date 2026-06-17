"""Anthropic provider. Real API client for Claude models.

NOT exercised by unit tests (covered by `MockProvider` and the
`AnthropicProvider` smoke test in Phase 2 against a recorded
response). The implementation here is for production use.

See `docs/PROVIDER_SPEC.md` §3 + §8 (Anthropic Messages API).
"""

from __future__ import annotations

import os
from collections.abc import AsyncIterator
from typing import Any

import httpx

from aco_runtime_lib.providers.base import (
    ChatMessage,
    ChatRequest,
    ChatResponse,
    FinishReason,
    Provider,
    ProviderError,
    Usage,
)


class AnthropicProvider(Provider):
    """Anthropic Messages API client.

    Reads the API key from the `api_key_env` env var (never from a
    file — see `docs/SECURITY.md` §2).
    """

    DEFAULT_BASE_URL = "https://api.anthropic.com"
    DEFAULT_API_VERSION = "2023-06-01"

    def __init__(
        self,
        api_key: str | None = None,
        api_key_env: str = "ANTHROPIC_API_KEY",
        base_url: str = DEFAULT_BASE_URL,
        api_version: str = DEFAULT_API_VERSION,
        client: httpx.AsyncClient | None = None,
    ) -> None:
        if api_key is None:
            api_key = os.environ.get(api_key_env)
        if not api_key:
            raise ProviderError(
                f"missing API key: set {api_key_env} env var",
                retryable=False,
            )
        self._api_key = api_key
        self._base_url = base_url.rstrip("/")
        self._api_version = api_version
        self._client = client or httpx.AsyncClient(
            base_url=self._base_url,
            timeout=httpx.Timeout(120.0, connect=10.0),
        )
        self.id = "anthropic"
        self.capabilities: frozenset[str] = frozenset(
            {
                "chat",
                "stream",
                "vision",
                "tool_call",
                "json_mode",
                "prompt_caching",
                "reasoning_effort",
            }
        )

    async def chat(self, req: ChatRequest) -> ChatResponse:
        body = _build_request_body(req)
        headers = {
            "x-api-key": self._api_key,
            "anthropic-version": self._api_version,
            "content-type": "application/json",
        }
        try:
            resp = await self._client.post("/v1/messages", json=body, headers=headers)
        except httpx.HTTPError as e:
            raise ProviderError(f"network error: {e}", retryable=True) from e
        return _parse_response(resp, req)

    async def stream(self, req: ChatRequest) -> AsyncIterator[str]:
        # Anthropic SSE streaming. We yield the assistant text tokens
        # as they arrive.
        body = _build_request_body(req) | {"stream": True}
        headers = {
            "x-api-key": self._api_key,
            "anthropic-version": self._api_version,
            "content-type": "application/json",
        }
        async with self._client.stream(
            "POST", "/v1/messages", json=body, headers=headers
        ) as resp:
            resp.raise_for_status()
            async for line in resp.aiter_lines():
                if not line:
                    continue
                if line.startswith("data: "):
                    import json as _json

                    try:
                        evt = _json.loads(line[len("data: ") :])
                    except _json.JSONDecodeError:
                        continue
                    block = evt.get("content_block") or {}
                    if block.get("type") == "text":
                        text = block.get("text", "")
                        if text:
                            yield text

    def context_window(self, model: str) -> int:
        # Conservative defaults; override via the model registry in
        # Phase 2 (PROVIDER_SPEC §4).
        if "opus" in model or "sonnet" in model or "haiku" in model:
            return 200_000
        return 128_000


# ── Internals ────────────────────────────────────────────────────


def _build_request_body(req: ChatRequest) -> dict[str, Any]:
    system_parts: list[str] = []
    user_messages: list[dict[str, Any]] = []
    for m in req.messages:
        if m.role == "system":
            system_parts.append(m.content)
        elif m.role == "user":
            user_messages.append({"role": "user", "content": m.content})
        elif m.role == "assistant":
            user_messages.append({"role": "assistant", "content": m.content})
        # "tool" messages are mapped to user-side tool_result blocks
        # in Phase 2; Phase 1 does not use tools.

    body: dict[str, Any] = {
        "model": req.model,
        "max_tokens": req.max_tokens or 4096,
        "messages": user_messages,
    }
    if system_parts:
        body["system"] = "\n\n".join(system_parts)
    if req.temperature is not None:
        body["temperature"] = req.temperature
    if req.stop:
        body["stop_sequences"] = list(req.stop)
    if req.tools:
        body["tools"] = [
            {
                "name": t["name"],
                "description": t.get("description", ""),
                "input_schema": t.get("input_schema", {"type": "object"}),
            }
            for t in req.tools
        ]
    return body


def _parse_response(resp: httpx.Response, req: ChatRequest) -> ChatResponse:
    status = resp.status_code
    if status == 401 or status == 403:
        raise ProviderError(f"auth failed ({status})", retryable=False)
    if status == 429:
        retry_after = resp.headers.get("retry-after", "1")
        raise ProviderError(
            f"rate limited; retry after {retry_after}s",
            retryable=True,
        )
    if status >= 500:
        raise ProviderError(f"server error ({status})", retryable=True)
    if status >= 400:
        try:
            body = resp.json()
        except Exception:
            body = {"raw": resp.text}
        raise ProviderError(
            f"bad request ({status}): {body.get('error', {}).get('message', body)}",
            retryable=False,
        )

    data = resp.json()
    text_blocks = [b for b in data.get("content", []) if b.get("type") == "text"]
    content = "".join(b.get("text", "") for b in text_blocks)
    stop_reason = data.get("stop_reason", "end_turn")
    finish_reason = _map_finish_reason(stop_reason)
    usage_raw = data.get("usage", {})
    usage = Usage(
        input_tokens=int(usage_raw.get("input_tokens", 0)),
        output_tokens=int(usage_raw.get("output_tokens", 0)),
        cached_tokens=int(usage_raw.get("cache_read_input_tokens", 0)),
    )
    return ChatResponse(
        id=data.get("id", ""),
        model=data.get("model", req.model),
        content=content,
        finish_reason=finish_reason,
        usage=usage,
    )


def _map_finish_reason(stop_reason: str) -> FinishReason:
    return {
        "end_turn": FinishReason.STOP,
        "max_tokens": FinishReason.LENGTH,
        "tool_use": FinishReason.TOOL_CALL,
        "stop_sequence": FinishReason.STOP,
    }.get(stop_reason, FinishReason.OTHER)
