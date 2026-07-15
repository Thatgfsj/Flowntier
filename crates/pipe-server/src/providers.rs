//! Built-in provider presets.
//!
//! One static record per of the 9 LLM providers Flowntier ships
//! with. The Settings UI iterates this list to render the
//! "Add AI Provider" picker; the agent loop looks up entries by
//! `id` when resolving a role's `default_provider`.
//!
//! Persistence is per-preset in the `provider` table (enabled /
//! default_model override / base_url override). The presets here
//! are read-only defaults that the `provider` row is seeded from
//! at first migration time.
//!
//! When adding a new preset, also add:
//!   1. An INSERT into the `provider` table in migration 0003
//!   2. The preset here
//!   3. The secret-name convention (`<ID>_API_KEY` → see
//!      [`secret_name_for`])
//!   4. The CSP `connect-src` entry in `tauri.conf.json`

use serde::{Deserialize, Serialize};

/// A provider preset (mirrors `storage::ProviderRow` plus static
/// defaults).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderPreset {
    /// Stable id used everywhere (URL path, env var hint,
    /// frontend cache key).
    pub id: &'static str,
    /// Human-readable display name for the UI.
    pub display_name: &'static str,
    /// "openai-compatible" — speaks OpenAI's `/v1/chat/completions`
    /// and `/v1/models`. "anthropic-compatible" — speaks Anthropic's
    /// `/v1/messages`.
    pub kind: &'static str,
    /// Default base URL (no trailing slash).
    pub base_url: &'static str,
    /// Env var name the agent loop reads when no override is set.
    pub secret_name: &'static str,
    /// Default model id to use when the user hasn't picked one.
    pub default_model: &'static str,
    /// User-facing note shown when this provider is selected.
    pub note: &'static str,
    /// Whether the provider supports a live `/models` endpoint.
    /// Anthropic doesn't ship one — we fall back to a static list.
    pub has_live_models_endpoint: bool,
}

/// All 9 built-in presets. Order is the order they appear in the
/// Settings UI "Add Provider" picker.
pub const PRESETS: &[ProviderPreset] = &[
    ProviderPreset {
        id: "openai",
        display_name: "OpenAI",
        kind: "openai-compatible",
        base_url: "https://api.openai.com/v1",
        secret_name: "OPENAI_API_KEY",
        default_model: "gpt-4o",
        note: "GPT-4o, GPT-4 Turbo, o1-preview",
        has_live_models_endpoint: true,
    },
    ProviderPreset {
        id: "anthropic",
        display_name: "Anthropic",
        kind: "anthropic-compatible",
        base_url: "https://api.anthropic.com",
        secret_name: "ANTHROPIC_API_KEY",
        default_model: "claude-opus-4-8",
        note: "Claude Opus 4.8 (recommended), Sonnet 4.6, Haiku",
        has_live_models_endpoint: false, // Anthropic has no /v1/models
    },
    ProviderPreset {
        id: "google",
        display_name: "Google AI (Gemini)",
        kind: "openai-compatible",
        base_url: "https://generativelanguage.googleapis.com/v1beta/openai",
        secret_name: "GOOGLE_API_KEY",
        default_model: "gemini-2.0-flash",
        note: "Gemini 2.0 Flash (fast), 1.5 Pro (deep)",
        has_live_models_endpoint: true,
    },
    ProviderPreset {
        id: "deepseek",
        display_name: "DeepSeek",
        kind: "openai-compatible",
        base_url: "https://api.deepseek.com",
        secret_name: "DEEPSEEK_API_KEY",
        default_model: "deepseek-chat",
        note: "DeepSeek-V3 (chat), DeepSeek-R1 (reasoning)",
        has_live_models_endpoint: true,
    },
    ProviderPreset {
        id: "minimax",
        display_name: "MiniMax",
        kind: "openai-compatible",
        base_url: "https://api.minimaxi.com/v1",
        secret_name: "MINIMAX_API_KEY",
        default_model: "MiniMax-Text-01",
        note: "abab-6.5s / abab-7 (default MiniMax-Text-01)",
        has_live_models_endpoint: true,
    },
    ProviderPreset {
        id: "kimi",
        display_name: "Moonshot Kimi",
        kind: "openai-compatible",
        base_url: "https://api.moonshot.cn/v1",
        secret_name: "MOONSHOT_API_KEY",
        default_model: "moonshot-v1-128k",
        note: "Kimi K2 (1T MoE), v1-128k long context",
        has_live_models_endpoint: true,
    },
    ProviderPreset {
        id: "glm",
        display_name: "Zhipu GLM",
        kind: "openai-compatible",
        base_url: "https://open.bigmodel.cn/api/paas/v4",
        secret_name: "GLM_API_KEY",
        default_model: "glm-4-plus",
        note: "GLM-4-Plus, GLM-4-Air, GLM-Z1 (reasoning)",
        has_live_models_endpoint: true,
    },
    ProviderPreset {
        id: "mimo",
        display_name: "Xiaomi MiMo",
        kind: "openai-compatible",
        // v0.4.22 (event 000085): corrected base_url. The previous
        // value `https://api.xiaomi.com/v1` does not resolve (DNS
        // returns NXDOMAIN) — the actual Xiaomi MiMo endpoint is
        // `api.xiaomimimo.com` (CNAME → mimo-pri-prod.alb.xiaomi.com).
        // Chairman's `role_overrides` routes all 6 roles through
        // `mimo:mimo-2.5-pro`, so every phase hit this DNS failure
        // and the workflow hung for 5min × 8 phase = 40 min before
        // the orchestrator's per-phase timeout fired. Verified via
        // `nslookup` + `curl` against the live endpoint.
        base_url: "https://api.xiaomimimo.com/v1",
        secret_name: "MIMO_API_KEY",
        default_model: "mimo-v1",
        note: "MiMo-7B (preview)",
        has_live_models_endpoint: true,
    },
    ProviderPreset {
        id: "siliconflow",
        display_name: "SiliconFlow",
        kind: "openai-compatible",
        base_url: "https://api.siliconflow.cn/v1",
        secret_name: "SILICONFLOW_API_KEY",
        default_model: "Qwen/Qwen2.5-72B-Instruct",
        note: "Aggregates many OSS models — Qwen, GLM, Yi, DeepSeek",
        has_live_models_endpoint: true,
    },
];

