/**
 * EditorPane — v0.4.36 in-app workspace file editor.
 *
 * Backed by CodeMirror 6 (basicSetup + oneDark). Loaded via
 * getWorkspaceFile; saved via writeWorkspaceFile with the
 * mtime round-trip that guards against clobbering concurrent
 * agent writes (conflict:true → the user chooses "overwrite
 * with my version" or "reload from disk").
 *
 * Safety posture (per spec): the pane opens READ-ONLY. Editing
 * requires an explicit click on "编辑"; Ctrl+S / Cmd+S saves and
 * echoes the mtime read at load time back to the server, so a
 * stale buffer can never silently overwrite a file that changed
 * underneath us.
 *
 * Lifecycle: one EditorView per `path`. When the path changes
 * (or the user hits "重新加载" after a conflict) the old view is
 * destroyed and a fresh one is built from the re-read file; the
 * view is also destroyed on unmount. The save handler lives in
 * a ref so the keymap/effect wiring never needs to be rebuilt
 * when callbacks are re-created by renders.
 */
import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { basicSetup } from "codemirror";
import { EditorView, keymap } from "@codemirror/view";
import { Compartment, EditorState } from "@codemirror/state";
import { indentWithTab } from "@codemirror/commands";
import { rust } from "@codemirror/lang-rust";
import { javascript } from "@codemirror/lang-javascript";
import { json } from "@codemirror/lang-json";
import { markdown } from "@codemirror/lang-markdown";
import { python } from "@codemirror/lang-python";
import { oneDark } from "@codemirror/theme-one-dark";
import {
  getWorkspaceFile,
  writeWorkspaceFile,
  type WorkspaceFile,
  type WorkspaceFileWriteResult,
} from "../lib/api.js";
import { extOf, pickLanguageSupport, type EditorLanguageKind } from "./editorUtil.js";

export interface EditorPaneProps {
  /** Workspace-relative path of the file to open. */
  path: string;
  onClose: () => void;
}

/** Map a language kind onto the corresponding bundled extension. */
function languageExtension(kind: EditorLanguageKind | null) {
  switch (kind) {
    case "rust":
      return rust();
    case "javascript":
      // tsx variant: jsx + typescript both on so .tsx parses.
      return javascript({ jsx: true, typescript: true });
    case "json":
      return json();
    case "markdown":
      return markdown();
    case "python":
      return python();
    case null:
      return [];
  }
}

/** Extract a compact display title: keep the last 3 path segments. */
function displayPath(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  if (parts.length <= 3) return path;
  return "…/" + parts.slice(-3).join("/");
}

