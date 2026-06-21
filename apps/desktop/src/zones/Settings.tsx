import { useEffect, useState } from 'react';
import { Card } from '@aco/ui';
import {
  listSecrets, saveSecret, deleteSecret, revealSecret, seedSecrets,
  listProviders, toggleProvider,
  listRouterRoles, listRouterModels, updateRouterRoles,
  type SecretInfo, type ProviderInfo, type RoleInfo,
} from '../lib/api.js';

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
  { id: 'siliconflow', name: 'SiliconFlow', envVar: 'SILICONFLOW_API_KEY', placeholder: 'sk-...', description: '硅基流动', color: '#0ea5e9' },
];

function QuickAddAI({ onSaved }: { onSaved: () => void }) {
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
        setError('保存失败：返回结果无效');
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
      setError(e instanceof Error ? e.message : '保存失败');
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
        添加 AI 供应商
      </button>
    );
  }

  return (
    <div className="rounded-lg border border-border bg-surface-1 p-4">
      <div className="mb-3 flex items-center justify-between">
        <h3 className="text-sm font-semibold text-primary">添加 AI 供应商</h3>
        <button type="button" onClick={() => { setOpen(false); setSelected(null); setApiKey(''); setError(null); setSuccess(false); }} className="text-xs text-text-secondary hover:text-primary">取消</button>
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
            {provider.name} API Key
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
          {error && <p className="text-xs text-status-failed">{error}</p>}
          {success && <p className="text-xs text-status-done">✓ 已保存并激活</p>}
          <button
            type="button"
            onClick={handleSave}
            disabled={busy || !apiKey.trim()}
            className="w-full rounded bg-chief px-3 py-2 text-sm font-medium text-white hover:bg-chief/90 disabled:opacity-50"
          >
            {busy ? '保存中...' : `保存 ${provider.name} API Key`}
          </button>
        </div>
      )}
    </div>
  );
}

// ── Secrets View ─────────────────────────────────────────────────

