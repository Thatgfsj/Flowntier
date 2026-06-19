"""Patch Settings.tsx to add a Secrets tab."""
import sys

path = sys.argv[1]
with open(path, 'r', encoding='utf-8') as f:
    src = f.read()

addition = '''
interface SecretInfo {
  name: string;
  present: boolean;
  masked: string | null;
}

function SecretsView({ onSaved }: { onSaved: () => void }) {
  const [secrets, setSecrets] = useState<SecretInfo[]>([]);
  const [editing, setEditing] = useState<string | null>(null);
  const [draftValue, setDraftValue] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [revealed, setRevealed] = useState<Record<string, string>>({});

  const load = async () => {
    try {
      const r = await fetch('http://127.0.0.1:7317/api/settings/secrets');
      setSecrets((await r.json()) as SecretInfo[]);
    } catch (e) {
      setError(`load failed: ${e}`);
    }
  };

  useEffect(() => {
    void load();
  }, []);

  const startEdit = (name: string) => {
    setEditing(name);
    setDraftValue('');
    setError(null);
  };

  const save = async (name: string) => {
    if (!draftValue) return;
    setBusy(true);
    setError(null);
    try {
      const r = await fetch(
        `http://127.0.0.1:7317/api/settings/secrets/${name}`,
        {
          method: 'PUT',
          headers: { 'content-type': 'application/json' },
          body: JSON.stringify({ value: draftValue }),
        }
      );
      if (!r.ok) throw new Error(`HTTP ${r.status}`);
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
      const r = await fetch(
        `http://127.0.0.1:7317/api/settings/secrets/${name}`,
        { method: 'DELETE' }
      );
      if (!r.ok && r.status !== 404) throw new Error(`HTTP ${r.status}`);
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
      const r = await fetch(
        `http://127.0.0.1:7317/api/settings/secrets/${name}/reveal`,
        { method: 'POST' }
      );
      if (!r.ok) throw new Error(`HTTP ${r.status}`);
      const data = (await r.json()) as { value: string };
      setRevealed((prev) => ({ ...prev, [name]: data.value }));
    } catch (e) {
      setError(`reveal failed: ${e}`);
    }
  };

  const reseed = async () => {
    setBusy(true);
    setError(null);
    try {
      await fetch(
        'http://127.0.0.1:7317/api/settings/secrets/seed',
        { method: 'POST' }
      );
      onSaved();
    } catch (e) {
      setError(`reseed failed: ${e}`);
    } finally {
      setBusy(false);
    }
  };

  const setCount = secrets.filter((s) => s.present).length;
  return (
    <div className="flex flex-1 overflow-hidden">
      <aside className="w-[380px] shrink-0 overflow-y-auto border-r border-border bg-surface-2 p-3">
        <h3 className="mb-2 px-1 text-xs font-semibold uppercase tracking-wide text-text-secondary">
          Secrets ({setCount} / {secrets.length})
        </h3>
        <div className="flex flex-col gap-2">
          {secrets.map((s) => (
            <button
              key={s.name}
              type="button"
              onClick={() => setEditing(s.name)}
              className={`flex flex-col items-start gap-1 rounded-md border p-2 text-left transition-colors ${
                editing === s.name
                  ? 'border-chief bg-surface-1'
                  : 'border-border bg-surface-1 hover:border-text-secondary'
              }`}
            >
              <div className="flex w-full items-center justify-between">
                <span className="font-mono text-sm">{s.name}</span>
                <span
                  className={`rounded px-1.5 py-0.5 text-[10px] ${
                    s.present
                      ? 'bg-success/20 text-success'
                      : 'bg-surface-3 text-text-secondary'
                  }`}
                >
                  {s.present ? 'set' : 'unset'}
                </span>
              </div>
              <div className="font-mono text-[11px] text-text-secondary">
                {revealed[s.name] ?? s.masked ?? '—'}
              </div>
            </button>
          ))}
        </div>
      </aside>

      <main className="flex flex-1 flex-col overflow-hidden bg-surface-1">
        <div className="flex items-center justify-between border-b border-border bg-surface-2 px-5 py-3">
          <div>
            <h3 className="text-sm font-semibold text-primary">
              {editing ?? 'Select a secret to edit'}
            </h3>
            <p className="text-xs text-text-secondary">
              Stored in OS keychain (Windows Credential Manager / macOS Keychain / Linux Secret Service).
              Manage via <code className="font-mono">/api/settings/secrets/*</code>.
            </p>
          </div>
          <button
            type="button"
            onClick={reseed}
            disabled={busy}
            className="rounded-md border border-border bg-surface-1 px-3 py-1.5 text-xs text-text-secondary hover:text-primary disabled:opacity-50"
          >
            Re-inject to os.environ
          </button>
        </div>
        {error !== null && (
          <div className="border-b border-danger/30 bg-danger/10 px-5 py-2 text-xs text-danger">
            {error}
          </div>
        )}
        <div className="flex-1 overflow-y-auto p-5">
          {editing === null ? (
            <div className="text-sm text-text-secondary">
              Pick a secret on the left. After setting it, the runtime auto-injects to
              os.environ on startup, or call
              {' '}<code className="font-mono">/api/settings/secrets/seed</code>
              {' '}to refresh without restart.
            </div>
          ) : (
            <SecretDetail
              name={editing}
              info={secrets.find((s) => s.name === editing) ?? null}
              revealedValue={revealed[editing] ?? null}
              draftValue={draftValue}
              onDraftChange={setDraftValue}
              onSave={() => void save(editing)}
              onDelete={() => void remove(editing)}
              onReveal={() => void reveal(editing)}
              busy={busy}
            />
          )}
        </div>
      </main>
    </div>
  );
}

function SecretDetail({
  name,
  info,
  revealedValue,
  draftValue,
  onDraftChange,
  onSave,
  onDelete,
  onReveal,
  busy,
}: {
  name: string;
  info: SecretInfo | null;
  revealedValue: string | null;
  draftValue: string;
  onDraftChange: (v: string) => void;
  onSave: () => void;
  onDelete: () => void;
  onReveal: () => void;
  busy: boolean;
}) {
  return (
    <div className="mx-auto max-w-2xl space-y-4">
      <div>
        <label className="mb-1 block text-xs font-semibold uppercase tracking-wide text-text-secondary">
          Env var name
        </label>
        <div className="font-mono text-sm text-primary">{name}</div>
      </div>
      <div>
        <label className="mb-1 block text-xs font-semibold uppercase tracking-wide text-text-secondary">
          Current value (masked)
        </label>
        <div className="flex items-center gap-2">
          <code className="flex-1 rounded border border-border bg-surface-2 px-3 py-2 font-mono text-sm">
            {revealedValue ?? info?.masked ?? 'unset'}
          </code>
          <button
            type="button"
            onClick={onReveal}
            disabled={!info?.present}
            className="rounded-md border border-border bg-surface-2 px-3 py-1.5 text-xs text-text-secondary hover:text-primary disabled:opacity-50"
          >
            {revealedValue ? 'Re-fetch' : 'Show plaintext'}
          </button>
        </div>
        <p className="mt-1 text-[11px] text-text-secondary">
          Plaintext is returned only when you click Show. Not cached in the UI.
        </p>
      </div>
      <div>
        <label className="mb-1 block text-xs font-semibold uppercase tracking-wide text-text-secondary">
          New value (overwrites)
        </label>
        <input
          type="password"
          value={draftValue}
          onChange={(e) => onDraftChange(e.target.value)}
          placeholder={info?.present ? 'new value' : 'value'}
          className="w-full rounded border border-border bg-surface-2 px-3 py-2 font-mono text-sm placeholder:text-text-secondary focus:border-chief focus:outline-none focus:ring-2 focus:ring-chief/50"
        />
        <div className="mt-2 flex justify-end gap-2">
          <button
            type="button"
            onClick={onSave}
            disabled={busy || !draftValue}
            className="rounded-md bg-chief px-3 py-1.5 text-xs font-medium text-white hover:bg-chief/90 disabled:opacity-50"
          >
            {busy ? 'Saving...' : 'Save to keychain'}
          </button>
          {info?.present && (
            <button
              type="button"
              onClick={onDelete}
              disabled={busy}
              className="rounded-md border border-danger/40 px-3 py-1.5 text-xs text-danger hover:bg-danger/10 disabled:opacity-50"
            >
              Delete
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

'''