export function EditorPane({ path, onClose }: EditorPaneProps) {
  const { t } = useTranslation();
  const containerRef = useRef<HTMLDivElement | null>(null);
  const viewRef = useRef<EditorView | null>(null);
  // Baseline doc of the last load/save: dirty = current doc differs.
  const baselineDocRef = useRef<EditorState["doc"] | null>(null);
  // Disk mtime at last load/save; echoed back on write (conflict guard).
  const mtimeRef = useRef<number | null>(null);
  const readonlyCompartmentRef = useRef(new Compartment());

  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [editable, setEditable] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);
  const [conflict, setConflict] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  // Bumped by the "reload from disk" action; re-runs the load effect
  // so the view is destroyed and rebuilt from fresh disk content.
  const [reloadNonce, setReloadNonce] = useState(0);

  // ── Save ─────────────────────────────────────────────────────
  const doSave = useCallback(async () => {
    const view = viewRef.current;
    if (!view || loading || saving) return;
    setSaving(true);
    setSaveError(null);
    try {
      const content = view.state.doc.toString();
      const result: WorkspaceFileWriteResult = await writeWorkspaceFile({
        path,
        content,
        // exactOptionalPropertyTypes: `number | null` matches the
        // declared `?: number | null` slot, no undefined involved.
        expected_mtime_ms: mtimeRef.current,
      });
      if (result.ok) {
        if (typeof result.mtime_ms === "number") mtimeRef.current = result.mtime_ms;
        baselineDocRef.current = view.state.doc;
        setDirty(false);
        setConflict(false);
      } else if (result.conflict) {
        setConflict(true);
      } else if (result.readonly) {
        setSaveError(t("editor.errorReadonlyFile"));
      } else {
        setSaveError(result.error ?? t("editor.errorSave", { error: "-" }));
      }
    } catch (e) {
      setSaveError(t("editor.errorSave", { error: String(e) }));
    } finally {
      setSaving(false);
    }
  }, [loading, path, saving, t]);

  // Ref indirection so the CodeMirror keymap + the window keydown
  // listener always call the latest doSave without re-creating the
  // EditorView on every render.
  const saveRef = useRef<() => void>(() => {});
  saveRef.current = () => {
    void doSave();
  };

  // Same indirection for `t`: the load effect must NOT re-run when
  // the language toggles (that would destroy unsaved edits), but
  // error messages produced inside the effect should still be
  // translated in the active language.
  const tRef = useRef(t);
  tRef.current = t;

  // ── Ctrl+S / Cmd+S (window-level so it works even when the
  //    editor itself isn't focused; we deliberately do NOT also
  //    bind Mod-s in the keymap to avoid double-fire). ──────────
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && !e.altKey && !e.shiftKey && e.key.toLowerCase() === "s") {
        e.preventDefault();
        saveRef.current();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  // ── Load + view lifecycle (per path / reload) ────────────────
  // NOTE: the container div below is ALWAYS mounted (loading and
  // error states overlay it), so containerRef.current is non-null
  // on the very first effect run.
  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;
    let disposed = false;
    let view: EditorView | null = null;

    setLoading(true);
    setLoadError(null);
    setDirty(false);
    setConflict(false);
    setSaveError(null);
    setEditable(false);
    baselineDocRef.current = null;
    mtimeRef.current = null;

    void (async () => {
      try {
        const file: WorkspaceFile = await getWorkspaceFile(path);
        if (disposed) return;
        if (!file.ok) {
          setLoadError(file.error ?? tRef.current("editor.errorLoad", { error: "-" }));
          setLoading(false);
          return;
        }
        mtimeRef.current = typeof file.mtime_ms === "number" ? file.mtime_ms : null;
        const compartment = readonlyCompartmentRef.current;
        view = new EditorView({
          parent: container,
          state: EditorState.create({
            doc: file.content,
            extensions: [
              basicSetup,
              languageExtension(pickLanguageSupport(extOf(path))),
              oneDark,
              compartment.of([EditorState.readOnly.of(true), EditorView.editable.of(false)]),
              keymap.of([indentWithTab]),
              EditorView.updateListener.of((u) => {
                if (!u.docChanged) return;
                const baseline = baselineDocRef.current;
                if (!baseline) return;
                setDirty(!u.state.doc.eq(baseline));
              }),
            ],
          }),
        });
        viewRef.current = view;
        baselineDocRef.current = view.state.doc;
      } catch (e) {
        if (!disposed) setLoadError(tRef.current("editor.errorLoad", { error: String(e) }));
      } finally {
        if (!disposed) setLoading(false);
      }
    })();

    return () => {
      disposed = true;
      // Destroy the view for the OLD path/nonce; the next effect
      // run creates a fresh one.
      if (view) view.destroy();
      if (viewRef.current === view) viewRef.current = null;
    };
  }, [path, reloadNonce]);

  // ── Readonly / editable toggle ───────────────────────────────
  const toggleEditable = useCallback(() => {
    const view = viewRef.current;
    if (!view) return;
    setEditable((prev) => {
      const next = !prev;
      view.dispatch({
        effects: readonlyCompartmentRef.current.reconfigure([
          EditorState.readOnly.of(!next),
          EditorView.editable.of(next),
        ]),
      });
      return next;
    });
  }, []);

  // ── Conflict actions ─────────────────────────────────────────
  // "用磁盘版本覆盖": write again WITHOUT expected_mtime_ms, per
  // spec — the chairman has seen the conflict and chosen to
  // clobber the disk version with the buffer content.
  const overwriteDisk = useCallback(async () => {
    const view = viewRef.current;
    if (!view || saving) return;
    setSaving(true);
    setSaveError(null);
    try {
      const result = await writeWorkspaceFile({ path, content: view.state.doc.toString() });
      if (result.ok) {
        if (typeof result.mtime_ms === "number") mtimeRef.current = result.mtime_ms;
        baselineDocRef.current = view.state.doc;
        setDirty(false);
        setConflict(false);
      } else {
        setSaveError(result.error ?? t("editor.errorSave", { error: "-" }));
      }
    } catch (e) {
      setSaveError(t("editor.errorSave", { error: String(e) }));
    } finally {
      setSaving(false);
    }
  }, [path, saving, t]);

  const reloadFromDisk = useCallback(() => {
    setConflict(false);
    setReloadNonce((n) => n + 1);
  }, []);

  const dirtyDot = dirty ? <span className="text-status-failed">●</span> : null;

  return (
    <section
      className="flex h-full min-h-0 flex-col overflow-hidden rounded-lg border border-border bg-surface-1 shadow-sm"
      aria-label={t("editor.aria")}
    >
      {/* Header: title + dirty dot + actions */}
      <header className="flex shrink-0 items-center gap-2 border-b border-border bg-surface-2 px-3 py-1.5">
        <h2 className="min-w-0 truncate font-mono text-xs text-text-primary" title={path}>
          {displayPath(path)} {dirtyDot}
        </h2>
        <span className="ml-auto flex shrink-0 items-center gap-1">
          <button
            type="button"
            onClick={() => {
              void doSave();
            }}
            disabled={loading || saving}
            className="rounded border border-chief/40 bg-chief/10 px-2 py-0.5 text-[11px] text-chief hover:bg-chief/20 disabled:opacity-50"
            title={t("editor.saveTitle")}
          >
            {saving ? t("editor.saving") : t("editor.save")}
          </button>
          <button
            type="button"
            onClick={toggleEditable}
            disabled={loading}
            className={
              "rounded border px-2 py-0.5 text-[11px] disabled:opacity-50 " +
              (editable
                ? "border-status-failed/50 bg-status-failed/10 text-status-failed hover:bg-status-failed/20"
                : "border-border bg-surface-2 text-text-secondary hover:bg-surface-3")
            }
            title={editable ? t("editor.lockTitle") : t("editor.editTitle")}
          >
            {editable ? t("editor.lock") : t("editor.edit")}
          </button>
          <button
            type="button"
            onClick={onClose}
            className="rounded border border-border bg-surface-2 px-2 py-0.5 text-[11px] text-text-secondary hover:bg-surface-3"
            title={t("editor.closeTitle")}
          >
            {t("editor.close")}
          </button>
        </span>
      </header>

      {/* Conflict banner */}
      {conflict && (
        <div className="flex shrink-0 flex-wrap items-center gap-2 border-b border-status-failed/40 bg-status-failed/10 px-3 py-1.5 text-[11px] text-status-failed">
          <span>{t("editor.conflictHint")}</span>
          <button
            type="button"
            onClick={() => {
              void overwriteDisk();
            }}
            disabled={saving}
            className="rounded border border-status-failed/50 bg-surface-2 px-2 py-0.5 text-[11px] text-status-failed hover:bg-status-failed/20 disabled:opacity-50"
          >
            {t("editor.overwriteDisk")}
          </button>
          <button
            type="button"
            onClick={reloadFromDisk}
            disabled={saving}
            className="rounded border border-border bg-surface-2 px-2 py-0.5 text-[11px] text-text-secondary hover:bg-surface-3 disabled:opacity-50"
          >
            {t("editor.reload")}
          </button>
        </div>
      )}

      {/* Save error (non-conflict) */}
      {saveError && (
        <div className="shrink-0 border-b border-status-failed/40 bg-status-failed/10 px-3 py-1 text-[11px] text-status-failed">
          {saveError}
        </div>
      )}

      {/* Body — the CodeMirror container is ALWAYS mounted so the
          load effect can attach the view immediately; loading /
          error states are overlays on top of it. */}
      <div className="relative min-h-0 flex-1">
        {loadError && (
          <div className="absolute inset-0 overflow-auto p-3 text-xs text-status-failed">
            {loadError}
          </div>
        )}
        {loading && !loadError && (
          <div className="absolute inset-0 overflow-auto p-3 text-xs text-text-secondary">
            {t("editor.loading")}
          </div>
        )}
        <div
          ref={containerRef}
          className="h-full min-h-0 overflow-auto text-xs"
          // CodeMirror injects its own content styling; keep the
          // pane height bounded so the strip below stays visible.
          data-testid="editor-cm-container"
        />
      </div>

      {!loadError && !loading && (
        <footer className="flex shrink-0 items-center justify-between border-t border-border bg-surface-2 px-3 py-1 text-[10px] text-text-secondary">
          <span>{editable ? t("editor.modeEditing") : t("editor.modeReadonly")}</span>
          <span>{t("editor.saveHint")}</span>
        </footer>
      )}
    </section>
  );
}
