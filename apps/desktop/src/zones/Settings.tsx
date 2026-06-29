import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { invoke } from '@tauri-apps/api/core';
import { Card } from '@flowntier/ui';
import {
  saveSecret,
  listProviders, toggleProvider,
  listRouterRoles, listRouterModels, updateRouterRoles,
  fetchProviderModels,
  addCustomProvider, removeCustomProvider,
  type ProviderInfo, type RoleInfo,
  type ProviderModel,
} from '../lib/api.js';
import { useCustomModels } from '../hooks/useCustomModels.js';
import { appVersion, buildSha } from '../lib/version.js';
import { SearchBugPanel } from '../components/SearchBugPanel.js';
import { tErr } from '../lib/errs.js';

// ── Quick Add AI ─────────────────────────────────────────────────

const QUICK_PROVIDERS = [
  { id: 'openai', name: 'OpenAI', envVar: 'OPENAI_API_KEY', placeholder: 'sk-...', description: 'GPT-5, GPT-5 Mini', color: '#10a37f' },
  { id: 'anthropic', name: 'Anthropic', envVar: 'ANTHROPIC_API_KEY', placeholder: 'sk-ant-...', description: 'Claude Opus, Sonnet, Haiku', color: '#d97706' },
  { id: 'google', name: 'Google Gemini', envVar: 'GOOGLE_API_KEY', placeholder: 'AIza...', description: 'Gemini 2.5 Pro, Flash', color: '#4285f4' },
  { id: 'deepseek', name: 'DeepSeek', envVar: 'DEEPSEEK_API_KEY', placeholder: 'sk-...', description: 'DeepSeek Chat, Reasoner', color: '#6366f1' },
  { id: 'minimax', name: 'MiniMax', envVar: 'MINIMAX_API_KEY', placeholder: 'eyJ...', description: 'MiniMax M3', color: '#f97316' },
  { id: 'kimi', name: 'Kimi (月之暗面)', envVar: 'MOONSHOT_API_KEY', placeholder: 'sk-...', description: 'Kimi K2', color: '#8b5cf6' },
  { id: 'zhipu', name: 'GLM (智谱)', envVar: 'ZHIPU_API_KEY', placeholder: '', description: 'GLM-4', color: '#059669' },
  { id: 'mimo', name: 'MIMO (小米)', envVar: 'MIMO_API_KEY', placeholder: 'sk-...', description: '小米 MIMO', color: '#ff6900' },
  { id: 'siliconflow', name: 'SiliconFlow', envVar: 'SILICONFLOW_API_KEY', placeholder: 'sk-...', description: 'SiliconFlow (硅基流动)', color: '#0ea5e9' },
];

function QuickAddAI({ onSaved }: { onSaved: () => void }) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [selected, setSelected] = useState<string | null>(null);
  const [apiKey, setApiKey] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState(false);

  const provider = QUICK_PROVIDERS.find((p) => p.id === selected);

  const handleSave = async () => {
    if (!provider || !apiKey.trim()) return;
    setBusy(true);
    setError(null);
    try {
      console.log('[QuickAddAI] saving:', provider.envVar);
      const result = await saveSecret(provider.envVar, apiKey.trim());
      console.log('[QuickAddAI] save result:', result);
      if (!result || !result.saved) {
        setError(t('settings.quickAdd.errorInvalidKey'));
        return;
      }
      if (result.warning) {
        // Key persisted, but seed to os.environ failed — non-fatal.
        console.warn('[QuickAddAI] seed warning:', result.warning);
      }
      setSuccess(true);
      setApiKey('');
      onSaved();
      setTimeout(() => {
        setOpen(false);
        setSuccess(false);
        setSelected(null);
      }, 1500);
    } catch (e) {
      console.error('[QuickAddAI] save failed:', e);
      setError(tErr(t, e, 'settings.error.saveFailed'));
    } finally {
      setBusy(false);
    }
  };

  if (!open) {
    return (
      <button
        type="button"
        onClick={() => setOpen(true)}
        className="flex w-full items-center justify-center gap-2 rounded-lg border border-chief/30 bg-chief/10 px-4 py-2.5 text-sm font-medium text-chief transition-colors hover:bg-chief/20"
      >
        <span className="text-lg">+</span>
        {t('settings.providers.addAI')}
      </button>
    );
  }

  return (
    <div className="rounded-lg border border-border bg-surface-1 p-4">
      <div className="mb-3 flex items-center justify-between">
        <h3 className="text-sm font-semibold text-primary">{t('settings.quickAdd.title')}</h3>
        <button type="button" onClick={() => { setOpen(false); setSelected(null); setApiKey(''); setError(null); setSuccess(false); }} className="text-xs text-text-secondary hover:text-primary">{t('settings.action.cancel')}</button>
      </div>

      <div className="mb-3 max-h-[300px] overflow-y-auto">
        <div className="grid grid-cols-2 gap-2">
          {QUICK_PROVIDERS.map((p) => (
            <button
              key={p.id}
              type="button"
              onClick={() => { setSelected(p.id); setApiKey(''); setError(null); }}
              className={`flex items-center gap-2 rounded-md border p-2.5 text-left transition-colors ${selected === p.id ? 'border-chief bg-surface-2' : 'border-border bg-surface-1 hover:border-text-secondary'}`}
            >
              <div className="h-3 w-3 shrink-0 rounded-full" style={{ backgroundColor: p.color }} />
              <div className="min-w-0">
                <div className="text-xs font-medium">{p.name}</div>
                <div className="truncate text-[10px] text-text-secondary">{p.description}</div>
              </div>
            </button>
          ))}
        </div>
      </div>

      {provider && (
        <div className="space-y-2">
          <label className="block text-xs text-text-secondary">
            {provider.name} {t('settings.custom.apiKeyLabel')}
            <span className="ml-1 text-[10px] text-text-secondary">→ {provider.envVar}</span>
          </label>
          <input
            type="password"
            value={apiKey}
            onChange={(e) => setApiKey(e.target.value)}
            placeholder={provider.placeholder}
            className="w-full rounded border border-border bg-surface-2 px-3 py-2 font-mono text-sm placeholder:text-text-secondary focus:border-chief focus:outline-none focus:ring-2 focus:ring-chief/50"
            onKeyDown={(e) => { if (e.key === 'Enter' && apiKey.trim()) void handleSave(); }}
          />
          {error && <p role="alert" aria-live="polite" className="text-xs text-status-failed">{error}</p>}
          {success && <p role="status" aria-live="polite" className="text-xs text-status-done">{t('settings.quickAdd.saved')}</p>}
          <button
            type="button"
            onClick={handleSave}
            disabled={busy || !apiKey.trim()}
            className="w-full rounded bg-chief px-3 py-2 text-sm font-medium text-white hover:bg-chief/90 disabled:opacity-50"
          >
            {busy ? t('settings.action.save') : t('settings.secrets.saveKeyFor', { provider: provider.name })}
          </button>
        </div>
      )}
    </div>
  );
}

// ── Main Settings ────────────────────────────────────────────────

