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
 *
 * v0.4.22 (event 000118): chair reported that collapsing an
 * expanded folder made the entire tree disappear. Root cause:
 * the previous click handler did
 *   `setOpen(o => !o); onPickDir(entry.path)`
 * i.e. every click navigated the polling root into the clicked
 * folder. So clicking what looked like a "collapse" actually
 * re-rooted the tree, replacing the visible workspace view
 * with the clicked folder's contents. From the user's
 * perspective the tree had vanished.
 *
 * Fix:
 *   - Click on a folder row ONLY toggles expand/collapse.
 *     No more silent navigation.
 *   - "navigate into" requires an explicit action: a small
 *     "进入" button next to each directory, or double-click.
 *   - When navigated, a breadcrumb + "↑ 回到根" button appears
 *     at the top so the user can always return.
 *   - The polling fetch is now based on the explicit
 *     `navigatedDir` state (not the same as `open`). Default
 *     is "" (workspace root), unchanged from before.
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
  /**
   * Toggle expand/collapse for this node. Called when the user
   * clicks the row. Does NOT navigate — that's a separate
   * action (`onNavigate`).
   */
  onToggle: (path: string) => void;
  /**
   * "→ 进入" — explicit navigation action. Only triggered by the
   * dedicated button or by double-clicking the row.
   */
  onNavigate: (path: string) => void;
  /** Set of paths the user has explicitly expanded. */
  expanded: Set<string>;
}

