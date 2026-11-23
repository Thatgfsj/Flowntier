import type { AvailableModel, RuntimeSnapshot } from "./types.js";

// v0.4.22 (event 000118, follow-up): the live /v1/models path on
// a non-curated provider can come back with `display_name` /
// `provider_display` / `model` missing — the Rust side serializes
// each as JSON `null` and Tauri's IPC drops those into TS
// `undefined`. The previous build hard-asserted those fields as
// `string` in the type, so the runtime crash
//   `Cannot read properties of undefined (reading 'includes')`
// slipped through `tsc --noEmit` and only blew up inside the
// React render. We now normalize every model row in a single
// pass at the network boundary, so all downstream code can
// trust that `m.display_name`, `m.model`, `m.provider_display`,
// `m.thinking_strength` are either string or null/undefined —
// never a `undefined` that someone forgot to guard.
//
// `safeStr` is intentionally permissive: `null`, `undefined`,
// numbers, and booleans all collapse to a human-readable
// placeholder so the user always sees SOMETHING in the dropdown
// rather than an empty option or a literal "undefined".
export function safeStr(v: unknown): string {
  if (v === null || v === undefined) return "";
  if (typeof v === "string") return v;
  if (typeof v === "number" || typeof v === "boolean") return String(v);
  return "";
}
export function normalizeModel(m: AvailableModel): AvailableModel {
  const id = safeStr(m.model);
  // v0.4.22 (event 000118, follow-up 2): the chairman
  // reported that the previous fallback chain collapsed every
  // live /v1/models row to `(unknown provider) · (unknown model)`
  // — `(unknown provider)` is hostile noise: the user has
  // perfectly usable `id` and `provider` strings, the
  // upstream just omitted `display_name` / `provider_display`.
  // Prefer the raw id + provider as the human-readable fallback
  // so the user always sees SOMETHING meaningful.
  const displayName = safeStr(m.display_name) || id || "(unnamed model)";
  const provider = safeStr(m.provider);
  const providerDisplay = safeStr(m.provider_display) || provider || id || "(unnamed provider)";
  // thinking_strength comes back as either a free string or an
  // enum ('low'|'medium'|'high'). Anything else is meaningless
  // to the badge, so collapse to ''.
  const ts =
    typeof m.thinking_strength === "string" && m.thinking_strength.length > 0
      ? m.thinking_strength
      : undefined;
  const ctx =
    typeof m.context_length === "number" && m.context_length > 0 ? m.context_length : null;
  // v0.4.22 (follow-up): exactOptionalPropertyTypes is on,
  // so we cannot pass `undefined` into a `?: string` slot —
  // build the result via spread, omitting optional keys when
  // they're undefined.
  const base: AvailableModel = {
    provider,
    provider_display: providerDisplay,
    model: id,
    display_name: displayName,
    context_length: ctx,
  };
  return ts ? { ...base, thinking_strength: ts } : base;
}
export function normalizeSnapshotModels(s: RuntimeSnapshot): RuntimeSnapshot {
  return {
    providers: s.providers,
    roles: s.roles,
    available_models: s.available_models.map(normalizeModel),
  };
}

// Module-scope lookup so both `modelOptionLabel` and
// `modelShortLabel` share the same canonical mapping and we
// don't allocate a new object on every render.
export const MODEL_FRIENDLY: Record<string, string> = {
  "MiniMax-M3": "MiniMax M3 (reasoning)",
  "MiniMax-Text-01": "MiniMax Text-01 (text, recommended)",
  "MiniMax-VL-01": "MiniMax VL-01 (vision+text)",
  "abab-6.5s-chat": "abab-6.5s (fast)",
  "abab-7-chat": "abab-7 (general)",
};

// ROLE_LABELS is module-scope (no React hook access). Use
// getRoleLabel(t, role) below to look up the localized string.
export const ROLE_KEYS: Record<string, string> = {
  chief: "settings.roles.chief",
  critic_a: "settings.roles.criticA",
  critic_b: "settings.roles.criticB",
  worker: "settings.roles.worker",
  planner: "settings.roles.planner",
  reporter: "settings.roles.reporter",
};
export function getRoleLabel(t: (k: string) => string, role: string): string {
  // v0.4.16: Rust emits "agent:chief" / "agent:critic:a" etc.
  // Strip the "agent:" prefix and normalize the critic separators
  // so ROLE_KEYS (which uses short names) matches.
  const short = role.startsWith("agent:") ? role.slice("agent:".length) : role;
  const normalized = short.replace(/:/g, "_"); // critic:a -> critic_a
  const key = ROLE_KEYS[normalized];
  return key ? t(key) : normalized || role;
}