function SecretsView({ onSaved }: { onSaved: () => void }) {
  const [secrets, setSecrets] = useState<SecretInfo[]>([]);
  const [editing, setEditing] = useState<string | null>(null);
  const [draftValue, setDraftValue] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [revealed, setRevealed] = useState<Record<string, string>>({});

  const load = async () => {
    try {
      setSecrets(await listSecrets());
    } catch (e) {
      setError(`load failed: ${e}`);
    }
  };

  useEffect(() => { void load(); }, []);

  const save = async (name: string) => {
    if (!draftValue) return;
    setBusy(true);
    setError(null);
    try {
      await saveSecret(name, draftValue);
      setEditing(null);
      setDraftValue('');
      onSaved();
      void load();
    } catch (e) {
      setError(`save failed: ${e}`);
    } finally {
      setBusy(false);
    }
  };

  const remove = async (name: string) => {
    if (!confirm(`Delete ${name}?`)) return;
    setBusy(true);
    setError(null);
    try {
      await deleteSecret(name);
      onSaved();
      void load();
    } catch (e) {
      setError(`delete failed: ${e}`);
    } finally {
      setBusy(false);
    }
  };

  const reveal = async (name: string) => {
    try {
      const value = await revealSecret(name);
      setRevealed((prev) => ({ ...prev, [name]: value }));
    } catch (e) {
      setError(`reveal failed: ${e}`);
    }
  };

  const reseed = async () => {
    setBusy(true);
    setError(null);
    try {
      await seedSecrets();
      onSaved();
    } catch (e) {
      setError(`reseed failed: ${e}`);
    } finally {
      setBusy(false);
    }
  };

  const setCount = secrets.filter((s) => s.present).length;
  const sel = secrets.find((s) => s.name === editing);

  return (
    <div className="flex flex-1 overflow-hidden">
      <aside className="w-[380px] shrink-0 overflow-y-auto border-r border-border bg-surface-2 p-3">
        <h3 className="mb-2 px-1 text-xs font-semibold uppercase tracking-wide text-text-secondary">
          Secrets ({setCount} / {secrets.length})
        </h3>
        <div className="flex flex-col gap-2">
          {secrets.map((s) => (
            <button key={s.name} type="button" onClick={() => setEditing(s.name)}
              className={`flex flex-col items-start gap-1 rounded-md border p-2 text-left transition-colors ${editing === s.name ? 'border-chief bg-surface-1' : 'border-border bg-surface-1 hover:border-text-secondary'}`}>
              <div className="flex w-full items-center justify-between">
                <span className="font-mono text-sm">{s.name}</span>
                <span className={`rounded px-1.5 py-0.5 text-[10px] ${s.present ? 'bg-success/20 text-success' : 'bg-surface-3 text-text-secondary'}`}>
                  {s.present ? 'set' : 'unset'}
                </span>
              </div>
              <div className="font-mono text-[11px] text-text-secondary">{revealed[s.name] ?? s.masked ?? '—'}</div>
            </button>
          ))}
        </div>
      </aside>

      <main className="flex flex-1 flex-col overflow-hidden bg-surface-1">
        <div className="flex items-center justify-between border-b border-border bg-surface-2 px-5 py-3">
          <div>
            <h3 className="text-sm font-semibold text-primary">{editing ?? 'Select a secret to edit'}</h3>
            <p className="text-xs text-text-secondary">Stored in OS keychain (Windows Credential Manager).</p>
          </div>
          <button type="button" onClick={reseed} disabled={busy}
            className="rounded-md border border-border bg-surface-1 px-3 py-1.5 text-xs text-text-secondary hover:text-primary disabled:opacity-50">
            Re-inject to os.environ
          </button>
        </div>
        {error !== null && (
          <div className="border-b border-danger/30 bg-danger/10 px-5 py-2 text-xs text-danger">{error}</div>
        )}
        <div className="flex-1 overflow-y-auto p-5">
          {editing === null ? (
            <div className="text-sm text-text-secondary">Pick a secret on the left.</div>
          ) : (
            <div className="mx-auto max-w-2xl space-y-4">
              <div>
                <label className="mb-1 block text-xs font-semibold uppercase tracking-wide text-text-secondary">Env var name</label>
                <div className="font-mono text-sm text-primary">{editing}</div>
              </div>
              <div>
                <label className="mb-1 block text-xs font-semibold uppercase tracking-wide text-text-secondary">Current value (masked)</label>
                <div className="flex items-center gap-2">
                  <code className="flex-1 rounded border border-border bg-surface-2 px-3 py-2 font-mono text-sm">{revealed[editing] ?? sel?.masked ?? 'unset'}</code>
                  <button type="button" onClick={() => void reveal(editing)} disabled={!sel?.present}
                    className="rounded-md border border-border bg-surface-2 px-3 py-1.5 text-xs text-text-secondary hover:text-primary disabled:opacity-50">
                    {revealed[editing] ? 'Re-fetch' : 'Show plaintext'}
                  </button>
                </div>
              </div>
              <div>
                <label className="mb-1 block text-xs font-semibold uppercase tracking-wide text-text-secondary">New value (overwrites)</label>
                <input type="password" value={draftValue} onChange={(e) => setDraftValue(e.target.value)}
                  placeholder={sel?.present ? 'new value' : 'value'}
                  className="w-full rounded border border-border bg-surface-2 px-3 py-2 font-mono text-sm placeholder:text-text-secondary focus:border-chief focus:outline-none focus:ring-2 focus:ring-chief/50" />
                <div className="mt-2 flex justify-end gap-2">
                  <button type="button" onClick={() => void save(editing)} disabled={busy || !draftValue}
                    className="rounded-md bg-chief px-3 py-1.5 text-xs font-medium text-white hover:bg-chief/90 disabled:opacity-50">
                    {busy ? 'Saving...' : 'Save to keychain'}
                  </button>
                  {sel?.present && (
                    <button type="button" onClick={() => void remove(editing)} disabled={busy}
                      className="rounded-md border border-danger/40 px-3 py-1.5 text-xs text-danger hover:bg-danger/10 disabled:opacity-50">
                      Delete
                    </button>
                  )}
                </div>
              </div>
            </div>
          )}
        </div>
      </main>
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

const ROLE_LABELS: Record<string, string> = {
  chief: '首席代理', critic_a: '审核员 A', critic_b: '审核员 B', worker: '执行员', reporter: '汇报员',
};

export interface SettingsProps {
  open: boolean;
  onClose: () => void;
}

export function Settings({ open, onClose }: SettingsProps) {
  const [snapshot, setSnapshot] = useState<RuntimeSnapshot>(EMPTY);
  const [selected, setSelected] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [savedAt, setSavedAt] = useState<string | null>(null);
  const [view, setView] = useState<'providers' | 'secrets'>('providers');

  const refresh = async () => {
    try {
      const [prov, roles, models] = await Promise.all([
        listProviders(),
        listRouterRoles(),
        listRouterModels(),
      ]);
      console.log('[Settings] refresh:', { prov, roles, models });
      if (prov && roles && models) {
        setSnapshot({ providers: prov.providers, roles: roles.roles, available_models: models.models });
        setSavedAt(new Date().toLocaleTimeString());
      }
    } catch (e) {
      console.error('[Settings] refresh failed:', e);
    }
  };

  useEffect(() => {
    if (!open) return;
    void refresh();
  }, [open]);

  const toggle = async (id: string, enabled: boolean) => {
    setSaving(true);
    try {
      await toggleProvider(id, enabled);
      setSnapshot((prev) => ({
        ...prev,
        providers: prev.providers.map((p) => (p.id === id ? { ...p, enabled } : p)),
      }));
    } finally {
      setSaving(false);
    }
  };

  const setRoleDefault = async (role: string, model: string) => {
    setSaving(true);
    try {
      const newRoles = snapshot.roles.map((r) => r.role === role ? { ...r, default_model: model } : r);
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
    <div className="fixed inset-0 z-50 flex bg-black/60 backdrop-blur-sm" role="dialog" aria-label="设置" onClick={onClose}>
      <div className="ml-auto flex h-full w-[1100px] max-w-[95vw] flex-col border-l border-border bg-surface-1 shadow-2xl" onClick={(e) => e.stopPropagation()}>
        <header className="flex h-14 shrink-0 items-center justify-between border-b border-border bg-surface-2 px-5">
          <div>
            <h2 className="text-base font-semibold text-primary">设置</h2>
            <p className="text-xs text-text-secondary">管理 LLM 供应商</p>
          </div>
          <div className="flex items-center gap-3">
            {savedAt !== null && <span className="text-xs text-text-secondary">已保存 · {savedAt}</span>}
            {saving && <span className="text-xs text-text-secondary">保存中…</span>}
            <div className="flex items-center rounded-md border border-border bg-surface-2 p-0.5">
              <button type="button" onClick={() => setView('providers')}
                className={`rounded px-2.5 py-1 text-xs ${view === 'providers' ? 'bg-surface-1 text-primary' : 'text-text-secondary hover:text-primary'}`}>
                Providers
              </button>
              <button type="button" onClick={() => setView('secrets')}
                className={`rounded px-2.5 py-1 text-xs ${view === 'secrets' ? 'bg-surface-1 text-primary' : 'text-text-secondary hover:text-primary'}`}>
                Secrets
              </button>
            </div>
            <button type="button" onClick={onClose} className="rounded-md border border-border bg-surface-1 px-3 py-1.5 text-xs text-text-secondary hover:text-primary">关闭</button>
          </div>
        </header>

        <div className="flex flex-1 overflow-hidden">
          {view === 'secrets' ? (
            <SecretsView onSaved={() => void refresh()} />
          ) : (
            <>
              <aside className="w-[380px] shrink-0 overflow-y-auto border-r border-border bg-surface-2 p-3">
                <div className="mb-3">
                  <QuickAddAI onSaved={() => void refresh()} />
                </div>
                <h3 className="mb-2 px-1 text-xs font-semibold uppercase tracking-wide text-text-secondary">
                  供应商（{snapshot.providers.length}）
                </h3>
                <div className="flex flex-col gap-2">
                  {snapshot.providers.map((p) => {
                    const isSel = p.id === selected;
                    return (
                      <button key={p.id} type="button" onClick={() => setSelected(p.id)}
                        className={`flex flex-col items-start gap-1 rounded-md border p-2 text-left transition-colors ${isSel ? 'border-chief bg-surface-1' : 'border-border bg-surface-1 hover:border-text-secondary'}`}>
                        <div className="flex w-full items-center justify-between">
                          <div className="flex items-center gap-2">
                            <span className="font-medium text-sm">{p.display_name}</span>
                            {p.is_local && <span className="rounded bg-surface-3 px-1.5 py-0.5 text-[10px] text-text-secondary">本地</span>}
                          </div>
                          <Toggle on={p.enabled} onChange={(v) => void toggle(p.id, v)} disabled={!p.key_present && !p.is_local} />
                        </div>
                        <div className="text-[11px] text-text-secondary">{p.models.length} 个模型 · {p.api_key_env}</div>
                        <KeyBadge present={p.key_present} />
                      </button>
                    );
                  })}
                </div>
              </aside>

              <main className="flex-1 overflow-y-auto p-5">
                {sel && (
                  <Card className="mb-4">
                    <h3 className="mb-1 text-sm font-semibold">{sel.display_name}</h3>
                    <p className="mb-3 text-xs text-text-secondary">{sel.notes}</p>
                    <div className="grid grid-cols-2 gap-3 text-xs">
                      <Field label="类型">{sel.kind}</Field>
                      <Field label="API Base URL"><code className="font-mono">{sel.base_url}</code></Field>
                      <Field label="API Key 环境变量"><code className="font-mono">{sel.api_key_env}</code></Field>
                      <Field label="Key 已配置">
                        {sel.key_present ? (
                          <span className="text-status-done">✓ 是</span>
                        ) : (
                          <span className="text-status-warn">✗ 否（设置 <code className="font-mono">{sel.api_key_env}</code> 后重启 runtime）</span>
                        )}
                      </Field>
                    </div>
                    {sel.models.length > 0 && (
                      <div className="mt-3">
                        <h4 className="mb-1 text-xs font-semibold uppercase tracking-wide text-text-secondary">可用模型</h4>
                        <ul className="grid grid-cols-2 gap-1 text-xs">
                          {sel.models.map((m) => (
                            <li key={m.id} className="rounded bg-surface-2 px-2 py-1 font-mono">
                              {m.display_name} <span className="ml-1 text-text-secondary">({m.id})</span>
                            </li>
                          ))}
                        </ul>
                      </div>
                    )}
                  </Card>
                )}

                <h3 className="mb-2 px-1 text-xs font-semibold uppercase tracking-wide text-text-secondary">角色 → 模型 分配</h3>
                <div className="flex flex-col gap-2">
                  {snapshot.roles.map((r) => (
                    <Card key={r.role} className="!p-3">
                      <div className="flex items-center gap-3">
                        <div className="w-24 shrink-0 text-sm font-medium">{ROLE_LABELS[r.role] ?? r.role}</div>
                        <select value={r.default_model} onChange={(e) => void setRoleDefault(r.role, e.target.value)} disabled={saving}
                          className="flex-1 rounded-md border border-border bg-surface-1 px-2 py-1.5 text-xs focus:border-chief focus:outline-none disabled:opacity-50">
                          {snapshot.available_models.map((m) => {
                            const ref = `${m.provider}:${m.model}`;
                            return <option key={ref} value={ref}>{m.provider_display} · {m.display_name}</option>;
                          })}
                        </select>
                      </div>
                      {r.fallback_chain.length > 0 && (
                        <div className="mt-2 flex flex-wrap gap-1 text-[10px] text-text-secondary">
                          <span>回退：</span>
                          {r.fallback_chain.map((m) => <code key={m} className="rounded bg-surface-2 px-1.5 py-0.5 font-mono">{m}</code>)}
                        </div>
                      )}
                    </Card>
                  ))}
                </div>
              </main>
            </>
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
  return (
    <button type="button" onClick={(e) => { e.stopPropagation(); onChange(!on); }} disabled={disabled}
      className={`relative h-5 w-9 rounded-full transition-colors ${on ? 'bg-chief' : 'bg-surface-3'} ${disabled ? 'opacity-40 cursor-not-allowed' : ''}`}
      aria-pressed={on} aria-label="启用">
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
