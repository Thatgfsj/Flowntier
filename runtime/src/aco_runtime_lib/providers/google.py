"""Google Gemini provider (generateContent API).

Minimal v0.2 implementation. Phase 3 will add streaming and
tool-calling. See `docs/PROVIDER_SPEC.md` §3 + Google docs:
https://ai.google.dev/api/rest
"""

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


class GoogleProvider(Provider):
    DEFAULT_BASE_URL = "https://generativelanguage.googleapis.com"

    def __init__(
        self,
        api_key: str | None = None,
        api_key_env: str = "GOOGLE_API_KEY",
        base_url: str = DEFAULT_BASE_URL,
        client: httpx.AsyncClient | None = None,
    ) -> None:
        if api_key is None:
            api_key = os.environ.get(api_key_env)
        if not api_key:
            raise ProviderError(f"missing API key: set {api_key_env}", retryable=False)
        self._api_key = api_key
        self._base_url = base_url.rstrip("/")
        self._client = client or httpx.AsyncClient(
            base_url=self._base_url,
            timeout=httpx.Timeout(120.0, connect=10.0),
        )
        self.id = "google"
        self.capabilities: frozenset[str] = frozenset(
            {"chat", "stream", "vision", "tool_call", "json_mode"}
        )

    async def chat(self, req: ChatRequest) -> ChatResponse:
        url = f"/v1beta/models/{req.model}:generateContent"
        body = _build_body(req)
        try:
            resp = await self._client.post(url, json=body, params={"key": self._api_key})
        except httpx.HTTPError as e:
            raise ProviderError(f"network error: {e}", retryable=True) from e
        return _parse_response(resp, req)

    async def stream(self, req: ChatRequest) -> AsyncIterator[str]:
        # Gemini has a streamGenerateContent endpoint; v0.2 returns
        # the full response in one chunk to keep this stub minimal.
        response = await self.chat(req)
        yield response.content

    def context_window(self, model: str) -> int:
        if "flash" in model:
            return 1_000_000
        return 1_000_000


def _build_body(req: ChatRequest) -> dict[str, Any]:
    contents: list[dict[str, Any]] = []
    system: dict[str, str] | None = None
    for m in req.messages:
        if m.role == "system":
            system = {"text": m.content}
        elif m.role == "user":
            contents.append({"role": "user", "parts": [{"text": m.content}]})
        elif m.role == "assistant":
            contents.append({"role": "model", "parts": [{"text": m.content}]})
    body: dict[str, Any] = {"contents": contents}
    if system is not None:
        body["systemInstruction"] = system
    gen: dict[str, Any] = {"maxOutputTokens": req.max_tokens or 2048}
    if req.temperature is not None:
        gen["temperature"] = req.temperature
    body["generationConfig"] = gen
    return body


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
    candidates = data.get("candidates", [])
    if not candidates:
        raise ProviderError("no candidates in response", retryable=False)
    parts = candidates[0].get("content", {}).get("parts", [])
    content = "".join(p.get("text", "") for p in parts)
    finish_raw = candidates[0].get("finishReason", "STOP")
    finish_reason = {
        "STOP": FinishReason.STOP,
        "MAX_TOKENS": FinishReason.LENGTH,
        "SAFETY": FinishReason.CONTENT_FILTER,
    }.get(finish_raw, FinishReason.OTHER)
    usage_raw = data.get("usageMetadata", {})
    return ChatResponse(
        id=data.get("responseId", ""),
        model=req.model,
        content=content,
        finish_reason=finish_reason,
        usage=Usage(
            input_tokens=int(usage_raw.get("promptTokenCount", 0)),
            output_tokens=int(usage_raw.get("candidatesTokenCount", 0)),
            cached_tokens=0,
        ),
    )
