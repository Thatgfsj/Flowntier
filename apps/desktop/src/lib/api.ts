/**
 * ACO API — all calls go through Tauri invoke().
 * No HTTP, no CSP, no port issues.
 */

import { invoke } from '@tauri-apps/api/core';

// ── Health ───────────────────────────────────────────────────────

export async function checkHealth(): Promise<boolean> {
  try {
    return await invoke<boolean>('health_check');
  } catch {
    return false;
  }
}

export async function ensureConnected(maxRetries = 3): Promise<boolean> {
  for (let i = 0; i <= maxRetries; i++) {
    if (await checkHealth()) return true;
    if (i < maxRetries) await sleep(500 * Math.pow(2, i));
  }
  return false;
}

// ── Secrets ──────────────────────────────────────────────────────

export interface SecretInfo {
  name: string;
  present: boolean;
  masked: string | null;
}

export async function listSecrets(): Promise<SecretInfo[]> {
  return invoke<SecretInfo[]>('list_secrets');
}

export interface SaveSecretResult {
  saved: boolean;
  warning: string | null;
}

export async function saveSecret(name: string, value: string): Promise<SaveSecretResult> {
  return invoke<SaveSecretResult>('save_secret', { name, value });
}

export async function deleteSecret(name: string): Promise<void> {
  return invoke('delete_secret', { name });
}

export async function revealSecret(name: string): Promise<string> {
  return invoke<string>('reveal_secret', { name });
}

export async function seedSecrets(): Promise<string[]> {
  return invoke<string[]>('seed_secrets');
}

// ── Providers ────────────────────────────────────────────────────

// v0.4.15: renamed fields to match what pipe-server
// `list_providers` actually emits (handlers.rs:543-555). The
// old names (api_key_env, key_present, is_local, notes, models)
// were never produced by the server, so all reads were undefined
// — which silently filtered every preset out of the list,
// making the panel show "供应商（0）".
export interface ProviderInfo {
  id: string;
  display_name: string;
  api_kind: string;
  base_url: string;
  default_model: string;
  secret_name: string;
  has_secret: boolean;
  enabled: boolean;
  note: string;
  has_live_models_endpoint: boolean;
  // Emitted as [] by the server; UI must hit
  // discover_models or /api/providers/{id}/models to populate.
  models: { id: string; display_name: string }[];
  // Always false in current presets (no Ollama/LM Studio yet).
  // Kept in the type so future local providers don't require a
  // schema migration.
  is_local: boolean;
}

export async function listProviders(): Promise<{ providers: ProviderInfo[] }> {
  return invoke('list_providers');
}

export async function toggleProvider(id: string, enabled: boolean): Promise<void> {
  return invoke('toggle_provider', { id, enabled });
}

// ── Router ───────────────────────────────────────────────────────

export interface RoleInfo {
  role: string;
  default_model: string;
  fallback_chain: string[];
}

export async function listRouterRoles(): Promise<{ roles: RoleInfo[] }> {
  return invoke('list_router_roles');
}

export async function listRouterModels(): Promise<{ models: { provider: string; provider_display: string; model: string; display_name: string }[] }> {
  return invoke('list_router_models');
}

export async function updateRouterRoles(roles: RoleInfo[]): Promise<void> {
  return invoke('update_router_roles', { roles });
}

// ── Plugins ──────────────────────────────────────────────────────

export interface PluginDescriptor {
  name: string;
  description: string;
  actions: string[];
}

export async function listPlugins(): Promise<PluginDescriptor[]> {
  return invoke('list_plugins');
}

export async function invokePlugin(name: string, args: Record<string, unknown>): Promise<unknown> {
  return invoke('invoke_plugin', { name, args });
}

export interface ProviderModel {
  id: string;
  display_name: string;
  /** BUG-FRONTEND-RT-12 (event 000041): context window in
   *  tokens. The chairman's directive: let the user fill in
   *  this. Null/missing = use the runtime default. */
  context_length?: number | null;
  /** Reasoning effort. low=fast+cheap, medium=balanced,
   *  high=deep+expensive. Default 'medium'. */
  thinking_strength?: 'low' | 'medium' | 'high';
}

export interface FetchedModelsResult {
  ok: boolean;
  models: ProviderModel[];
  count: number;
  error?: string;
}

export async function fetchProviderModels(id: string): Promise<FetchedModelsResult> {
  return invoke<FetchedModelsResult>('fetch_provider_models', { id });
}

export interface CustomProviderSpec {
  id: string;
  display_name: string;
  kind: 'anthropic' | 'openai' | 'openai_compat';
  base_url: string;
  api_key_env: string;
  models: ProviderModel[];
}

export async function addCustomProvider(spec: CustomProviderSpec): Promise<ProviderInfo> {
  return invoke<ProviderInfo>('add_custom_provider', { ...spec });
}

export async function removeCustomProvider(id: string): Promise<{ ok: boolean }> {
  return invoke<{ ok: boolean }>('remove_custom_provider', { id });
}

// ── Workflow ─────────────────────────────────────────────────────

export async function startWorkflow(text: string): Promise<{ id: string }> {
  return invoke('start_workflow_cmd', { text });
}

export async function getWorkflowPlan(id: string): Promise<Record<string, unknown>> {
  // This still goes through HTTP internally (Rust → Python)
  // But the frontend only sees invoke()
  return invoke('get_workflow', { id });
}

// ── KV (Phase 4 onboarding state) ───────────────────────────────
// Generic key/value store backed by the SQLite `kv` table.
// Used for the first_run flag that gates the Welcome screen,
// the user's last-selected tab, etc. Always returns null
// if the key is unset (we use `null` as the "absent" sentinel).

export async function kvGet<T = unknown>(key: string): Promise<T | null> {
  try {
    const r = await invoke<{ k: string; v: T | null }>('kv_get', { key });
    return r.v;
  } catch (e) {
    console.warn('[api] kvGet failed:', key, e);
    return null;
  }
}

export async function kvSet<T = unknown>(key: string, value: T): Promise<void> {
  await invoke('kv_set', { key, value });
}

export async function resetOnboarding(): Promise<void> {
  // Set first_run=true so the Welcome screen re-appears on the
  // next launch. The user re-triggers via Settings → About.
  await kvSet('first_run', 'true');
}

// ── Sample workflow (Phase 4 onboarding) ───────────────────────

export interface SampleWorkflow {
  name: string;
  display_name: string;
  description: string;
  user_request: string;
  expected_tasks: string[];
}

export async function loadSampleWorkflow(): Promise<SampleWorkflow> {
  return invoke('load_sample_workflow');
}

// ── Helpers ──────────────────────────────────────────────────────

function sleep(ms: number): Promise<void> {
  return new Promise((r) => setTimeout(r, ms));
}
