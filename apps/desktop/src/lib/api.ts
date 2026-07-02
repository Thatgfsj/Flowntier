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

// v0.4.19: resolve a role's default_model + keyring into a single
// preview so the ChatZone status line can show "未配置: 在设置里先选
// default_model" inline. Always returns ok:true/false + payload,
// never throws.
// v0.4.19: resolve a role's default_model + keyring into a single
// preview so the ChatZone status line can show "未配置: 在设置里先选
// default_model" inline. Always returns ok:true/false + payload,
// never throws.
export interface RoleResolveStatus {
  ok: boolean;
  role?: string;
  provider_short?: string;
  model_id?: string;
  base_url?: string;
  api_kind?: string;
  has_key?: boolean;
  fallback_chain?: string[];
  error?: string;
  // v0.4.20: per-(role, model) quota state. undefined when no
  // failure has been recorded (the common case). When present,
  // status ∈ "failed" | "pending_5h_wait" | "rate_limited".
  quota_status?: QuotaStatusEntry;
}
export async function getRoleResolveStatus(role: string): Promise<RoleResolveStatus> {
  return invoke<RoleResolveStatus>('get_role_resolve_status', { role });
}

// ── v0.4.20 quota tracker ─────────────────────────────────────────
// Each row in `quota_failures` represents a (role, model) pair that
// failed recently. The Settings → 角色额度状态 block lists every
// row; the ChatZone status line shows the row inline next to the
// model select.

export interface QuotaStatusEntry {
  /** "agent:chief" | "agent:worker" | "agent:planner" | ... */
  role_id: string;
  /** "minimax:MiniMax-Text-01" — provider:model identifier. */
  model_id: string;
  /** "failed" | "pending_5h_wait" | "rate_limited". */
  status: string;
  attempt_count: number;
  last_error_at: number;
  last_error_message: string;
  next_attempt_at: number | null;
}

export interface QuotaStatusResponse {
  ok: boolean;
  rows?: QuotaStatusEntry[];
  error?: string;
}

/** GET /api/quota/status. Returns all quota_failures rows. */
export async function getQuotaStatus(): Promise<QuotaStatusResponse> {
  return invoke<QuotaStatusResponse>('get_quota_status');
}

/** POST /api/quota/reset. Clears one (role, model) row. */
export async function resetQuota(
  role: string,
  model_id?: string,
): Promise<{ ok: boolean; cleared_rows?: number; error?: string }> {
  return invoke('reset_quota', { role, model_id });
}

/**
 * v0.4.20: convenience wrapper — fetch quota_status for one role via
 * the resolve endpoint (saves a round-trip when the caller already
 * needs the resolve result).
 */
export async function getRoleQuotaStatus(
  role: string,
): Promise<QuotaStatusEntry | null> {
  const r = await getRoleResolveStatus(role);
  return r.quota_status ?? null;
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

// v0.4.17: pipe-server now returns HTTP 200 with `{ok:false, error:...}`
// for every failure path (no API key, network error, parse error,
// non-2xx upstream). This is to keep the failure context inside the
// JSON-RPC body instead of throwing through pipe_request's
// non-2xx-to-Error converter (which would strip the structured
// error context).
export interface FetchedModelsResult {
  ok: boolean;
  models: ProviderModel[];
  count: number;
  error?: string;
  /** Provider id echoed back by the backend. */
  provider_id?: string;
  /** The URL the backend actually hit (for debugging). */
  url?: string;
  /** True if the response came from the hardcoded fallback catalog. */
  fallback?: boolean;
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

// ── v0.4.21 (event 000066): workspace + file tree ───────────────
// These wrap the pipe-server endpoints added by event 000066 so
// the chairman can see what files exist in the active workdir
// and so the runtime's filesystem context actually moves when
// the workdir changes (the underlying bug the chairman reported
// as "切工作目录不显示新文件").

export interface WorkspaceInfo {
  ok: boolean;
  root: string;
  name: string;
}

export async function getRuntimeWorkspace(): Promise<WorkspaceInfo> {
  return invoke<WorkspaceInfo>('get_runtime_workspace');
}

export interface FileTreeEntry {
  name: string;
  path: string;
  is_dir: boolean;
  is_file: boolean;
  size?: number;
  children?: FileTreeEntry[];
}

export interface FileTreeResponse {
  ok: boolean;
  root: string;
  path: string;
  entries: FileTreeEntry[];
  truncated: boolean;
  count: number;
}

export async function getWorkspaceTree(
  body: { path?: string; depth?: number; max_entries?: number } = {},
): Promise<FileTreeResponse> {
  return invoke<FileTreeResponse>('get_workspace_tree', { body });
}

export async function setRuntimeWorkspace(path: string): Promise<{ ok: boolean; root: string }> {
  // Re-use the existing `set_workdir_with_nwt` Tauri command
  // (which writes workdir.json + .nwt scaffolding AND now
  // notifies the pipe-server via /api/workspace/set — see
  // event 000066 patch in lib.rs:1280). Returns the absolute
  // .nwt path; for just-the-root the caller can call
  // getRuntimeWorkspace().
  const nwt = await invoke<string>('set_workdir_with_nwt', { path });
  return { ok: true, root: nwt };
}

// ── v0.4.21 (event 000066): recent errors (TopBar red badge) ────

export interface ErrorRecord {
  at: number;
  severity: 'error' | 'warn' | 'info';
  source: string;
  summary: string;
  detail?: string | null;
}

export interface RecentErrorsResponse {
  ok: boolean;
  count: number;
  rows: ErrorRecord[];
}

export async function getRecentErrors(limit = 10): Promise<RecentErrorsResponse> {
  return invoke<RecentErrorsResponse>('get_recent_errors', { body: { limit } });
}