interface RuntimeSnapshot {
  providers: ProviderInfo[];
  roles: RoleInfo[];
  available_models: { provider: string; provider_display: string; model: string; display_name: string }[];
}

const EMPTY: RuntimeSnapshot = { providers: [], roles: [], available_models: [] };

// ROLE_LABELS is module-scope (no React hook access). Use
// getRoleLabel(t, role) below to look up the localized string.
const ROLE_KEYS: Record<string, string> = {
  chief: 'settings.roles.chief',
  critic_a: 'settings.roles.criticA',
  critic_b: 'settings.roles.criticB',
  worker: 'settings.roles.worker',
  planner: 'settings.roles.planner',
  reporter: 'settings.roles.reporter',
};
function getRoleLabel(t: (k: string) => string, role: string): string {
  // v0.4.16: Rust emits "agent:chief" / "agent:critic:a" etc.
  // Strip the "agent:" prefix and normalize the critic separators
  // so ROLE_KEYS (which uses short names) matches.
  const short = role.startsWith('agent:') ? role.slice('agent:'.length) : role;
  const normalized = short.replace(/:/g, '_'); // critic:a -> critic_a
  const key = ROLE_KEYS[normalized];
  return key ? t(key) : (normalized || role);
}

export interface SettingsProps {
  open: boolean;
  onClose: () => void;
  /** Current workdir, if set. Used by the About card's
   *  "Change workdir" UI (BUG-018 fix). */
  workdir?: string | null;
}