# 1. Insert SecretsView before "export function Settings"
marker = "export function Settings({ open, onClose }: SettingsProps) {"
if marker not in src:
    raise SystemExit('marker not found')
src = src.replace(marker, addition + marker, 1)

# 2. Add view state
old_state = """export function Settings({ open, onClose }: SettingsProps) {
  const [snapshot, setSnapshot] = useState<RuntimeSnapshot>(EMPTY);
  const [selected, setSelected] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [savedAt, setSavedAt] = useState<string | null>(null);"""
new_state = """export function Settings({ open, onClose }: SettingsProps) {
  const [snapshot, setSnapshot] = useState<RuntimeSnapshot>(EMPTY);
  const [selected, setSelected] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [savedAt, setSavedAt] = useState<string | null>(null);
  const [view, setView] = useState<'providers' | 'secrets'>('providers');"""
src = src.replace(old_state, new_state, 1)

# 3. Add tab buttons in header
old_close_btn = """<button
              type="button"
              onClick={onClose}
              className="rounded-md border border-border bg-surface-1 px-3 py-1.5 text-xs text-text-secondary hover:text-primary"
            >
              关闭
            </button>"""
new_close_btn = """<div className="flex items-center rounded-md border border-border bg-surface-2 p-0.5">
              <button
                type="button"
                onClick={() => setView('providers')}
                className={`rounded px-2.5 py-1 text-xs ${view === 'providers' ? 'bg-surface-1 text-primary' : 'text-text-secondary hover:text-primary'}`}
              >
                Providers
              </button>
              <button
                type="button"
                onClick={() => setView('secrets')}
                className={`rounded px-2.5 py-1 text-xs ${view === 'secrets' ? 'bg-surface-1 text-primary' : 'text-text-secondary hover:text-primary'}`}
              >
                Secrets
              </button>
            </div>
            <button
              type="button"
              onClick={onClose}
              className="rounded-md border border-border bg-surface-1 px-3 py-1.5 text-xs text-text-secondary hover:text-primary"
            >
              关闭
            </button>"""
