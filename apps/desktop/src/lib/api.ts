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

export interface ProviderInfo {
  id: string;
  display_name: string;
  kind: string;
  base_url: string;
  api_key_env: string;
  enabled: boolean;
  key_present: boolean;
  is_local: boolean;
  notes: string;
  models: { id: string; display_name: string }[];
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

// ── Workflow ─────────────────────────────────────────────────────

export async function startWorkflow(text: string): Promise<{ id: string }> {
  return invoke('start_workflow_cmd', { text });
}

export async function getWorkflowPlan(id: string): Promise<Record<string, unknown>> {
  // This still goes through HTTP internally (Rust → Python)
  // But the frontend only sees invoke()
  return invoke('get_workflow', { id });
}

// ── Helpers ──────────────────────────────────────────────────────

function sleep(ms: number): Promise<void> {
  return new Promise((r) => setTimeout(r, ms));
}