export function Settings({ open, onClose, workdir }: SettingsProps) {
  const { t } = useTranslation();
  const [snapshot, setSnapshot] = useState<RuntimeSnapshot>(EMPTY);
  const [selected, setSelected] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [savedAt, setSavedAt] = useState<string | null>(null);
  // BUG-FRONTEND-RT-6 (event 000038): double-confirmation flow
  // for destructive "Clear local data" — the user must type a
  // specific phrase before the destructive button activates.
  const [deleteDialogOpen, setDeleteDialogOpen] = useState(false);
  const [deletePhrase, setDeletePhrase] = useState('');
  const [deleteBusy, setDeleteBusy] = useState(false);
  const customModels = useCustomModels();

  // BUG-FRONTEND-RT-6 (event 000038): the destructive "Clear
  // local data" action. The dialog has its own phrase check;
  // this function only runs after the user has typed the
  // expected phrase. We do a final defence-in-depth check here
  // in case the function is ever called from a different code
  // path (e.g. a refactor) that bypasses the dialog.
  const confirmWipe = async () => {
    if (deletePhrase !== t('settings.about.deletePhrase')) {
      console.warn('[Settings] wipe blocked: phrase mismatch');
      return;
    }
    setDeleteBusy(true);
    try {
      await invoke('wipe_all_data');
      setDeleteDialogOpen(false);
      setDeletePhrase('');
      // Reload the page so the React state re-initializes from
      // an empty data dir. The Welcome screen will appear next
      // launch (kv.first_run is null => first run path).
      setTimeout(() => window.location.reload(), 100);
    } catch (e) {
      alert(t('settings.about.clearDataError', { error: String(e) }));
    } finally {
      setDeleteBusy(false);
    }
  };
  // Best-effort: ask the Rust side for the resolved data dir
  // (so the About card can show "Logs are at..."). Falls back to
  // a friendly placeholder if the runtime is offline.
  const [dataDir, setDataDir] = useState<string>('');
  const [logDir, setLogDir] = useState<string>('');

  const refresh = async () => {
    try {
      const [prov, roles, models] = await Promise.all([
        listProviders(),
        listRouterRoles(),
        listRouterModels(),
      ]);
      console.log('[Settings] refresh:', { prov, roles, models });
      if (prov && roles && models) {
        // Merge the user-curated custom models per provider into the
        // model picker so newly-released models (e.g. DeepSeek r2)
        // are selectable without us shipping a preset update.
        const enabledIds = new Set(
          prov.providers.filter((p) => p.enabled).map((p) => p.id),
        );
        const displayNameById = new Map(
          prov.providers.map((p) => [p.id, p.display_name]),
        );
        const userModels: typeof models.models = [];
        for (const pid of enabledIds) {
          for (const m of customModels.getForProvider(pid)) {
            userModels.push({
              provider: pid,
              provider_display: displayNameById.get(pid) ?? pid,
              model: m.id,
              display_name: m.display_name,
            });
          }
        }
        const merged = [...models.models];
        for (const um of userModels) {
          if (!merged.some((m) => m.provider === um.provider && m.model === um.model)) {
            merged.push(um);
          }
        }
        setSnapshot({ providers: prov.providers, roles: roles.roles, available_models: merged });
        setSavedAt(new Date().toLocaleTimeString());
      }
    } catch (e) {
      console.error('[Settings] refresh failed:', e);
    }
  };

  // Re-merge when custom models change (after add/remove in the modal)
  // so the model picker updates without a full server refresh.
  useEffect(() => {
    if (snapshot.providers.length > 0) void refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [customModels.totalCount]);

  useEffect(() => {
    if (!open) return;
    void refresh();
  }, [open]);
  useEffect(() => {
    // v0.4.12 (event 000048): replaced the UA-sniff hack with
    // a real call to get_diagnostics. This returns both the
    // data_dir and log_dir that the running process is actually
    // using — not a platform-default guess.
    let cancelled = false;
    void (async () => {
      try {
        const diag = (await invoke('get_diagnostics')) as {
          data_dir?: string | null;
          log_dir?: string | null;
        };
        if (cancelled) return;
        if (diag.data_dir && diag.data_dir !== '<unknown>') {
          setDataDir(diag.data_dir);
        }
        if (diag.log_dir) {
          setLogDir(diag.log_dir);
        }
      } catch (e) {
        console.warn('[Settings] get_diagnostics failed:', e);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const toggle = async (id: string, enabled: boolean) => {
    setSaving(true);
    try {
      await toggleProvider(id, enabled);
      setSnapshot((prev) => ({
        ...prev,
        providers: prev.providers.map((p) => (p.id === id ? { ...p, enabled } : p)),
      }));
      setSavedAt(new Date().toLocaleTimeString());
    } catch (e) {
      console.error('[Settings] toggle failed:', e);
      setSnapshot((prev) => ({
        ...prev,
        // Revert the optimistic UI update on failure.
        providers: prev.providers.map((p) => (p.id === id ? { ...p, enabled: !enabled } : p)),
      }));
    } finally {
      setSaving(false);
    }
  };

  const setRoleDefault = async (role: string, model: string) => {
    setSaving(true);
    try {
      const newRoles = snapshot.roles.map((r) => (r.role === role ? { ...r, default_model: model } : r));
      await updateRouterRoles(newRoles);
      setSnapshot((prev) => ({ ...prev, roles: newRoles }));
      setSavedAt(new Date().toLocaleTimeString());
    } finally {
      setSaving(false);
    }
  };

  // Add or remove a fallback for a single role. The full roles array
  // is sent so a single round-trip commits the new state — per-role
  // patching would require a new server endpoint.
  const setRoleFallback = async (role: string, chain: string[]) => {
    setSaving(true);
    try {
      const newRoles = snapshot.roles.map((r) => (r.role === role ? { ...r, fallback_chain: chain } : r));
      await updateRouterRoles(newRoles);
      setSnapshot((prev) => ({ ...prev, roles: newRoles }));
      setSavedAt(new Date().toLocaleTimeString());
    } finally {
      setSaving(false);
    }
  };

  if (!open) return null;

  const sel = snapshot.providers.find((p) => p.id === selected);

  return (
    <div className="fixed inset-0 z-50 flex bg-black/60 backdrop-blur-sm" role="dialog" aria-modal="true" aria-label={t('topbar.settings')} onClick={onClose}>
      <div className="ml-auto flex h-full w-[1100px] max-w-[95vw] flex-col border-l border-border bg-surface-1 shadow-2xl" onClick={(e) => e.stopPropagation()}>
        <header className="flex h-14 shrink-0 items-center justify-between border-b border-border bg-surface-2 px-5">
          <div>
            <h2 className="text-base font-semibold text-primary">{t('topbar.settings')}</h2>
            <p className="text-xs text-text-secondary">{t('settings.headerSubtitle')}</p>
          </div>
          <div className="flex items-center gap-3">
            {/* BUG-FRONTEND-4 (audit 000026 #54): the previous
                code rendered both "Saving..." and "Saved · HH:MM:SS"
                simultaneously when saving completes, producing
                a confusing visual. Now they are mutually exclusive
                via an if/else. */}
            {saving ? (
              <span className="text-xs text-text-secondary">{t('settings.action.save')}</span>
            ) : savedAt !== null ? (
              <span className="text-xs text-text-secondary">{t('settings.action.savedAt', {time: savedAt})}</span>
            ) : null}
            <button type="button" onClick={onClose} className="rounded-md border border-border bg-surface-1 px-3 py-1.5 text-xs text-text-secondary hover:text-primary">{t('settings.action.close')}</button>
          </div>
        </header>

        <div className="flex flex-1 overflow-hidden">
          <aside className="w-[380px] shrink-0 overflow-y-auto border-r border-border bg-surface-2 p-3">
            <div className="mb-3 flex flex-col gap-2">
                <QuickAddAI onSaved={() => void refresh()} />
                <CustomProviderForm onSaved={() => void refresh()} />
              </div>
                <h3 className="mb-2 px-1 text-xs font-semibold uppercase tracking-wide text-text-secondary">
                  {t('settings.providers.titleWithCount', { count: snapshot.providers.filter((p) => p.has_secret || p.is_local).length })}
                </h3>
                <div className="flex flex-col gap-2">
                  {snapshot.providers
                    .filter((p) => p.has_secret || p.is_local)
                    .map((p) => {
                    const isSel = p.id === selected;
                    // v0.4.15: was `p.notes.includes` — but Rust
                    // emits `note` (singular). The old key was
                    // undefined, so custom-provider rows never
                    // showed the ✕ delete button.
                    const isCustom = p.note.includes('Custom relay');
                    const usable = p.has_secret || p.is_local;
                    const handleDeleteCustom = async (e: React.MouseEvent) => {
                      e.stopPropagation();
                      if (!confirm(t('settings.confirm.deleteCustom.title', {name: p.display_name}))) return;
                      try {
                        await removeCustomProvider(p.id);
                        if (selected === p.id) setSelected(null);
                        refresh();
                      } catch (err) {
                        alert(err instanceof Error ? err.message : t('settings.error.deleteCustomFailed'));
                      }
                    };
                    return (
                      <button key={p.id} type="button" onClick={() => setSelected(p.id)}
                        className={`flex flex-col items-start gap-1 rounded-md border p-2 text-left transition-colors ${isSel ? 'border-chief bg-surface-1' : 'border-border bg-surface-1 hover:border-text-secondary'} ${!usable ? 'opacity-60' : ''}`}>
                        <div className="flex w-full items-center justify-between">
                          <div className="flex items-center gap-2">
                            <span className="font-medium text-sm">{p.display_name}</span>
                            {p.is_local && <span className="rounded bg-surface-3 px-1.5 py-0.5 text-[10px] text-text-secondary">{t('settings.models.local')}</span>}
                            {isCustom && <span className="rounded bg-chief/20 px-1.5 py-0.5 text-[10px] text-chief">{t('settings.custom.kindLabel')}</span>}
                          </div>
                          <div className="flex items-center gap-1.5">
                            <Toggle on={p.enabled} onChange={(v) => void toggle(p.id, v)} disabled={!usable} />
                            {isCustom && (
                              <button type="button" onClick={handleDeleteCustom} title={String(t('settings.action.deleteCustom'))} className="rounded p-0.5 text-[10px] text-red-400 hover:bg-red-400/10 hover:text-red-300">✕</button>
                            )}
                          </div>
                        </div>
                        {/* v0.4.15: server emits default_model and a
                            hardcoded empty models array; we surface the
                            default model name (and the env var it reads)
                            so the row isn't empty. The "↻ discover"
                            button lives in the detail panel to keep this
                            row compact. */}
                        <div className="text-[11px] text-text-secondary">
                          {t('settings.quickAdd.modelCount', {count: p.models.length})}
                        </div>
                        <KeyBadge present={p.has_secret} />
                        {!p.has_secret && (
                          <div className="text-[10px] text-chief">{t('settings.quickAdd.addKeyHint')}</div>
                        )}
                      </button>
                    );
                  })}
                </div>
              </aside>

              <main className="flex-1 overflow-y-auto p-5">
                {sel && (
                  <Card className="mb-4">
                    <h3 className="mb-1 text-sm font-semibold">{sel.display_name}</h3>
                    <p className="mb-3 text-xs text-text-secondary">{sel.note}</p>
                    <div className="grid grid-cols-2 gap-3 text-xs">
                      <Field label={t('settings.custom.kindLabel')}>{sel.api_kind}</Field>
                      <Field label={t('settings.custom.baseUrlLabel')}><code className="font-mono">{sel.base_url}</code></Field>
                      <Field label={t('settings.custom.apiKeyLabel')}><code className="font-mono">{sel.secret_name}</code></Field>
                      <Field label={t('settings.field.keyConfigured')}>
                        {sel.has_secret ? (
                          <span className="text-status-done">{t('settings.field.keyYes')}</span>
                        ) : (
                          <span className="text-status-warn">
                            {t('settings.field.keyNo', { env: sel.secret_name })}
                          </span>
                        )}
                      </Field>
                    </div>
                    {sel.models.length > 0 && (
                      <div className="mt-3">
                        <h4 className="mb-1 text-xs font-semibold uppercase tracking-wide text-text-secondary">{t('settings.models.available')}</h4>
                        <ul className="grid grid-cols-2 gap-1 text-xs">
                          {sel.models.map((m) => (
                            <li key={m.id} className="rounded bg-surface-2 px-2 py-1 font-mono">
                              {m.display_name} <span className="ml-1 text-text-secondary">({m.id})</span>
                            </li>
                          ))}
                        </ul>
                      </div>
                    )}

                    <ProviderModelManager
                      providerId={sel.id}
                      providerDisplay={sel.display_name}
                      customModels={customModels.getForProvider(sel.id)}
                      onAdd={(models) => customModels.addMany(sel.id, models)}
                      onRemove={(modelId) => customModels.remove(sel.id, modelId)}
                      onClear={() => customModels.clear(sel.id)}
                    />
                  </Card>
                )}

                <Card className="mt-4">
                  <h3 className="mb-1 text-sm font-semibold">{t('settings.about.title')}</h3>
                  <p className="mb-3 text-xs text-text-secondary">
                    {t('settings.about.version', {
                      version: appVersion,
                      build: buildSha ? ' (' + buildSha + ')' : ''
                    })}
                  </p>
                  <p className="mb-2 text-xs text-text-secondary">
                    {dataDir}
                  </p>
                  {logDir && (
                    <>
                      <p className="mb-1 break-all text-[10px] text-text-secondary">
                        {t('settings.about.logDir')}: {logDir}
                      </p>
                      <p className="mb-3 text-[10px] text-text-tertiary">
                        {t('settings.about.logDirHint')}
                      </p>
                    </>
                  )}
                  {/* BUG-018 fix (event 000024): Settings → About
                      now exposes a "Change workdir" button that
                      calls `set_workdir_with_nwt` (atomic) with a
                      new path. Previously this code path didn't
                      exist; users who picked a workdir on first
                      launch were stuck with it. */}
                  <button
                    type="button"
                    onClick={async () => {
                      // Use the Tauri dialog plugin to pick a
                      // directory. Falls back to a manual text
                      // prompt if the plugin is unavailable.
                      let newPath: string | null = null;
                      try {
                        const { open } = await import('@tauri-apps/plugin-dialog');
                        newPath = await open({ directory: true, multiple: false });
                      } catch (e) {
                        console.warn('[Settings] dialog plugin unavailable:', e);
                      }
                      if (!newPath) {
                        const input = window.prompt(t('settings.about.changeWorkdirPrompt'));
                        if (!input) return;
                        newPath = input.trim();
                      }
                      if (!newPath) return;
                      try {
                        await invoke('set_workdir_with_nwt', { path: newPath });
                        // Reload to re-fetch workdir state in App.tsx.
                        setTimeout(() => window.location.reload(), 100);
                      } catch (e) {
                        alert(t('settings.about.changeWorkdirError', { error: String(e) }));
                      }
                    }}
                    className="mb-2 rounded-md border border-border bg-surface-2 px-3 py-2 text-xs text-text-primary hover:bg-surface-3"
                  >
                    {t('settings.about.changeWorkdir')}
                  </button>
                  {workdir && (
                    <p className="mb-3 break-all text-[10px] text-text-secondary">
                      {t('settings.about.currentWorkdir')}: {workdir}
                    </p>
                  )}
                  <button
                    type="button"
                    onClick={async () => {
                      // BUG-FRONTEND-RT-6 (event 000038): a
                      // second confirmation is now required —
                      // the user has to type a specific phrase
                      // ("DELETE" in en-US / "删除" in zh-CN)
                      // to prevent accidental data loss. The
                      // destructive button stays disabled until
                      // the input matches.
                      setDeletePhrase('');
                      setDeleteDialogOpen(true);
                    }}
                    className="rounded-md border border-status-failed/40 bg-status-failed/10 px-3 py-2 text-xs text-status-failed hover:bg-status-failed/20"
                  >
                    {t('settings.about.clearData')}
                  </button>
                </Card>
                <SearchBugPanel />

                {/* v0.4.12 (event 000048): RoleAssignmentCard moved
                    here (was above the About card). Keeps the
                    "Search bug" UI immediately above the role
                    assignments, since both are read-mostly
                    diagnostic / configuration views. */}
                <h3 className="mb-2 mt-4 px-1 text-xs font-semibold uppercase tracking-wide text-text-secondary">{t('settings.roles.title')}</h3>
                <p className="mb-2 px-1 text-[10px] text-text-secondary">

                </p>
                <div className="flex flex-col gap-2">
                  {snapshot.roles.map((r) => (
                    <RoleAssignmentCard
                      key={r.role}
                      role={r}
                      availableModels={snapshot.available_models}
                      saving={saving}
                      onDefaultChange={(model) => void setRoleDefault(r.role, model)}
                      onFallbackChange={(chain) => void setRoleFallback(r.role, chain)}
                    />
                  ))}
                </div>
              </main>

      {/* BUG-FRONTEND-RT-6 (event 000038): double-confirmation
          modal for destructive "Clear local data". The user must
          type the specific phrase (`DELETE` in en-US, `删除` in
          zh-CN) to activate the destructive button. Prevents
          accidental data loss from a stray click. The phrase is
          localized via `settings.about.deletePhrase`. */}
      {deleteDialogOpen && (
        <div
          role="dialog"
          aria-modal="true"
          aria-labelledby="delete-dialog-title"
          className="fixed inset-0 z-[60] flex items-center justify-center bg-black/60 backdrop-blur-sm"
          onClick={(e) => {
            if (e.target === e.currentTarget) setDeleteDialogOpen(false);
          }}
        >
          <div className="w-[420px] max-w-[90vw] rounded-lg border border-status-failed/40 bg-surface-1 p-5 shadow-xl">
            <h2 id="delete-dialog-title" className="text-base font-semibold text-status-failed">
              {t('settings.about.clearData')}
            </h2>
            <p className="mt-2 text-xs text-text-secondary">
              {t('settings.about.clearDataConfirmBody')}
            </p>
            <p className="mt-3 text-xs text-text-secondary">
              {t('settings.about.deletePhraseHint', { phrase: t('settings.about.deletePhrase') })}
            </p>
            <input
              type="text"
              value={deletePhrase}
              onChange={(e) => setDeletePhrase(e.target.value)}
              placeholder={t('settings.about.deletePhrase')}
              autoFocus
              onKeyDown={(e) => {
                if (e.key === 'Enter' && deletePhrase === t('settings.about.deletePhrase')) {
                  confirmWipe();
                } else if (e.key === 'Escape') {
                  setDeleteDialogOpen(false);
                }
              }}
              className="mt-2 w-full rounded border border-border bg-surface-2 px-2 py-1.5 font-mono text-sm focus:border-status-failed focus:outline-none"
            />
            <div className="mt-4 flex items-center justify-end gap-2">
              <button
                type="button"
                onClick={() => setDeleteDialogOpen(false)}
                className="rounded-md border border-border bg-surface-2 px-3 py-1.5 text-xs text-text-secondary hover:text-primary"
              >
                {t('settings.action.close')}
              </button>
              <button
                type="button"
                disabled={deleteBusy || deletePhrase !== t('settings.about.deletePhrase')}
                onClick={confirmWipe}
                className="rounded-md border border-status-failed/40 bg-status-failed px-3 py-1.5 text-xs font-medium text-white hover:bg-status-failed/90 disabled:cursor-not-allowed disabled:opacity-40"
              >
                {deleteBusy ? t('settings.action.save') : t('settings.about.clearData')}
              </button>
            </div>
          </div>
        </div>
      )}
        </div>
      </div>
    </div>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div>
      <div className="mb-0.5 text-text-secondary">{label}</div>
      <div className="font-mono text-primary">{children}</div>
    </div>
  );
}

function Toggle({ on, onChange, disabled }: { on: boolean; onChange: (v: boolean) => void; disabled?: boolean }) {
  const { t } = useTranslation();
  return (
    <button type="button" onClick={(e) => { e.stopPropagation(); onChange(!on); }} disabled={disabled}
      className={`relative h-5 w-9 rounded-full transition-colors focus:outline-none focus:ring-2 focus:ring-chief/50 ${on ? 'bg-chief' : 'bg-surface-3'} ${disabled ? 'opacity-40 cursor-not-allowed' : ''}`}
      aria-pressed={on} aria-label={t('settings.providers.enabled')}>
      <span className={`absolute top-0.5 h-4 w-4 rounded-full bg-white transition-all ${on ? 'left-4' : 'left-0.5'}`} />
    </button>
  );
}

function KeyBadge({ present }: { present: boolean }) {
  return present ? (
    <span className="rounded bg-status-done/20 px-1.5 py-0.5 text-[10px] text-status-done">Key ✓</span>
  ) : (
    <span className="rounded bg-status-warn/20 px-1.5 py-0.5 text-[10px] text-status-warn">Key ✗</span>
  );
}

// ── Per-role assignment card ──────────────────────────────────────

interface AvailableModel {
  provider: string;
  provider_display: string;
  model: string;
  display_name: string;
  thinking_strength?: string;
  context_length?: number | null;
}

// v0.4.16: render the thinking + context metadata inline next to
// each option so the user can pick the right model without
// leaving the page. Returns "" when the model has no metadata
// (e.g. live-catalog entries without metadata fields).
function modelBadge(m: AvailableModel): string {
  const parts: string[] = [];
  if (m.thinking_strength) {
    parts.push(`[think: ${m.thinking_strength}]`);
  }
  if (typeof m.context_length === 'number' && m.context_length > 0) {
    parts.push(`[${Math.round(m.context_length / 1000)}k]`);
  }
  return parts.length > 0 ? '· ' + parts.join(' ') : '';
}

interface RoleAssignmentCardProps {
  role: import('../lib/api.js').RoleInfo;
  availableModels: AvailableModel[];
  saving: boolean;
  onDefaultChange: (model: string) => void;
  onFallbackChange: (chain: string[]) => void;
}

/**
 * Per-role card: one default model + an editable ordered fallback
 * chain. Mirrors cc-switch's provider-card-with-failover pattern but
 * scoped to agent roles (chief / critic / worker) rather than to
 * external CLI apps.
 *
 * Saving is a single round-trip: the full `roles` array is sent to
 * `update_router_roles`, so adding/removing/reordering all commit in
 * one backend call.
 */
function RoleAssignmentCard({
  role,
  availableModels,
  saving,
  onDefaultChange,
  onFallbackChange,
}: RoleAssignmentCardProps) {
  const { t } = useTranslation();
  const inChain = new Set(role.fallback_chain);

  // Filter out the default and the existing chain entries from the
  // "add fallback" dropdown so we don't offer the user nonsense options.
  const addable = availableModels.filter((m) => {
    const ref = `${m.provider}:${m.model}`;
    return ref !== role.default_model && !inChain.has(ref);
  });

  const move = (idx: number, dir: -1 | 1) => {
    const next = [...role.fallback_chain];
    const j = idx + dir;
    if (j < 0 || j >= next.length) return;
    const tmp = next[idx] as string;
    next[idx] = next[j] as string;
    next[j] = tmp;
    onFallbackChange(next);
  };

  const removeAt = (idx: number) => {
    onFallbackChange(role.fallback_chain.filter((_, i) => i !== idx));
  };

  const addModel = (ref: string) => {
    if (ref === role.default_model || inChain.has(ref)) return;
    onFallbackChange([...role.fallback_chain, ref]);
  };

  return (
    <Card className="!p-3">
      <div className="flex items-center gap-3">
        <div className="w-20 shrink-0">
          <div className="text-sm font-medium">{getRoleLabel(t, role.role)}</div>
          <div className="font-mono text-[10px] text-text-secondary">{role.role}</div>
        </div>
        <div className="flex-1">
          <div className="mb-1 text-[10px] uppercase tracking-wide text-text-secondary">{t('settings.models.default')}</div>
          <select
            value={role.default_model}
            onChange={(e) => onDefaultChange(e.target.value)}
            disabled={saving}
            className="w-full rounded-md border border-border bg-surface-1 px-2 py-1.5 text-xs focus:border-chief focus:outline-none disabled:opacity-50"
          >
            {availableModels.length === 0 ? (
              <option value={role.default_model}>{t('settings.models.emptyOption')}</option>
            ) : (
              availableModels.map((m) => {
                const ref = `${m.provider}:${m.model}`;
                return (
                  <option key={ref} value={ref}>
                    {m.provider_display} · {m.display_name}  {modelBadge(m)}
                  </option>
                );
              })
            )}
          </select>
        </div>
      </div>

      <div className="mt-3 border-t border-border pt-3">
        <div className="mb-1 flex items-center justify-between">
          <div className="text-[10px] uppercase tracking-wide text-text-secondary">
            {t('settings.models.fallbackChain', {count: role.fallback_chain.length})}
          </div>
          {addable.length > 0 && (
            <select
              defaultValue=""
              onChange={(e) => {
                if (e.target.value) {
                  addModel(e.target.value);
                  e.target.value = "";
                }
              }}
              disabled={saving}
              className="rounded border border-border bg-surface-1 px-1.5 py-0.5 text-[11px] focus:border-chief focus:outline-none disabled:opacity-50"
            >
              <option value="">+ 添加回退</option>
              {addable.map((m) => {
                const ref = `${m.provider}:${m.model}`;
                return (
                  <option key={ref} value={ref}>
                    {m.provider_display} · {m.display_name}  {modelBadge(m)}
                  </option>
                );
              })}
            </select>
          )}
        </div>

        {role.fallback_chain.length === 0 ? (
          <div className="text-[11px] text-text-secondary">{t('settings.models.emptyFallback')}</div>
        ) : (
          <ol className="space-y-1">
            {role.fallback_chain.map((ref, idx) => {
              const m = availableModels.find((x) => `${x.provider}:${x.model}` === ref);
              const label = m ? `${m.provider_display} · ${m.display_name}` : ref;
              return (
                <li
                  key={`${ref}-${idx}`}
                  className="flex items-center gap-2 rounded border border-border bg-surface-1 px-2 py-1"
                >
                  <span className="w-6 shrink-0 text-center text-[10px] font-mono text-text-secondary">
                    {idx + 1}
                  </span>
                  <span className="flex-1 truncate font-mono text-[11px]" title={ref}>
                    {label}
                  </span>
                  <button
                    type="button"
                    onClick={() => move(idx, -1)}
                    disabled={idx === 0 || saving}
                    className="rounded px-1.5 py-0.5 text-[10px] text-text-secondary hover:bg-surface-2 hover:text-primary disabled:opacity-30"
                    aria-label={t('settings.action.moveUp')}
                  >
                    ↑
                  </button>
                  <button
                    type="button"
                    onClick={() => move(idx, 1)}
                    disabled={idx === role.fallback_chain.length - 1 || saving}
                    className="rounded px-1.5 py-0.5 text-[10px] text-text-secondary hover:bg-surface-2 hover:text-primary disabled:opacity-30"
                    aria-label={t('settings.action.moveDown')}
                  >
                    ↓
                  </button>
                  <button
                    type="button"
                    onClick={() => removeAt(idx)}
                    disabled={saving}
                    className="rounded px-1.5 py-0.5 text-[10px] text-text-secondary hover:bg-status-failed/20 hover:text-status-failed disabled:opacity-30"
                    aria-label={t('settings.action.delete')}
                  >
                    ×
                  </button>
                </li>
              );
            })}
          </ol>
        )}
      </div>
    </Card>
  );
}

// ── Provider model manager (cc-switch style) ─────────────────────

interface ProviderModelManagerProps {
  providerId: string;
  providerDisplay: string;
  customModels: ProviderModel[];
  onAdd: (models: ProviderModel[]) => void;
  onRemove: (modelId: string) => void;
  onClear: () => void;
}

/**
 * Pulls a live model list from the provider's own API and lets the
 * user pick which ones to add to their curated set. Mirrors
 * cc-switch's per-provider model-fetch flow but stores the user's
 * selection in localStorage so it survives restarts.
 *
 * The Tauri shell forwards to the Python `GET
 * /api/providers/{id}/models` endpoint, which dispatches the right
 * per-provider request (Anthropic: /v1/models with x-api-key, Gemini:
 * /v1beta/models?key=, OpenAI-compat: /models, Ollama: /api/tags,
 * LM Studio: /models).
 */
function ProviderModelManager({
  providerId,
  providerDisplay,
  customModels,
  onAdd,
  onRemove,
  onClear,
}: ProviderModelManagerProps) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [fetched, setFetched] = useState<ProviderModel[]>([]);
  const [picked, setPicked] = useState<Set<string>>(new Set());

  const pull = async () => {
    setBusy(true);
    setError(null);
    setFetched([]);
    setPicked(new Set());
    setOpen(true);
    try {
      const res = await fetchProviderModels(providerId);
      if (!res.ok) {
        // v0.4.16: surface the real backend error verbatim. The
        // previous behaviour swallowed it behind a generic
        // "拉取失败" string and chairman couldn't tell whether
        // it was a 401, a network error, or a non-OpenAI
        // response shape. We still keep the i18n key as a
        // fallback if for some reason backend returns no error.
        const backendErr = res.error ?? t('settings.models.pullError');
        setError(backendErr);
        try {
          // Console-log the structured info so the user can
          // copy-paste it into a bug report.
          console.warn('[Flowntier] pull models failed', res);
        } catch {/* ignore */}
      } else {
        setFetched(res.models);
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const togglePick = (id: string) => {
    setPicked((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const commit = () => {
    const toAdd = fetched.filter((m) => picked.has(m.id));
    if (toAdd.length > 0) onAdd(toAdd);
    setOpen(false);
  };

  // Models the user has already added to their curated set (so the
  // fetched list can show "✓ 已添加" badges).
  const alreadyHave = new Set(customModels.map((m) => m.id));

  return (
    <div className="mt-3 rounded-md border border-border bg-surface-2 p-3">
      <div className="mb-2 flex items-center justify-between">
        <h4 className="text-xs font-semibold uppercase tracking-wide text-text-secondary">
          {t('settings.models.customModels', {count: customModels.length})}
        </h4>
        <button
          type="button"
          onClick={() => void pull()}
          disabled={busy}
          className="rounded-md border border-chief/40 bg-chief/10 px-2 py-0.5 text-[11px] text-chief hover:bg-chief/20 disabled:opacity-50"
        >
          {busy ? t('settings.action.save') : '🔄 ' + t('settings.providers.discoverModels')}
        </button>
      </div>

      {customModels.length === 0 ? (
        <div className="text-[11px] text-text-secondary">
          {t('settings.models.emptyCustomModels', {provider: providerDisplay})}
        </div>
      ) : (
        <ul className="grid grid-cols-2 gap-1 text-xs">
          {customModels.map((m) => (
            <li
              key={m.id}
              className="flex items-center gap-1 rounded bg-surface-1 px-2 py-1 font-mono"
            >
              <span className="flex-1 truncate" title={m.id}>{m.display_name}</span>
              <button
                type="button"
                onClick={() => onRemove(m.id)}
                className="text-[10px] text-text-secondary hover:text-status-failed"
                aria-label={t('settings.action.remove')}
              >
                ×
              </button>
            </li>
          ))}
        </ul>
      )}

      {customModels.length > 0 && (
        <button
          type="button"
          onClick={onClear}
          className="mt-2 text-[10px] text-text-secondary hover:text-status-failed"
        >
          {t('settings.models.clearAll')}
        </button>
      )}

      {open && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4"
          onClick={() => !busy && setOpen(false)}
        >
          <div
            className="flex max-h-[80vh] w-[600px] max-w-full flex-col rounded-lg bg-surface-1 shadow-2xl"
            onClick={(e) => e.stopPropagation()}
          >
            <header className="flex items-center justify-between border-b border-border px-4 py-3">
              <h3 className="text-sm font-semibold">
                {t('settings.models.pullTitle', {provider: providerDisplay})}
              </h3>
              <button
                type="button"
                onClick={() => setOpen(false)}
                disabled={busy}
                className="text-text-secondary hover:text-primary disabled:opacity-30"
                aria-label={t('settings.action.close')}
              >
                ×
              </button>
            </header>

            <div className="flex-1 overflow-y-auto p-4">
              {busy ? (
                <div className="flex items-center justify-center py-8 text-sm text-text-secondary">
                  {t('settings.models.callingApi', {provider: providerDisplay})}
                </div>
              ) : error ? (
                <div role="alert" aria-live="polite" className="rounded border border-status-failed/40 bg-status-failed/10 p-3 text-xs text-status-failed">
                  {error}
                </div>
              ) : fetched.length === 0 ? (
                <div className="py-4 text-sm text-text-secondary">{t('settings.models.noModels')}</div>
              ) : (
                <>
                  <div className="mb-2 text-[11px] text-text-secondary">
                    {t('settings.models.foundCount', {count: fetched.length})}
                  </div>
                  <div className="mb-3 flex items-center gap-2">
                    <button
                      type="button"
                      onClick={() => setPicked(new Set(fetched.map((m) => m.id)))}
                      className="text-[11px] text-chief hover:underline"
                    >
                      {t('settings.models.all')}
                    </button>
                    <span className="text-text-secondary">·</span>
                    <button
                      type="button"
                      onClick={() => setPicked(new Set())}
                      className="text-[11px] text-text-secondary hover:underline"
                    >
                      {t('settings.models.none')}
                    </button>
                    <span className="ml-auto text-[11px] text-text-secondary">
                      {t('settings.models.selectedCount', {count: picked.size})}
                    </span>
                  </div>
                  <ul className="grid grid-cols-2 gap-1 text-xs">
                    {fetched.map((m) => {
                      const have = alreadyHave.has(m.id);
                      const picked_ = picked.has(m.id);
                      return (
                        <li
                          key={m.id}
                          className={`flex items-center gap-2 rounded border px-2 py-1 font-mono ${
                            picked_ ? 'border-chief bg-chief/10' : 'border-border bg-surface-2'
                          }`}
                        >
                          <input
                            type="checkbox"
                            checked={picked_}
                            onChange={() => togglePick(m.id)}
                            className="h-3 w-3"
                          />
                          <div className="min-w-0 flex-1">
                            <div className="truncate text-primary" title={m.id}>
                              {m.display_name}
                              {have && <span className="ml-1 text-[10px] text-status-done">{t('settings.models.alreadyAdded')}</span>}
                            </div>
                            <div className="truncate text-[10px] text-text-secondary" title={m.id}>
                              {m.id}
                            </div>
                          </div>
                        </li>
                      );
                    })}
                  </ul>
                </>
              )}
            </div>

            <footer className="flex items-center justify-end gap-2 border-t border-border px-4 py-3">
              <button
                type="button"
                onClick={() => setOpen(false)}
                disabled={busy}
                className="rounded-md border border-border bg-surface-2 px-3 py-1.5 text-xs hover:bg-surface-3 disabled:opacity-30"
              >
                取消
              </button>
              <button
                type="button"
                onClick={commit}
                disabled={busy || picked.size === 0}
                className="rounded-md bg-chief px-3 py-1.5 text-xs font-medium text-white hover:bg-chief/90 disabled:opacity-30"
              >
                {t('settings.models.addSelected', {count: picked.size})}
              </button>
            </footer>
          </div>
        </div>
      )}
    </div>
  );
}

// ── Custom Provider (relay station) ─────────────────────────────


// BUG-FRONTEND-RT-12 (event 000041): each model registered with
// a custom provider now carries an optional context_length
// (in tokens; e.g. 200000 for MiniMax-M3's 200k window) and a
// required thinking_strength enum. The chairman's directive
// was: "let the user fill in the model name + context length,
// and pick thinking strength from a list".
type ThinkingStrength = 'low' | 'medium' | 'high';
interface ModelRow {
  id: string;
  display_name: string;
  /** Context window size in tokens. null = use the runtime default. */
  context_length: number | null;
  thinking_strength: ThinkingStrength;
}
const THINKING_OPTIONS: { value: ThinkingStrength; labelKey: string }[] = [
  { value: 'low',    labelKey: 'settings.models.thinking.low' },
  { value: 'medium', labelKey: 'settings.models.thinking.medium' },
  { value: 'high',   labelKey: 'settings.models.thinking.high' },
];

function CustomProviderForm({ onSaved }: { onSaved: () => void }) {
  const { t } = useTranslation();
  const KIND_OPTIONS = [
    { value: 'anthropic', label: t('settings.custom.kind.anthropic') },
    { value: 'openai', label: t('settings.custom.kind.openai') },
    { value: 'openai_compat', label: 'OpenAI 兼容 (AI SDK)' },
  ] as const;
  // BUG-FRONTEND-RT-12 (event 000041): each model now carries
  // its own `context_length` (free-text int) + `thinking_strength`
  // (one of 'low' | 'medium' | 'high'). Lets the chairman set
  // per-model values (e.g. 200k context + high thinking for the
  // flagship MiniMax-M3, 8k + low for a cheap fast model).
  const [open, setOpen] = useState(false);
  const [id, setId] = useState('');
  const [displayName, setDisplayName] = useState('');
  const [kind, setKind] = useState<'anthropic' | 'openai' | 'openai_compat'>('openai');
  const [baseUrl, setBaseUrl] = useState('');
  const [apiKey, setApiKey] = useState('');
  const [models, setModels] = useState<ModelRow[]>([]);
  const [newModelId, setNewModelId] = useState('');
  const [newModelName, setNewModelName] = useState('');
  const [newModelContext, setNewModelContext] = useState('');
  const [newModelThinking, setNewModelThinking] = useState<ThinkingStrength>('medium');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState(false);

  const reset = () => {
    setId(''); setDisplayName(''); setKind('openai'); setBaseUrl('');
    setApiKey(''); setModels([]); setNewModelId(''); setNewModelName('');
    setNewModelContext(''); setNewModelThinking('medium');
    setError(null); setSuccess(false);
  };

  const addModelRow = () => {
    const mid = newModelId.trim();
    if (!mid) return;
    if (models.some((m) => m.id === mid)) { setError(t('settings.models.modelExists', { id: mid })); return; }
    // BUG-FRONTEND-RT-12: validate the free-text context length.
    // Accept empty (= use provider default) or a positive int.
    // Cap at 10M to keep the JSON reasonable.
    let ctx: number | null = null;
    const ctxRaw = newModelContext.trim();
    if (ctxRaw.length > 0) {
      const n = Number(ctxRaw);
      if (!Number.isFinite(n) || n <= 0 || n > 10_000_000) {
        setError(t('settings.models.invalidContextLength'));
        return;
      }
      ctx = Math.floor(n);
    }
    setModels([
      ...models,
      {
        id: mid,
        display_name: newModelName.trim() || mid,
        context_length: ctx,
        thinking_strength: newModelThinking,
      },
    ]);
    setNewModelId(''); setNewModelName(''); setNewModelContext('');
    setNewModelThinking('medium');
    setError(null);
  };

  const removeModelRow = (mid: string) => setModels(models.filter((m) => m.id !== mid));

  const handleSubmit = async () => {
    // Validate
    const idTrim = id.trim().toLowerCase();
    if (!/^[a-z0-9_]+$/.test(idTrim)) { setError(t('settings.error.invalidId')); return; }
    if (!displayName.trim()) { setError(t('settings.quickAdd.errorMissingName')); return; }
    if (!baseUrl.trim() || !(baseUrl.startsWith('http://') || baseUrl.startsWith('https://'))) {
      setError(t('settings.error.invalidBaseUrl')); return;
    }
    if (!apiKey.trim()) { setError(t('settings.error.missingApiKey')); return; }
    if (models.length === 0) { setError(t('settings.quickAdd.errorMissingKey').replace('Please enter an API key', 'Please add at least one model')); return; }

    setBusy(true); setError(null);
    try {
      // 1. Save API key to keychain
      const envVarName = `CUSTOM_${idTrim.toUpperCase()}_API_KEY`;
      const saveResult = await saveSecret(envVarName, apiKey.trim());
      if (!saveResult || !saveResult.saved) {
        setError(t('settings.error.saveFailed'));
        setBusy(false);
        return;
      }

      // 2. Register custom provider
      await addCustomProvider({
        id: idTrim,
        display_name: displayName.trim(),
        kind,
        base_url: baseUrl.trim().replace(/\/+$/, ''),
        api_key_env: envVarName,
        models,
      });

      setSuccess(true);
      onSaved();
      setTimeout(() => { setOpen(false); reset(); }, 1200);
    } catch (e) {
      setError(e instanceof Error ? e.message : t('settings.error.saveFailed'));
    } finally {
      setBusy(false);
    }
  };

  if (!open) {
    return (
      <button
        type="button"
        onClick={() => setOpen(true)}
        className="flex w-full items-center justify-center gap-2 rounded-lg border border-dashed border-text-secondary/30 bg-surface-1 px-4 py-2.5 text-sm text-text-secondary transition-colors hover:border-chief/40 hover:text-chief"
      >
        <span className="text-lg">＋</span>
        {t('settings.error.customAdd')}
      </button>
    );
  }

  return (
    <div className="rounded-lg border border-border bg-surface-1 p-4">
      <div className="mb-3 flex items-center justify-between">
        <h3 className="text-sm font-semibold text-primary">{t('settings.customProvider.title')}</h3>
        <button type="button" onClick={() => { setOpen(false); reset(); }} className="text-xs text-text-secondary hover:text-primary">{t('settings.action.cancel')}</button>
      </div>

      <div className="flex flex-col gap-2.5 text-xs">
        {/* ID */}
        <label className="flex flex-col gap-1">
          <span className="text-text-secondary">ID <span className="text-[10px]">(英文+数字+下划线)</span></span>
          <input value={id} onChange={(e) => setId(e.target.value)} placeholder={t('settings.custom.idPlaceholder')} className="rounded border border-border bg-surface-2 px-2 py-1.5 text-primary outline-none focus:border-chief" />
        </label>

        {/* Display Name */}
        <label className="flex flex-col gap-1">
          <span className="text-text-secondary">{t('settings.custom.nameLabel')}</span>
          <input value={displayName} onChange={(e) => setDisplayName(e.target.value)} placeholder={t('settings.custom.namePlaceholder')} className="rounded border border-border bg-surface-2 px-2 py-1.5 text-primary outline-none focus:border-chief" />
        </label>

        {/* Protocol */}
        <label className="flex flex-col gap-1">
          <span className="text-text-secondary">{t('settings.custom.kindLabel')}</span>
          <select value={kind} onChange={(e) => setKind(e.target.value as typeof kind)} className="rounded border border-border bg-surface-2 px-2 py-1.5 text-primary outline-none focus:border-chief">
            {KIND_OPTIONS.map((o) => <option key={o.value} value={o.value}>{o.label}</option>)}
          </select>
        </label>

        {/* Base URL */}
        <label className="flex flex-col gap-1">
          <span className="text-text-secondary">Base URL</span>
          <input value={baseUrl} onChange={(e) => setBaseUrl(e.target.value)} placeholder={t('settings.custom.baseUrlPlaceholder')} className="rounded border border-border bg-surface-2 px-2 py-1.5 text-primary outline-none focus:border-chief" />
        </label>

        {/* API Key label (i18n) */}
        <label className="flex flex-col gap-1">
          <span className="text-text-secondary">{t('settings.custom.apiKeyLabel')}</span>
          <input type="password" value={apiKey} onChange={(e) => setApiKey(e.target.value)} placeholder={t('settings.custom.apiKeyPlaceholder')} className="rounded border border-border bg-surface-2 px-2 py-1.5 text-primary outline-none focus:border-chief" />
        </label>

        {/* Models */}
        <div className="flex flex-col gap-1">
          <span className="text-text-secondary">{t('settings.models.list')}</span>
          {models.length > 0 && (
            <ul className="flex flex-col gap-1">
              {models.map((m) => (
                <li key={m.id} className="flex items-center gap-2 rounded bg-surface-2 px-2 py-1 text-[11px]">
                  <span className="min-w-0 flex-1 truncate text-primary" title={m.id}>
                    {m.display_name} <span className="text-[10px] text-text-secondary">({m.id})</span>
                  </span>
                  <span className="shrink-0 text-[10px] text-text-secondary">
                    {m.context_length
                      ? `${m.context_length.toLocaleString()} ${t('settings.models.tokens')}`
                      : t('settings.models.defaultContext')}
                  </span>
                  <span className="shrink-0 rounded bg-surface-3 px-1.5 py-0.5 text-[10px] text-text-primary">
                    {t(`settings.models.thinking.${m.thinking_strength}`)}
                  </span>
                  <button type="button" onClick={() => removeModelRow(m.id)} className="shrink-0 text-[10px] text-red-400 hover:text-red-300">✕</button>
                </li>
              ))}
            </ul>
          )}
          {/* Per-model fields. Per the chairman: model name + display
              name + context length (free text) + thinking strength
              (dropdown). All four on one row to save space. */}
          <div className="flex flex-wrap items-center gap-1.5">
            <input value={newModelId} onChange={(e) => setNewModelId(e.target.value)} placeholder={t('settings.models.newModelId')} className="flex-1 min-w-[80px] rounded border border-border bg-surface-2 px-2 py-1 text-primary outline-none focus:border-chief" />
            <input value={newModelName} onChange={(e) => setNewModelName(e.target.value)} placeholder={t('settings.models.newModelName')} className="flex-1 min-w-[80px] rounded border border-border bg-surface-2 px-2 py-1 text-primary outline-none focus:border-chief" />
            <input
              value={newModelContext}
              onChange={(e) => setNewModelContext(e.target.value)}
              placeholder={t('settings.models.contextPlaceholder')}
              type="number"
              min={0}
              max={10_000_000}
              className="w-20 rounded border border-border bg-surface-2 px-2 py-1 text-primary outline-none focus:border-chief"
            />
            <select
              value={newModelThinking}
              onChange={(e) => setNewModelThinking(e.target.value as ThinkingStrength)}
              className="rounded border border-border bg-surface-2 px-2 py-1 text-primary outline-none focus:border-chief"
            >
              {THINKING_OPTIONS.map((o) => (
                <option key={o.value} value={o.value}>{t(o.labelKey)}</option>
              ))}
            </select>
            <button type="button" onClick={addModelRow} className="shrink-0 rounded bg-surface-3 px-2 py-1 text-text-secondary hover:text-primary">+</button>
          </div>
          <p className="text-[10px] text-text-secondary">
            {t('settings.models.contextHint')}
          </p>
        </div>
      </div>

      {error && <p role="alert" aria-live="polite" className="mt-2 text-[11px] text-red-400">{error}</p>}
      {success && <p role="status" aria-live="polite" className="mt-2 text-[11px] text-status-done">{t('settings.error.alreadyAdded')}</p>}

      <div className="mt-3 flex justify-end">
        <button
          type="button"
          onClick={handleSubmit}
          disabled={busy}
          className="rounded-md bg-chief px-4 py-1.5 text-xs font-medium text-white hover:bg-chief/90 disabled:opacity-30"
        >
          {busy ? t('settings.action.save') : t('settings.action.create')}
        </button>
      </div>
    </div>
  );
}
