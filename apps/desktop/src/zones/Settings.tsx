import { useEffect, useState } from 'react';
import { Card } from '@aco/ui';

interface ProviderInfo {
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

interface RoleInfo {
  role: string;
  default_model: string;
  fallback_chain: string[];
}

const ROLE_LABELS: Record<string, string> = {
  chief: '首席代理',
  critic_a: '审核员 A',
  critic_b: '审核员 B',
  worker: '执行员',
  reporter: '汇报员',
};

export interface SettingsProps {
  open: boolean;
  onClose: () => void;
}

interface RuntimeSnapshot {
  providers: ProviderInfo[];
  roles: RoleInfo[];
  available_models: { provider: string; provider_display: string; model: string; display_name: string }[];
}

const EMPTY: RuntimeSnapshot = { providers: [], roles: [], available_models: [] };

/**
 * Settings 抽屉 — provider 管理 + role 分配。
 *
 * 左列：11 个 provider 卡片（带 enable 切换 + key 状态指示）
 * 右列：选中的 provider 详情 + 5 个 role 的下拉
 * 底部：保存按钮（PATCH /api/providers/{id} + PUT /api/router/roles）
 */
export function Settings({ open, onClose }: SettingsProps) {
  const [snapshot, setSnapshot] = useState<RuntimeSnapshot>(EMPTY);
  const [selected, setSelected] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [savedAt, setSavedAt] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    void (async () => {
      try {
        const [provResp, rolesResp] = await Promise.all([
          fetch('http://127.0.0.1:7317/api/providers'),
          fetch('http://127.0.0.1:7317/api/router/roles'),
        ]);
        const provData = (await provResp.json()) as {
          providers: ProviderInfo[];
          roles: RoleInfo[];
        };
        const rolesData = (await rolesResp.json()) as { roles: RoleInfo[] };
        const modelsResp = await fetch('http://127.0.0.1:7317/api/router/models');
        const modelsData = (await modelsResp.json()) as {
          models: { provider: string; provider_display: string; model: string; display_name: string }[];
        };
        setSnapshot({
          providers: provData.providers,
          roles: rolesData.roles,
          available_models: modelsData.models,
        });
        if (!selected && provData.providers.length > 0) {
          setSelected(provData.providers[0].id);
        }
      } catch (e) {
        // eslint-disable-next-line no-console
        console.warn('Settings: failed to load providers', e);
      }
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const toggle = async (id: string, enabled: boolean) => {
    setSaving(true);
    try {
      await fetch(`http://127.0.0.1:7317/api/providers/${id}`, {
        method: 'PATCH',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ enabled }),
      });
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
      const newRoles = snapshot.roles.map((r) =>
        r.role === role ? { ...r, default_model: model } : r,
      );
      await fetch('http://127.0.0.1:7317/api/router/roles', {
        method: 'PUT',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ roles: newRoles }),
      });
      setSnapshot((prev) => ({ ...prev, roles: newRoles }));
      setSavedAt(new Date().toLocaleTimeString());
    } finally {
      setSaving(false);
    }
  };

  if (!open) return null;

  const sel = snapshot.providers.find((p) => p.id === selected);

  return (
    <div
      className="fixed inset-0 z-50 flex bg-black/60 backdrop-blur-sm"
      role="dialog"
      aria-label="设置"
      onClick={onClose}
    >
      <div
        className="ml-auto flex h-full w-[1100px] max-w-[95vw] flex-col border-l border-border bg-surface-1 shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        <header className="flex h-14 shrink-0 items-center justify-between border-b border-border bg-surface-2 px-5">
          <div>
            <h2 className="text-base font-semibold text-primary">设置</h2>
            <p className="text-xs text-text-secondary">
              管理 LLM 供应商（env vars only — API key 永不写入磁盘）
            </p>
          </div>
          <div className="flex items-center gap-3">
            {savedAt !== null && (
              <span className="text-xs text-text-secondary">已保存 · {savedAt}</span>
            )}
            {saving && <span className="text-xs text-text-secondary">保存中…</span>}
            <button
              type="button"
              onClick={onClose}
              className="rounded-md border border-border bg-surface-1 px-3 py-1.5 text-xs text-text-secondary hover:text-primary"
            >
              关闭
            </button>
          </div>
        </header>

        <div className="flex flex-1 overflow-hidden">
          {/* Left: provider list */}
          <aside className="w-[380px] shrink-0 overflow-y-auto border-r border-border bg-surface-2 p-3">
            <h3 className="mb-2 px-1 text-xs font-semibold uppercase tracking-wide text-text-secondary">
              供应商（{snapshot.providers.length}）
            </h3>
            <div className="flex flex-col gap-2">
              {snapshot.providers.map((p) => {
                const isSel = p.id === selected;
                return (
                  <button
                    key={p.id}
                    type="button"
                    onClick={() => setSelected(p.id)}
                    className={`flex flex-col items-start gap-1 rounded-md border p-2 text-left transition-colors ${
                      isSel
                        ? 'border-chief bg-surface-1'
                        : 'border-border bg-surface-1 hover:border-text-secondary'
                    }`}
                  >
                    <div className="flex w-full items-center justify-between">
                      <div className="flex items-center gap-2">
                        <span className="font-medium text-sm">{p.display_name}</span>
                        {p.is_local && (
                          <span className="rounded bg-surface-3 px-1.5 py-0.5 text-[10px] text-text-secondary">
                            本地
                          </span>
                        )}
                      </div>
                      <Toggle
                        on={p.enabled}
                        onChange={(v) => void toggle(p.id, v)}
                        disabled={!p.key_present && !p.is_local}
                      />
                    </div>
                    <div className="text-[11px] text-text-secondary">
                      {p.models.length} 个模型 · {p.api_key_env}
                    </div>
                    <KeyBadge present={p.key_present} />
                  </button>
                );
              })}
            </div>
          </aside>

          {/* Right: detail + roles */}
          <main className="flex-1 overflow-y-auto p-5">
            {sel && (
              <Card className="mb-4">
                <h3 className="mb-1 text-sm font-semibold">{sel.display_name}</h3>
                <p className="mb-3 text-xs text-text-secondary">{sel.notes}</p>
                <div className="grid grid-cols-2 gap-3 text-xs">
                  <Field label="类型">{sel.kind}</Field>
                  <Field label="API Base URL">
                    <code className="font-mono">{sel.base_url}</code>
                  </Field>
                  <Field label="API Key 环境变量">
                    <code className="font-mono">{sel.api_key_env}</code>
                  </Field>
                  <Field label="Key 已配置">
                    {sel.key_present ? (
                      <span className="text-status-done">✓ 是（从 {sel.api_key_env}）</span>
                    ) : (
                      <span className="text-status-warn">
                        ✗ 否（设置 <code className="font-mono">{sel.api_key_env}</code> 后重启 runtime）
                      </span>
                    )}
                  </Field>
                </div>
                {sel.models.length > 0 && (
                  <div className="mt-3">
                    <h4 className="mb-1 text-xs font-semibold uppercase tracking-wide text-text-secondary">
                      可用模型
                    </h4>
                    <ul className="grid grid-cols-2 gap-1 text-xs">
                      {sel.models.map((m) => (
                        <li
                          key={m.id}
                          className="rounded bg-surface-2 px-2 py-1 font-mono"
                        >
                          {m.display_name}
                          <span className="ml-1 text-text-secondary">({m.id})</span>
                        </li>
                      ))}
                    </ul>
                  </div>
                )}
              </Card>
            )}

            <h3 className="mb-2 px-1 text-xs font-semibold uppercase tracking-wide text-text-secondary">
              角色 → 模型 分配
            </h3>
            <div className="flex flex-col gap-2">
              {snapshot.roles.map((r) => (
                <Card key={r.role} className="!p-3">
                  <div className="flex items-center gap-3">
                    <div className="w-24 shrink-0 text-sm font-medium">
                      {ROLE_LABELS[r.role] ?? r.role}
                    </div>
                    <select
                      value={r.default_model}
                      onChange={(e) => void setRoleDefault(r.role, e.target.value)}
                      disabled={saving}
                      className="flex-1 rounded-md border border-border bg-surface-1 px-2 py-1.5 text-xs focus:border-chief focus:outline-none disabled:opacity-50"
                    >
                      {snapshot.available_models.map((m) => {
                        const ref = `${m.provider}:${m.model}`;
                        return (
                          <option key={ref} value={ref}>
                            {m.provider_display} · {m.display_name}
                          </option>
                        );
                      })}
                    </select>
                  </div>
                  {r.fallback_chain.length > 0 && (
                    <div className="mt-2 flex flex-wrap gap-1 text-[10px] text-text-secondary">
                      <span>回退：</span>
                      {r.fallback_chain.map((m) => (
                        <code
                          key={m}
                          className="rounded bg-surface-2 px-1.5 py-0.5 font-mono"
                        >
                          {m}
                        </code>
                      ))}
                    </div>
                  )}
                </Card>
              ))}
            </div>
          </main>
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

function Toggle({
  on,
  onChange,
  disabled,
}: {
  on: boolean;
  onChange: (v: boolean) => void;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      onClick={(e) => {
        e.stopPropagation();
        onChange(!on);
      }}
      disabled={disabled}
      className={`relative h-5 w-9 rounded-full transition-colors ${
        on ? 'bg-chief' : 'bg-surface-3'
      } ${disabled ? 'opacity-40 cursor-not-allowed' : ''}`}
      aria-pressed={on}
      aria-label="启用"
    >
      <span
        className={`absolute top-0.5 h-4 w-4 rounded-full bg-white transition-all ${
          on ? 'left-4' : 'left-0.5'
        }`}
      />
    </button>
  );
}

function KeyBadge({ present }: { present: boolean }) {
  return present ? (
    <span className="rounded bg-status-done/20 px-1.5 py-0.5 text-[10px] text-status-done">
      Key ✓
    </span>
  ) : (
    <span className="rounded bg-status-warn/20 px-1.5 py-0.5 text-[10px] text-status-warn">
      Key ✗
    </span>
  );
}