/// Look up a preset by id. Returns None if the id is unknown —
/// callers should fall back to custom_provider lookup.
pub fn get(id: &str) -> Option<&'static ProviderPreset> {
    PRESETS.iter().find(|p| p.id == id)
}

/// Hard-coded fallback catalog used when a provider doesn't ship
/// a public `/v1/models` endpoint (Anthropic, MiniMax, Kimi, GLM,
/// MIMO, SiliconFlow all fall in this bucket as of v0.4.16). Each
/// entry carries thinking_strength + context_length so the role
/// dropdown can display them.
#[derive(Debug, Clone, Copy)]
pub struct ModelEntry {
    pub id: &'static str,
    pub display_name: &'static str,
    pub thinking_strength: &'static str, // "low" | "medium" | "high"
    pub context_length: u32,
}

pub const ANTHROPIC_FALLBACK_MODELS: &[ModelEntry] = &[
    ModelEntry { id: "claude-opus-4-8",            display_name: "Claude Opus 4.8 (recommended)", thinking_strength: "high",   context_length: 200_000 },
    ModelEntry { id: "claude-sonnet-4-6",         display_name: "Claude Sonnet 4.6",             thinking_strength: "medium", context_length: 200_000 },
    ModelEntry { id: "claude-haiku-4-5-20251022", display_name: "Claude Haiku 4.5 (fast)",       thinking_strength: "low",    context_length: 200_000 },
];

