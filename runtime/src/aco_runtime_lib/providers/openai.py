"""OpenAI Chat Completions provider."""

from __future__ import annotations

import os
from collections.abc import AsyncIterator
from typing import Any

import httpx

from aco_runtime_lib.providers.base import (
    ChatRequest,
    ChatResponse,
    FinishReason,
    Provider,
    ProviderError,
    Usage,
)


class OpenAIProvider(Provider):
    DEFAULT_BASE_URL = "https://api.openai.com/v1"

    def __init__(
        self,
        api_key: str | None = None,
        api_key_env: str = "OPENAI_API_KEY",
        base_url: str = DEFAULT_BASE_URL,
        client: httpx.AsyncClient | None = None,
    ) -> None:
        if api_key is None:
            api_key = os.environ.get(api_key_env)
        if not api_key:
            raise ProviderError(f"missing API key: set {api_key_env} env var", retryable=False)
        self._api_key = api_key
        self._base_url = base_url.rstrip("/")
        self._client = client or httpx.AsyncClient(
            base_url=self._base_url,
            timeout=httpx.Timeout(120.0, connect=10.0),
        )
        self.id = "openai"
        self.capabilities: frozenset[str] = frozenset(
            {"chat", "stream", "vision", "tool_call", "json_mode"}
        )

    async def chat(self, req: ChatRequest) -> ChatResponse:
        body = _build_body(req)
        headers = {
            "authorization": f"Bearer {self._api_key}",
            "content-type": "application/json",
        }
        try:
            resp = await self._client.post("/chat/completions", json=body, headers=headers)
        except httpx.HTTPError as e:
            raise ProviderError(f"network error: {e}", retryable=True) from e
        return _parse_response(resp, req)

    async def stream(self, req: ChatRequest) -> AsyncIterator[str]:
        body = _build_body(req) | {"stream": True}
        headers = {
            "authorization": f"Bearer {self._api_key}",
            "content-type": "application/json",
        }
        async with self._client.stream(
            "POST", "/chat/completions", json=body, headers=headers
        ) as resp:
            resp.raise_for_status()
            async for line in resp.aiter_lines():
                if not line.startswith("data: "):
                    continue
                payload = line[len("data: ") :]
                if payload.strip() == "[DONE]":
                    break
                import json as _json

                try:
                    evt = _json.loads(payload)
                except _json.JSONDecodeError:
                    continue
                for choice in evt.get("choices", []):
                    delta = choice.get("delta") or {}
                    text = delta.get("content")
                    if text:
                        yield text

    def context_window(self, model: str) -> int:
        if "gpt-5" in model.lower():
            return 128_000
        return 16_000


def _build_body(req: ChatRequest) -> dict[str, Any]:
    return {
        "model": req.model,
        "messages": [{"role": m.role, "content": m.content} for m in req.messages],
        "max_tokens": req.max_tokens or 2048,
        "temperature": req.temperature if req.temperature is not None else 0.7,
    }


def _parse_response(resp: httpx.Response, req: ChatRequest) -> ChatResponse:
    status = resp.status_code
    if status in (401, 403):
        raise ProviderError(f"auth failed ({status})", retryable=False)
    if status == 429:
        raise ProviderError("rate limited", retryable=True)
    if status >= 500:
        raise ProviderError(f"server error ({status})", retryable=True)
    if status >= 400:
        raise ProviderError(f"bad request ({status}): {resp.text}", retryable=False)
    data = resp.json()
    choices = data.get("choices", [])
    if not choices:
        raise ProviderError("no choices in response", retryable=False)
    message = choices[0].get("message", {})
    content = message.get("content", "")
    finish_raw = choices[0].get("finish_reason", "stop")
    finish_reason = {
        "stop": FinishReason.STOP,
        "length": FinishReason.LENGTH,
        "tool_calls": FinishReason.TOOL_CALL,
    }.get(finish_raw, FinishReason.OTHER)
    usage_raw = data.get("usage", {})
    return ChatResponse(
        id=data.get("id", ""),
        model=data.get("model", req.model),
        content=content,
        finish_reason=finish_reason,
        usage=Usage(
            input_tokens=int(usage_raw.get("prompt_tokens", 0)),
            output_tokens=int(usage_raw.get("completion_tokens", 0)),
            cached_tokens=0,
        ),
    )