function Node({ entry, depth, onToggle, onNavigate, expanded }: NodeProps) {
  const indent = { paddingLeft: `${depth * 12 + 8}px` };
  const open = expanded.has(entry.path);
  if (entry.is_dir) {
    return (
      <div>
        <div
          className="group flex w-full items-center gap-1 rounded px-1 py-0.5 text-left text-xs hover:bg-surface-3"
          style={indent}
          title={entry.path}
        >
          <button
            type="button"
            // v0.4.22 (event 000118): row click ONLY toggles
            // expand/collapse. Previously this also called
            // onPickDir(entry.path) which silently re-rooted
            // the polled tree, making the workspace view
            // vanish whenever the user "collapsed" a folder.
            onClick={() => onToggle(entry.path)}
            onDoubleClick={() => onNavigate(entry.path)}
            className="flex flex-1 items-center gap-1 text-left"
            aria-expanded={open}
            aria-label={`${open ? '折叠' : '展开'} ${entry.name}`}
          >
            <span className="w-3 select-none text-text-secondary">{open ? '▾' : '▸'}</span>
            <span className="font-mono">📁 {entry.name}</span>
          </button>
          <button
            type="button"
            // Explicit "→ 进入" affordance. Without this the
            // user has no way to drill into a subfolder, so
            // we provide a small button visible on hover.
            onClick={() => onNavigate(entry.path)}
            title={`进入 ${entry.name}`}
            className="ml-auto rounded border border-border bg-surface-2 px-1.5 py-0.5 text-[10px] text-text-secondary opacity-0 transition-opacity hover:bg-surface-3 group-hover:opacity-100 focus:opacity-100"
          >
            进入
          </button>
        </div>
        {open && entry.children && (
          <div>
            {entry.children.map((c) => (
              <Node
                key={c.path}
                entry={c}
                depth={depth + 1}
                onToggle={onToggle}
                onNavigate={onNavigate}
                expanded={expanded}
              />
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
  // v0.4.22 (event 000118): explicit navigation state. Only
  // changes when the user clicks the "进入" button (or
  // double-clicks a folder). Collapsing a folder does NOT
  // touch this. Default empty = workspace root.
  const [navigatedDir, setNavigatedDir] = useState<string>('');
  // Track which folders the user has explicitly expanded.
  // Stored as a Set so the entire tree's expand state survives
  // refreshes — previously this lived in each <Node>'s local
  // useState, which reset to `depth < 1` every time the entry
  // changed.
  const [expanded, setExpanded] = useState<Set<string>>(() => new Set());

  const refresh = useCallback(async () => {
    try {
      const ws: WorkspaceInfo = await getRuntimeWorkspace();
      setRoot(ws.root);
      // Only re-root the poll when the user has explicitly
      // navigated. Otherwise we always fetch the workspace
      // root so the user can expand/collapse client-side
      // without losing their place.
      const path = navigatedDir && ws.root && navigatedDir.startsWith(ws.root)
        ? navigatedDir.slice(ws.root.length).replace(/^[\\/]+/, '')
        : '';
      const resp = await getWorkspaceTree({ path, depth: 4, max_entries: 500 });
      setData(resp);
      setError(null);
    } catch (e) {
      setError(typeof e === 'string' ? e : (e as Error).message ?? 'tree fetch failed');
    } finally {
      setLoading(false);
    }
  }, [navigatedDir]);

  useEffect(() => {
    void refresh();
    const t = setInterval(() => { void refresh(); }, pollMs);
    return () => clearInterval(t);
  }, [refresh, pollMs]);

  const onToggle = useCallback((path: string) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  }, []);

  const onNavigate = useCallback((path: string) => {
    // Setting `navigatedDir` triggers the polling fetch with
    // that subpath. The workspace-root view is preserved by
    // the breadcrumb + "↑ 回到根" button below.
    setNavigatedDir(path);
    // When the user drills into a folder, auto-expand it
    // and its ancestors so they can see the contents
    // immediately without an extra click.
    setExpanded((prev) => {
      const next = new Set(prev);
      next.add(path);
      return next;
    });
  }, []);

  const backToRoot = useCallback(() => {
    setNavigatedDir('');
  }, []);

  const rootLabel = useMemo(() => {
    if (!root) return '—';
    // Compact display: show last 2 path components.
    const parts = root.split(/[\\/]/).filter(Boolean);
    if (parts.length <= 2) return root;
    return `…/${parts.slice(-2).join('/')}`;
  }, [root]);

  const navigatedLabel = useMemo(() => {
    if (!navigatedDir) return null;
    // Compact: relative path from workspace root.
    if (root && navigatedDir.startsWith(root)) {
      return navigatedDir.slice(root.length).replace(/^[\\/]+/, '');
    }
    return navigatedDir;
  }, [navigatedDir, root]);

  return (
    <div className="flex flex-col gap-1">
      <header className="flex items-center justify-between gap-2 px-1">
        <div className="flex min-w-0 flex-col">
          <h2 className="text-xs font-semibold uppercase tracking-wide text-text-secondary">
            工作目录文件
          </h2>
          <span className="truncate font-mono text-[10px] text-text-secondary" title={root}>
            {rootLabel}
          </span>
        </div>
        <button
          type="button"
          onClick={() => { void refresh(); }}
          className="shrink-0 rounded border border-border px-2 py-0.5 text-[10px] hover:bg-surface-3"
          disabled={loading}
          aria-label="刷新文件树"
        >
          {loading ? '…' : '刷新'}
        </button>
      </header>

      {/* v0.4.22 (event 000118): breadcrumb appears when the
          user has drilled into a subfolder. Clicking "↑ 回到根"
          returns to the workspace-root view so the user is
          never stuck in an empty sub-tree. */}
      {navigatedDir && (
        <div className="flex items-center gap-1 rounded border border-chief/30 bg-chief/5 px-2 py-1">
          <button
            type="button"
            onClick={backToRoot}
            className="rounded border border-border bg-surface-2 px-1.5 py-0.5 text-[10px] hover:bg-surface-3"
            title="返回工作目录根"
          >
            ↑ 回到根
          </button>
          <span className="truncate font-mono text-[10px] text-text-secondary" title={navigatedDir}>
            {navigatedLabel || '/'}
          </span>
        </div>
      )}

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
          <Node
            key={e.path}
            entry={e}
            depth={0}
            onToggle={onToggle}
            onNavigate={onNavigate}
            expanded={expanded}
          />
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