// OpenAI-compatible providers that don't expose a /v1/models endpoint.
// v0.4.16 (event 000052): chairman needs thinking_strength +
// context_length metadata to choose models. These are best-effort
// defaults — the user can override per-model in the custom-models UI.
pub const OPENAI_FALLBACK_MODELS: &[(&str, &[ModelEntry])] = &[
    ("minimax", &[
        ModelEntry { id: "MiniMax-Text-01",  display_name: "MiniMax M3 (recommended)",    thinking_strength: "high",   context_length: 128_000 },
        ModelEntry { id: "abab-6.5s-chat",   display_name: "abab-6.5s (fast)",            thinking_strength: "low",    context_length:  32_000 },
        ModelEntry { id: "abab-7-chat",      display_name: "abab-7",                      thinking_strength: "medium", context_length:  64_000 },
    ]),
    ("kimi", &[
        ModelEntry { id: "moonshot-v1-128k", display_name: "Moonshot v1 128k (Kimi K2)", thinking_strength: "medium", context_length: 128_000 },
        ModelEntry { id: "moonshot-v1-32k",  display_name: "Moonshot v1 32k",             thinking_strength: "medium", context_length:  32_000 },
    ]),
    ("glm", &[
        ModelEntry { id: "glm-4",     display_name: "GLM-4 (recommended)", thinking_strength: "high",   context_length: 128_000 },
        ModelEntry { id: "glm-3-turbo", display_name: "GLM-3 Turbo",         thinking_strength: "low",    context_length:  16_000 },
    ]),
    ("mimo", &[
        ModelEntry { id: "mimo-v1", display_name: "Xiaomi MiMo v1 (recommended)", thinking_strength: "high",   context_length: 64_000 },
    ]),
    ("siliconflow", &[
        ModelEntry { id: "Qwen/Qwen2.5-72B-Instruct", display_name: "Qwen2.5 72B", thinking_strength: "high", context_length: 32_000 },
        ModelEntry { id: "deepseek-ai/DeepSeek-V2.5",  display_name: "DeepSeek V2.5", thinking_strength: "high", context_length: 32_000 },
        ModelEntry { id: "meta-llama/Meta-Llama-3.1-70B-Instruct", display_name: "Llama 3.1 70B", thinking_strength: "medium", context_length: 32_000 },
    ]),
    ("openai", &[
        ModelEntry { id: "gpt-4o",       display_name: "GPT-4o (recommended)", thinking_strength: "high",   context_length: 128_000 },
        ModelEntry { id: "gpt-4o-mini",  display_name: "GPT-4o mini",          thinking_strength: "medium", context_length: 128_000 },
        ModelEntry { id: "o1",           display_name: "o1 (reasoning)",       thinking_strength: "high",   context_length: 200_000 },
        ModelEntry { id: "o3-mini",      display_name: "o3-mini",              thinking_strength: "high",   context_length: 200_000 },
        ModelEntry { id: "gpt-5",        display_name: "GPT-5",                thinking_strength: "high",   context_length: 400_000 },
        ModelEntry { id: "gpt-5-mini",   display_name: "GPT-5 mini",           thinking_strength: "medium", context_length: 400_000 },
    ]),
    ("google", &[
        ModelEntry { id: "gemini-2.5-pro",   display_name: "Gemini 2.5 Pro (recommended)", thinking_strength: "high",   context_length: 1_000_000 },
        ModelEntry { id: "gemini-2.5-flash", display_name: "Gemini 2.5 Flash",             thinking_strength: "medium", context_length: 1_000_000 },
        ModelEntry { id: "gemini-1.5-pro",   display_name: "Gemini 1.5 Pro",               thinking_strength: "medium", context_length: 1_000_000 },
    ]),
    ("deepseek", &[
        ModelEntry { id: "deepseek-chat",    display_name: "DeepSeek Chat (V3)",     thinking_strength: "medium", context_length: 64_000 },
        ModelEntry { id: "deepseek-reasoner",display_name: "DeepSeek Reasoner (R1)", thinking_strength: "high",   context_length: 64_000 },
    ]),
    ("anthropic", &[
        ModelEntry { id: "claude-opus-4-8",    display_name: "Claude Opus 4.8 (recommended)", thinking_strength: "high",   context_length: 200_000 },
        ModelEntry { id: "claude-sonnet-4-6", display_name: "Claude Sonnet 4.6",             thinking_strength: "medium", context_length: 200_000 },
        ModelEntry { id: "claude-haiku-4-5-20251022", display_name: "Claude Haiku 4.5 (fast)", thinking_strength: "low", context_length: 200_000 },
    ]),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_are_unique_and_complete() {
        let mut ids: Vec<&str> = PRESETS.iter().map(|p| p.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), PRESETS.len(), "duplicate preset ids");
        assert_eq!(PRESETS.len(), 9, "expected 9 presets");
    }

    #[test]
    fn every_preset_has_a_secret_name() {
        for p in PRESETS {
            assert!(
                p.secret_name.ends_with("_API_KEY"),
                "{}: secret_name must end with _API_KEY",
                p.id
            );
        }
    }

    #[test]
    fn get_returns_correct_preset() {
        assert_eq!(get("openai").unwrap().display_name, "OpenAI");
        assert_eq!(get("anthropic").unwrap().kind, "anthropic-compatible");
        assert!(get("nonexistent").is_none());
    }
}