src = src.replace(old_close_btn, new_close_btn, 1)

# 4. Switch body to render SecretsView or providers
old_body_open = '<div className="flex flex-1 overflow-hidden">\n          {/* Left: provider list */}'
new_body_open = """<div className="flex flex-1 overflow-hidden">
          {view === 'secrets' ? (
            <SecretsView
              onSaved={() => {
                void (async () => {
                  try {
                    const [prov, roles, models] = await Promise.all([
                      fetch('http://127.0.0.1:7317/api/providers').then((r) => r.json() as Promise<{providers: ProviderInfo[]; roles: RoleInfo[]}>),
                      fetch('http://127.0.0.1:7317/api/router/roles').then((r) => r.json() as Promise<{roles: RoleInfo[]}>),
                      fetch('http://127.0.0.1:7317/api/router/models').then((r) => r.json() as Promise<{models: RuntimeSnapshot["available_models"]}>),
                    ]);
                    setSnapshot({ providers: prov.providers, roles: roles.roles, available_models: models.models });
                    setSavedAt(new Date().toLocaleTimeString());
                  } catch {
                    /* ignore */
                  }
                })();
              }}
            />
          ) : (
          <>
          {/* Left: provider list */}"""
src = src.replace(old_body_open, new_body_open, 1)

# 5. Close the conditional at end (last occurrence)
old_body_close = """          </main>
        </div>
      </div>
    </div>
  );
}"""
new_body_close = """          </main>
          </>
          )}
        </div>
      </div>
    </div>
  );
}"""
idx = src.rfind(old_body_close)
if idx < 0:
    raise SystemExit('body close marker not found')
src = src[:idx] + new_body_close + src[idx + len(old_body_close):]

with open(path, 'w', encoding='utf-8') as f:
    f.write(src)
print('written:', len(src), 'chars')