// v0.4.16: render the thinking + context metadata inline next to
// each option so the user can pick the right model without
// leaving the page. Returns "" when the model has no metadata
// (e.g. live-catalog entries without metadata fields).
// v0.4.22 (event 000118, follow-up): even though every model row
// is pre-normalized in `refresh()`, this helper stays defensive
// — the only string ops it does are template interpolation
// (which is null/undefined-safe in JS), so there's no `.includes`
// to break, but we still type-guard `thinking_strength` so a
// malformed JSON value (`null`, `0`, `false`, a non-string) can
// never reach `${…}` and render as `null`.
export function modelBadge(m: AvailableModel): string {
  const parts: string[] = [];
  if (typeof m.thinking_strength === "string" && m.thinking_strength.length > 0) {
    parts.push(`[think: ${m.thinking_strength}]`);
  }
  if (typeof m.context_length === "number" && m.context_length > 0) {
    parts.push(`[${Math.round(m.context_length / 1000)}k]`);
  }
  return parts.length > 0 ? "· " + parts.join(" ") : "";
}

// v0.4.22 (event 000118): the live /v1/models path stores
// `display_name = id`, so the dropdown used to render e.g.
// `MiniMax · MiniMax-M3` with no human-readable hint about what
// the model actually is. This helper produces a label that:
//
//   1. Uses the curated display_name when it provides info beyond
//      the id (e.g. "MiniMax Text-01 (text, recommended)").
//   2. Falls back to a canonical friendly name for well-known
//      MiniMax-family ids when display_name is just the id.
//   3. Always appends the model id in muted monospace so the
//      user can read both the friendly name AND the id.
//
// Result: `MiniMax · MiniMax Text-01 (recommended)  ┄ MiniMax-Text-01`
//        `MiniMax · MiniMax M3 (reasoning)         ┄ MiniMax-M3`
//        `OpenAI · GPT-4o (recommended)            ┄ gpt-4o`
export function modelOptionLabel(m: AvailableModel): string {
  // v0.4.22 (event 000118, post-fix): guard against undefined
  // fields. Custom-model entries / live-cache rows that
  // omitted `display_name` previously threw
  // `Cannot read properties of undefined (reading 'includes')`
  // inside this helper. `safeStr` collapses every kind of
  // missing value to '' so the helper can never throw on a
  // malformed row.
  const id = safeStr(m.model);
  const displayName = safeStr(m.display_name) || id || "(unnamed model)";
  const provider = safeStr(m.provider_display) || safeStr(m.provider) || id || "(unnamed provider)";
  // If display_name was just the id (live path), swap in the
  // friendly label. Otherwise keep the curated display_name.
  const isUnannotated = !m.display_name || displayName === id;
  const name =
    (isUnannotated && id && MODEL_FRIENDLY[id]) || displayName || id || "(unnamed model)";
  // Hide the model id tail if display_name already contains it.
  const idTail = id && name.includes(id) ? "" : id ? `  ┄ ${id}` : "";
  const badge = modelBadge(m);
  return `${provider} · ${name}${idTail}${badge ? "  " + badge : ""}`;
}

// Companion to `modelOptionLabel` for compact surfaces (selected
// chain entries, tooltips). Strips the id tail and metadata badge.
export function modelShortLabel(m: AvailableModel): string {
  const id = safeStr(m.model);
  const displayName = safeStr(m.display_name) || id || "(unnamed model)";
  const provider = safeStr(m.provider_display) || safeStr(m.provider) || id || "(unnamed provider)";
  const isUnannotated = !m.display_name || displayName === id;
  const name =
    (isUnannotated && id && MODEL_FRIENDLY[id]) || displayName || id || "(unnamed model)";
  return `${provider} · ${name}`;
}
