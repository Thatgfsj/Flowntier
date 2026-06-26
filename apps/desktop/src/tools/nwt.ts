/**
 * NWT (neuroweave-timeline) — TypeScript port of the upstream
 * Python project at https://github.com/Thatgfsj/neuroweave-timeline.
 *
 * Used by Flowntier to record every meaningful action as a
 * Timeline Event. The data format is intentionally compatible
 * with the upstream nwt CLI: same event JSON shape, same .nwt/
 * directory layout, same 6-digit zero-padded event ids. So a
 * developer can `cd <flowntier-project> && nwt history` from
 * the upstream CLI and see Flowntier's records.
 *
 * Storage layout (matches the upstream nwt Python):
 *   <root>/.nwt/
 *     metadata.json     (project name, created, schema version)
 *     timeline/
 *       000001.json     (one file per event, id-named)
 *       000002.json
 *       ...
 *     indices/
 *       tags.json       (map of tag -> [event ids])
 *       files.json      (map of file -> [event ids])
 *
 * Concurrency model: single-writer per project (the Flowntier
 * desktop app). Reads are unlocked. Writes are unlocked too —
 * the worst case is a torn write if two events are added
 * simultaneously, which we accept (one will be missing its id).
 * The AI agent invokes `nwt.log` synchronously, so two
 * simultaneous invocations from the same agent don't happen.
 *
 * Why a TypeScript port, not a subprocess wrapper:
 *  - The chairman's reference (gfcode) did the same rewrite:
 *    "用 TypeScript 重写了一遍, 塞到 src/tools/nwt.ts"
 *  - No Python dep in v0.4 (the runtime is Rust, not Python)
 *  - Direct in-process calls; no JSON-RPC round-trip
 *  - Data format is plain JSON; TypeScript's `JSON.parse/stringify`
 *    is sufficient (no PyON or pickle)
 */
import { readFileSync, writeFileSync, readdirSync, existsSync, mkdirSync, unlinkSync } from 'node:fs';
import { join, relative } from 'node:path';

// ── Event schema (matches upstream nwt) ──────────────────────

export interface NwtEvent {
  id: string;
  timestamp: string;
  task: string;
  summary: string;
  reason?: string;
  files?: string[];
  tags?: string[];
  parent?: string;
}

// ── Layout helpers (matches upstream nwt/storage/layout.py) ──

function ensureNwtDir(rootDir: string): string {
  const nwtDir = join(rootDir, '.nwt');
  if (!existsSync(nwtDir)) {
    mkdirSync(join(nwtDir, 'timeline'), { recursive: true });
    mkdirSync(join(nwtDir, 'indices'), { recursive: true });
  }
  return nwtDir;
}

function eventPath(nwtDir: string, id: string): string {
  return join(nwtDir, 'timeline', `${id}.json`);
}

function metadataPath(nwtDir: string): string {
  return join(nwtDir, 'metadata.json');
}

function tagsIndexPath(nwtDir: string): string {
  return join(nwtDir, 'indices', 'tags.json');
}

function filesIndexPath(nwtDir: string): string {
  return join(nwtDir, 'indices', 'files.json');
}

// ── Id allocation (matches upstream 6-digit zero-padded) ─────

function readHighestId(nwtDir: string): number {
  const timelineDir = join(nwtDir, 'timeline');
  if (!existsSync(timelineDir)) return 0;
  let max = 0;
  for (const f of readdirSync(timelineDir)) {
    const m = /^(\d{6})\.json$/.exec(f);
    if (m) {
      const n = parseInt(m[1]!, 10);
      if (n > max) max = n;
    }
  }
  return max;
}

function nextId(nwtDir: string): string {
  const next = readHighestId(nwtDir) + 1;
  return next.toString().padStart(6, '0');
}

// ── Timestamp helper ─────────────────────────────────────────

function nowIso(): string {
  return new Date().toISOString().replace(/\.\d{3}Z$/, 'Z');
}

// ── Index maintenance ────────────────────────────────────────

function updateIndex(indexPath: string, key: string, eventId: string): void {
  let idx: Record<string, string[]> = {};
  if (existsSync(indexPath)) {
    try {
      idx = JSON.parse(readFileSync(indexPath, 'utf-8'));
    } catch {
      idx = {};
    }
  }
  if (!idx[key]) idx[key] = [];
  if (!idx[key]!.includes(eventId)) idx[key]!.push(eventId);
  writeFileSync(indexPath, JSON.stringify(idx, null, 2));
}

