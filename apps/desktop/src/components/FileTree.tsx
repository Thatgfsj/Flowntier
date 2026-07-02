/**
 * FileTree — v0.4.21 (event 000066).
 *
 * Renders a directory listing for the pipe-server's current
 * workspace root. Polls every 5s so chief agent's writes show
 * up without a manual refresh. Clicking a directory re-fetches
 * its subtree; clicking a file shows its size (no preview yet —
 * that's a follow-up).
 *
 * Why this exists: the chairman reported "切工作目录不显示新文件".
 * Root cause was the runtime workspace never refreshed when
 * workdir.json changed. Event 000066 fixes the runtime side;
 * this component surfaces the live tree to the chairman so they
 * can see files appear in real time.
 */
import { useCallback, useEffect, useMemo, useState } from 'react';
import { getWorkspaceTree, type FileTreeEntry, type FileTreeResponse, type WorkspaceInfo, getRuntimeWorkspace } from '../lib/api.js';

export interface FileTreeProps {
  /** Optional override; default = poll `/api/workspace/tree` every N ms. */
  pollMs?: number;
}

function fmtSize(n: number | undefined): string {
  if (n == null) return '';
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}

interface NodeProps {
  entry: FileTreeEntry;
  depth: number;
  onPickDir: (path: string) => void;
}

function Node({ entry, depth, onPickDir }: NodeProps) {
  const [open, setOpen] = useState(depth < 1);
  const indent = { paddingLeft: `${depth * 12 + 8}px` };
  if (entry.is_dir) {
    return (
      <div>
        <button
          type="button"
          onClick={() => { setOpen((o) => !o); onPickDir(entry.path); }}
          className="flex w-full items-center gap-1 rounded px-1 py-0.5 text-left text-xs hover:bg-surface-3"
          style={indent}
          title={entry.path}
        >
          <span className="w-3 select-none text-text-secondary">{open ? '▾' : '▸'}</span>
          <span className="font-mono">📁 {entry.name}</span>
        </button>
        {open && entry.children && (
          <div>
            {entry.children.map((c) => (
              <Node key={c.path} entry={c} depth={depth + 1} onPickDir={onPickDir} />
            ))}
          </div>
        )}
      </div>
    );
  }
  return (
    <div
      className="flex w-full items-center gap-1 rounded px-1 py-0.5 text-left text-xs"
      style={indent}
      title={entry.path}
    >
      <span className="w-3 select-none text-text-secondary"> </span>
      <span className="font-mono">📄 {entry.name}</span>
      <span className="ml-auto text-text-secondary">{fmtSize(entry.size)}</span>
    </div>
  );
}

export function FileTree({ pollMs = 5000 }: FileTreeProps) {
  const [root, setRoot] = useState<string>('');
  const [data, setData] = useState<FileTreeResponse | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [pickedDir, setPickedDir] = useState<string>('');

  const refresh = useCallback(async () => {
    try {
      const ws: WorkspaceInfo = await getRuntimeWorkspace();
      setRoot(ws.root);
      const path = pickedDir && ws.root && pickedDir.startsWith(ws.root)
        ? pickedDir.slice(ws.root.length).replace(/^[\\/]+/, '')
        : '';
      const resp = await getWorkspaceTree({ path, depth: 2, max_entries: 200 });
      setData(resp);
      setError(null);
    } catch (e) {
      setError(typeof e === 'string' ? e : (e as Error).message ?? 'tree fetch failed');
    } finally {
      setLoading(false);
    }
  }, [pickedDir]);

  useEffect(() => {
    void refresh();
    const t = setInterval(() => { void refresh(); }, pollMs);
    return () => clearInterval(t);
  }, [refresh, pollMs]);

  const rootLabel = useMemo(() => {
    if (!root) return '—';
    // Compact display: show last 2 path components.
    const parts = root.split(/[\\/]/).filter(Boolean);
    if (parts.length <= 2) return root;
    return `…/${parts.slice(-2).join('/')}`;
  }, [root]);

  return (
    <div className="flex flex-col gap-1">
      <header className="flex items-center justify-between gap-2 px-1">
        <div className="flex flex-col">
          <h2 className="text-xs font-semibold uppercase tracking-wide text-text-secondary">
            工作目录文件
          </h2>
          <span className="font-mono text-[10px] text-text-secondary" title={root}>
            {rootLabel}
          </span>
        </div>
        <button
          type="button"
          onClick={() => { void refresh(); }}
          className="rounded border border-border px-2 py-0.5 text-[10px] hover:bg-surface-3"
          disabled={loading}
          aria-label="刷新文件树"
        >
          {loading ? '…' : '刷新'}
        </button>
      </header>
      {error && (
        <div className="rounded border border-red-300 bg-red-900/30 px-2 py-1 text-[11px] text-red-200">
          {error}
        </div>
      )}
      <div className="max-h-[60vh] overflow-y-auto rounded border border-border bg-surface-1 p-1">
        {data?.entries.length === 0 && (
          <div className="px-2 py-1 text-[11px] text-text-secondary">
            空目录
          </div>
        )}
        {data?.entries.map((e) => (
          <Node key={e.path} entry={e} depth={0} onPickDir={setPickedDir} />
        ))}
        {data?.truncated && (
          <div className="px-2 py-1 text-[10px] italic text-text-secondary">
            …more (已截断,增大 max_entries)
          </div>
        )}
      </div>
      <footer className="flex items-center justify-between px-1 text-[10px] text-text-secondary">
        <span>共 {data?.count ?? 0} 项</span>
        <span>每 {Math.round(pollMs / 1000)}s 自动刷新</span>
      </footer>
    </div>
  );
}