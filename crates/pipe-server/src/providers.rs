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
        base_url: "https://api.xiaomi.com/v1",
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

/// Hard-coded fallback list of Anthropic models for the UI when
/// `/v1/models` is not available (Anthropic doesn't ship one).
pub const ANTHROPIC_FALLBACK_MODELS: &[(&str, &str)] = &[
    ("claude-opus-4-8", "Claude Opus 4.8 (recommended)"),
    ("claude-sonnet-4-6", "Claude Sonnet 4.6"),
    ("claude-haiku-4-5-20251022", "Claude Haiku 4.5 (fast)"),
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