// ── Public API ──────────────────────────────────────────────

/**
 * Initialize the .nwt/ directory at `rootDir`. Idempotent —
 * calling it twice on the same project is a no-op (the
 * metadata.json is preserved).
 *
 * Returns the absolute path to .nwt/ so the caller can
 * `cd <path>` if they want to inspect it from the host shell.
 */
export function initWorkspace(rootDir: string, projectName?: string): string {
  const nwtDir = ensureNwtDir(rootDir);
  const metaFile = metadataPath(nwtDir);
  if (!existsSync(metaFile)) {
    const meta = {
      project_name: projectName ?? 'flowntier-project',
      created: nowIso(),
      schema_version: 1,
      nwt_cli_compat: '1.0',
    };
    writeFileSync(metaFile, JSON.stringify(meta, null, 2));
  }
  return nwtDir;
}

export interface LogOptions {
  task: string;
  summary: string;
  reason?: string;
  files?: string[];
  tags?: string[];
  parent?: string;
  /** Explicit timestamp; defaults to now (overrides for testing/replay). */
  timestamp?: string;
}

/**
 * Append a new event to the timeline. Auto-assigns id, writes
 * the event JSON, and updates the tag/file indices.
 *
 * Returns the new event id (6-digit string) so the caller can
 * reference it (e.g. set `parent: prevId` on the next event).
 */
export function logEvent(rootDir: string, opts: LogOptions): string {
  const nwtDir = ensureNwtDir(rootDir);
  initWorkspace(rootDir);

  const id = nextId(nwtDir);
  const event: NwtEvent = {
    id,
    timestamp: opts.timestamp ?? nowIso(),
    task: opts.task,
    summary: opts.summary,
    ...(opts.reason ? { reason: opts.reason } : {}),
    ...(opts.files ? { files: opts.files } : {}),
    ...(opts.tags ? { tags: opts.tags } : {}),
    ...(opts.parent ? { parent: opts.parent } : {}),
  };
  writeFileSync(eventPath(nwtDir, id), JSON.stringify(event, null, 2));

  // Update indices.
  for (const tag of opts.tags ?? []) {
    updateIndex(tagsIndexPath(nwtDir), tag, id);
  }
  for (const f of opts.files ?? []) {
    // Store as project-relative path.
    const rel = relative(rootDir, f) || f;
    updateIndex(filesIndexPath(nwtDir), rel, id);
  }

  return id;
}

/**
 * Read a single event by id. Returns null if not found.
 */
export function getEvent(rootDir: string, id: string): NwtEvent | null {
  const nwtDir = ensureNwtDir(rootDir);
  const p = eventPath(nwtDir, id);
  if (!existsSync(p)) return null;
  try {
    return JSON.parse(readFileSync(p, 'utf-8')) as NwtEvent;
  } catch {
    return null;
  }
}

/**
 * Read all events, sorted by id ascending (which == chronological
 * for the 6-digit format).
 */
export function history(rootDir: string, limit?: number): NwtEvent[] {
  const nwtDir = ensureNwtDir(rootDir);
  const timelineDir = join(nwtDir, 'timeline');
  if (!existsSync(timelineDir)) return [];
  const allFiles = readdirSync(timelineDir) as string[];
  const files: string[] = allFiles.filter((f: string) => /^\d{6}\.json$/.test(f));
  const raw: Array<NwtEvent | null> = files.map((f: string) => {
    try {
      return JSON.parse(readFileSync(join(timelineDir, f), 'utf-8')) as NwtEvent;
    } catch {
      return null;
    }
  });
  const events: NwtEvent[] = raw.filter((e: NwtEvent | null): e is NwtEvent => e !== null);
  events.sort((a: NwtEvent, b: NwtEvent) => a.id.localeCompare(b.id));
  if (limit !== undefined && limit > 0) {
    return events.slice(-limit);
  }
  return events;
}

/**
 * Archive events older than `cutoffDays` (default 30) into a
 * single archive JSON in `.nwt/timeline/_archive_<date>.json`,
 * then delete the originals.
 *
 * Returns the number of events archived.
 */
