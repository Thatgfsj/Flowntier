"""LLM provider layer. See `docs/PROVIDER_SPEC.md`."""

from aco_runtime_lib.providers.anthropic import AnthropicProvider
from aco_runtime_lib.providers.base import (
    ChatMessage,
    ChatRequest,
    ChatResponse,
    FinishReason,
    Provider,
    ProviderError,
    Usage,
)
from aco_runtime_lib.providers.google import GoogleProvider
from aco_runtime_lib.providers.manager import (
    ProviderManager,
    ProviderStatus,
    RoleAssignment,
)
from aco_runtime_lib.providers.minimax import MiniMaxProvider
from aco_runtime_lib.providers.mock import MockProvider, ScriptedResponse
from aco_runtime_lib.providers.openai import OpenAIProvider
from aco_runtime_lib.providers.presets import (
    PROVIDER_PRESETS,
    PresetModel,
    ProviderPreset,
    get_preset,
)
from aco_runtime_lib.providers.router import (
    ModelRef,
    ModelRouter,
    RouterConfig,
    RouterError,
    default_router,
)

__all__ = [
    "PROVIDER_PRESETS",
    "AnthropicProvider",
    "ChatMessage",
    "ChatRequest",
    "ChatResponse",
    "FinishReason",
    "GoogleProvider",
    "MiniMaxProvider",
    "MockProvider",
    "ModelRef",
    "ModelRouter",
    "OpenAIProvider",
    "PresetModel",
    "Provider",
    "ProviderError",
    "ProviderManager",
    "ProviderPreset",
    "ProviderStatus",
    "RoleAssignment",
    "RouterConfig",
    "RouterError",
    "ScriptedResponse",
    "Usage",
    "default_router",
    "get_preset",
]
