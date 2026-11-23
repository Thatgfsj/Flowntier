import type { ProviderInfo, RoleInfo } from "../../lib/api.js";

export interface RuntimeSnapshot {
  providers: ProviderInfo[];
  roles: RoleInfo[];
  available_models: {
    provider: string;
    provider_display: string;
    model: string;
    display_name: string;
  }[];
}

// ── Per-role assignment card ──────────────────────────────────────

export interface AvailableModel {
  provider: string;
  provider_display: string;
  model: string;
  display_name: string;
  thinking_strength?: string;
  context_length?: number | null;
}

// ── Custom Provider (relay station) ─────────────────────────────

// BUG-FRONTEND-RT-12 (event 000041): each model registered with
// a custom provider now carries an optional context_length
// (in tokens; e.g. 200000 for MiniMax-M3's 200k window) and a
// required thinking_strength enum. The chairman's directive
// was: "let the user fill in the model name + context length,
// and pick thinking strength from a list".
// v0.4.22 (event 000118, fix 4): chairman observed that
// `+ 添加 AI 供应商` forced him to fill in 思考强度 + 上下文
// for every model. Both fields are best-effort metadata; the
// runtime falls back to provider defaults when they're
// missing. We now accept an empty string and only persist the
// key when the user picked an actual value.
export type ThinkingStrength = "" | "low" | "medium" | "high";
export interface ModelRow {
  id: string;
  display_name: string;
  /** Context window size in tokens. null = use the runtime default. */
  context_length: number | null;
  /** '' = not specified (use runtime default). */
  thinking_strength: ThinkingStrength;
}