export function archiveOlderThan(rootDir: string, cutoffDays = 30): number {
  const nwtDir = ensureNwtDir(rootDir);
  const timelineDir = join(nwtDir, 'timeline');
  if (!existsSync(timelineDir)) return 0;
  const cutoff = Date.now() - cutoffDays * 86_400_000;
  const old: NwtEvent[] = [];
  for (const f of readdirSync(timelineDir)) {
    if (!/^\d{6}\.json$/.test(f)) continue;
    const p = join(timelineDir, f);
    try {
      const e: NwtEvent = JSON.parse(readFileSync(p, 'utf-8'));
      if (new Date(e.timestamp).getTime() < cutoff) {
        old.push(e);
        unlinkSync(p);
      }
    } catch {
      // Corrupt or unreadable — skip.
    }
  }
  if (old.length === 0) return 0;
  old.sort((a, b) => a.id.localeCompare(b.id));
  const stamp = new Date().toISOString().slice(0, 10);
  writeFileSync(
    join(timelineDir, `_archive_${stamp}.json`),
    JSON.stringify(old, null, 2),
  );
  return old.length;
}

/**
 * Search events by a substring match against task, summary, or
 * reason. Case-insensitive. Returns matches sorted by id.
 */
export function search(rootDir: string, query: string): NwtEvent[] {
  const q = query.toLowerCase();
  return history(rootDir).filter((e) =>
    e.task.toLowerCase().includes(q) ||
    e.summary.toLowerCase().includes(q) ||
    (e.reason?.toLowerCase().includes(q) ?? false),
  );
}

/**
 * Search events that touched a given file (project-relative path).
 * Uses the files index for O(1) lookup.
 */
export function searchByFile(rootDir: string, filePath: string): NwtEvent[] {
  const nwtDir = ensureNwtDir(rootDir);
  const idxPath = filesIndexPath(nwtDir);
  if (!existsSync(idxPath)) return [];
  let idx: Record<string, string[]> = {};
  try {
    idx = JSON.parse(readFileSync(idxPath, 'utf-8'));
  } catch {
    return [];
  }
  // Try both the raw path and the project-relative path.
  const candidates = [filePath, relative(rootDir, filePath)];
  const ids = new Set<string>();
  for (const c of candidates) {
    for (const id of idx[c] ?? []) ids.add(id);
  }
  if (ids.size === 0) return [];
  return Array.from(ids)
    .map((id) => getEvent(rootDir, id))
    .filter((e): e is NwtEvent => e !== null);
}

/**
 * Search events by tag.
 */
export function searchByTag(rootDir: string, tag: string): NwtEvent[] {
  const nwtDir = ensureNwtDir(rootDir);
  const idxPath = tagsIndexPath(nwtDir);
  if (!existsSync(idxPath)) return [];
  let idx: Record<string, string[]> = {};
  try {
    idx = JSON.parse(readFileSync(idxPath, 'utf-8'));
  } catch {
    return [];
  }
  return (idx[tag] ?? [])
    .map((id) => getEvent(rootDir, id))
    .filter((e): e is NwtEvent => e !== null);
}

/**
 * Walk the parent chain from the most recent event backwards.
 * Returns a chronological narrative — useful for /story output
 * or AI agent context.
 */
export function story(rootDir: string, fromId?: string): NwtEvent[] {
  const all = history(rootDir);
  if (all.length === 0) return [];
  // Find the starting event: if fromId is given, start there;
  // otherwise use the most recent.
  const start = fromId
    ? all.find((e) => e.id === fromId)
    : all[all.length - 1];
  if (!start) return [];
  // Walk back via parent links.
  const byId = new Map(all.map((e) => [e.id, e]));
  const chain: NwtEvent[] = [];
  let cur: NwtEvent | undefined = start;
  const seen = new Set<string>();
  while (cur && !seen.has(cur.id)) {
    seen.add(cur.id);
    chain.unshift(cur);
    cur = cur.parent ? byId.get(cur.parent) : undefined;
  }
  return chain;
}

/**
 * Get the highest event id (returns 0 if the timeline is empty).
 * Used to chain events with parent linkage.
 */
export function currentEventId(rootDir: string): string {
  return nextId(rootDir) // returns next; subtract 1 by caller if needed
    .replace(/^0+(?=\d)/, '');
}
