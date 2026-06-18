"""Provider presets catalog.

The 11 known provider kinds from `docs/PROVIDER_SPEC.md` §2. Each
preset is **metadata only** — the API key lives in the env var named
in `api_key_env` and is read at provider-construction time
(see `ProviderManager`).

Adding a new provider:
1. Add an entry to `PROVIDER_PRESETS` below.
2. (Optional) implement a real `Provider` subclass in providers/.
3. Done — the UI, manager, and router pick it up automatically.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Literal

# Provider kinds we ship out of the box. New kinds (e.g. a non-OpenAI
# API) require a custom `Provider` subclass — see PROVIDER_SPEC §10.
ProviderKind = Literal[
    "anthropic",
    "openai",
    "google",
    "openai_compat",
]


@dataclass(frozen=True)
class PresetModel:
    id: str
    display_name: str
    context_window: int
    max_output_tokens: int
    input_cost_mtok: float = 0.0
    output_cost_mtok: float = 0.0
    capabilities: tuple[str, ...] = ("chat", "stream", "json_mode")


@dataclass(frozen=True)
class ProviderPreset:
    """Catalog entry for a known provider.

    Mirrors the `[providers.<id>]` section in `config/providers.toml`
    (see CONFIG.md §4) but lives in code so the UI can list and
    pre-select these without shipping a TOML.
    """

    id: str
    display_name: str
    kind: ProviderKind
    base_url: str
    api_key_env: str
    """Name of the env var holding the API key. NEVER the key itself."""
    default_models: tuple[PresetModel, ...]
    notes: str = ""
    is_local: bool = False
    """Local providers (Ollama, LM Studio) don't need a real key."""


# ── Catalog ──────────────────────────────────────────────────────


PROVIDER_PRESETS: tuple[ProviderPreset, ...] = (
    ProviderPreset(
        id="anthropic",
        display_name="Anthropic",
        kind="anthropic",
        base_url="https://api.anthropic.com",
        api_key_env="ANTHROPIC_API_KEY",
        default_models=(
            PresetModel(
                "claude-opus-4-8",
                "Claude Opus 4.8",
                200_000,
                32_000,
                15.0,
                75.0,
                (
                    "chat",
                    "stream",
                    "vision",
                    "tool_call",
                    "json_mode",
                    "prompt_caching",
                    "reasoning_effort",
                ),
            ),
            PresetModel(
                "claude-sonnet-4-6",
                "Claude Sonnet 4.6",
                200_000,
                16_000,
                3.0,
                15.0,
                (
                    "chat",
                    "stream",
                    "vision",
                    "tool_call",
                    "json_mode",
                    "prompt_caching",
                    "reasoning_effort",
                ),
            ),
            PresetModel(
                "claude-haiku-4-5",
                "Claude Haiku 4.5",
                200_000,
                8_000,
                1.0,
                5.0,
                ("chat", "stream", "vision", "tool_call", "json_mode"),
            ),
        ),
        notes="First-class. Native vision + tool calling.",
    ),
    ProviderPreset(
        id="openai",
        display_name="OpenAI",
        kind="openai",
        base_url="https://api.openai.com/v1",
        api_key_env="OPENAI_API_KEY",
        default_models=(
            PresetModel(
                "gpt-5",
                "GPT-5",
                128_000,
                16_000,
                5.0,
                20.0,
                ("chat", "stream", "vision", "tool_call", "json_mode"),
            ),
            PresetModel(
                "gpt-5-mini",
                "GPT-5 Mini",
                128_000,
                16_000,
                0.5,
                2.0,
                ("chat", "stream", "tool_call", "json_mode"),
            ),
        ),
        notes="GPT-5 / GPT-5-mini.",
    ),
    ProviderPreset(
        id="google",
        display_name="Google Gemini",
        kind="google",
        base_url="https://generativelanguage.googleapis.com",
        api_key_env="GOOGLE_API_KEY",
        default_models=(
            PresetModel(
                "gemini-2-5-pro",
                "Gemini 2.5 Pro",
                1_000_000,
                8_000,
                2.5,
                10.0,
                ("chat", "stream", "vision", "tool_call", "json_mode"),
            ),
            PresetModel(
                "gemini-2-5-flash",
                "Gemini 2.5 Flash",
                1_000_000,
                8_000,
                0.5,
                2.0,
                ("chat", "stream", "vision", "tool_call", "json_mode"),
            ),
        ),
        notes="1M context window.",
    ),
    ProviderPreset(
        id="kimi",
        display_name="Kimi (Moonshot)",
        kind="openai_compat",
        base_url="https://api.moonshot.cn/v1",
        api_key_env="MOONSHOT_API_KEY",
        default_models=(
            PresetModel(
                "kimi-k2",
                "Kimi K2",
                128_000,
                8_000,
                1.0,
                3.0,
                ("chat", "stream", "tool_call", "json_mode"),
            ),
        ),
        notes="Chinese provider. OpenAI-compatible.",
    ),
    ProviderPreset(
        id="minimax",
        display_name="MiniMax",
        kind="openai_compat",
        base_url="https://api.minimaxi.com/v1",
        api_key_env="MINIMAX_API_KEY",
        default_models=(
            PresetModel(
                "minimax-m3", "MiniMax M3", 32_000, 8_000, 0.5, 1.0, ("chat", "stream", "json_mode")
            ),
        ),
        notes="Chinese provider. OpenAI-compatible.",
    ),
    ProviderPreset(
        id="deepseek",
        display_name="DeepSeek",
        kind="openai_compat",
        base_url="https://api.deepseek.com/v1",
        api_key_env="DEEPSEEK_API_KEY",
        default_models=(
            PresetModel(
                "deepseek-chat",
                "DeepSeek Chat",
                64_000,
                8_000,
                0.27,
                1.1,
                ("chat", "stream", "tool_call", "json_mode"),
            ),
            PresetModel(
                "deepseek-reasoner",
                "DeepSeek Reasoner",
                64_000,
                32_000,
                0.55,
                2.19,
                ("chat", "stream", "reasoning_effort"),
            ),
        ),
        notes="Reasoner model has chain-of-thought.",
    ),
    ProviderPreset(
        id="siliconflow",
        display_name="SiliconFlow",
        kind="openai_compat",
        base_url="https://api.siliconflow.cn/v1",
        api_key_env="SILICONFLOW_API_KEY",
        default_models=(
            PresetModel(
                "Qwen/Qwen2.5-72B-Instruct",
                "Qwen 2.5 72B",
                32_000,
                8_000,
                0.0,
                0.0,
                ("chat", "stream", "tool_call", "json_mode"),
            ),
        ),
        notes="Chinese aggregator. Free tier available.",
    ),
    ProviderPreset(
        id="openrouter",
        display_name="OpenRouter",
        kind="openai_compat",
        base_url="https://openrouter.ai/api/v1",
        api_key_env="OPENROUTER_API_KEY",
        default_models=(
            PresetModel(
                "auto",
                "OpenRouter (auto)",
                128_000,
                8_000,
                0.0,
                0.0,
                ("chat", "stream", "vision", "tool_call", "json_mode"),
            ),
        ),
        notes="Aggregator. Routes to the best model per request.",
    ),
    ProviderPreset(
        id="ollama",
        display_name="Ollama (local)",
        kind="openai_compat",
        base_url="http://localhost:11434/v1",
        api_key_env="OLLAMA_NO_KEY",
        default_models=(
            PresetModel(
                "llama3.3",
                "Llama 3.3 70B (local)",
                8_192,
                4_096,
                0.0,
                0.0,
                ("chat", "stream", "json_mode"),
            ),
        ),
        notes="Local. Run `ollama serve` first.",
        is_local=True,
    ),
    ProviderPreset(
        id="lmstudio",
        display_name="LM Studio (local)",
        kind="openai_compat",
        base_url="http://localhost:1234/v1",
        api_key_env="LMSTUDIO_NO_KEY",
        default_models=(
            PresetModel(
                "loaded-model",
                "Loaded model (local)",
                8_192,
                4_096,
                0.0,
                0.0,
                ("chat", "stream", "json_mode"),
            ),
        ),
        notes="Local. Start LM Studio server first.",
        is_local=True,
    ),
    ProviderPreset(
        id="custom",
        display_name="Custom OpenAI-compatible",
        kind="openai_compat",
        base_url="",
        api_key_env="ACO_PROVIDER_CUSTOM_API_KEY",
        default_models=(),
        notes="Add a base URL and your API key env var.",
    ),
)


def get_preset(provider_id: str) -> ProviderPreset | None:
    for p in PROVIDER_PRESETS:
        if p.id == provider_id:
            return p
    return None
