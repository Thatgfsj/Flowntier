import { useState } from "react";
import { useTranslation } from "react-i18next";
import { fetchProviderModels } from "../../lib/api.js";
import type { ProviderModel } from "../../lib/api.js";

// ── Provider model manager (cc-switch style) ─────────────────────

export interface ProviderModelManagerProps {
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
export function ProviderModelManager({
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
  // v0.4.17: inline manual-model-add form. Lets the chairman
  // register models they actually use without depending on the
  // provider's /v1/models endpoint (which keeps changing).
  const [manualOpen, setManualOpen] = useState(false);
  const [manualId, setManualId] = useState("");
  const [manualDisplay, setManualDisplay] = useState("");
  // v0.4.22 (fix 4): '' = 未设置. Defaulted to '' so the
  // chairman isn't forced to pick a thinking strength on every
  // model entry.
  const [manualThinking, setManualThinking] = useState<"" | "low" | "medium" | "high">("");
  // context_length is nullable; '' = not specified.
  const [manualContext, setManualContext] = useState<number | null>(null);
  const [manualErr, setManualErr] = useState<string | null>(null);

  const submitManual = () => {
    setManualErr(null);
    const idTrim = manualId.trim();
    if (!idTrim) {
      setManualErr(t("settings.error.invalidId"));
      return;
    }
    if (!/^[a-zA-Z0-9._:-]+$/.test(idTrim)) {
      setManualErr(t("settings.error.invalidId"));
      return;
    }
    const display = manualDisplay.trim() || idTrim;
    if (customModels.some((m) => m.id === idTrim)) {
      setManualErr(t("settings.models.modelExists", { id: idTrim }));
      return;
    }
    // v0.4.22 (fix 4): context_length is now optional. null
    // (= "未设置") is fine; only validate when the user typed
    // something.
    let ctx: number | null = null;
    if (manualContext !== null) {
      if (!Number.isFinite(manualContext) || manualContext < 1 || manualContext > 2_000_000) {
        setManualErr(t("settings.models.invalidContextLength"));
        return;
      }
      ctx = manualContext;
    }
    // thinking_strength is also optional — pass it through
    // only when the user picked one.
    const row: ProviderModel = {
      id: idTrim,
      display_name: display,
      context_length: ctx,
    };
    if (manualThinking) row.thinking_strength = manualThinking;
    onAdd([row]);
    // Reset form for next entry.
    setManualId("");
    setManualDisplay("");
    setManualThinking("");
    setManualContext(null);
    setManualOpen(false);
  };

  const pull = async () => {
    setBusy(true);
    setError(null);
    setFetched([]);
    setPicked(new Set());
    setOpen(true);
    try {
      const res = await fetchProviderModels(providerId);
      if (!res.ok) {
        // v0.4.17: backend now ALWAYS returns ok:true/false with
        // an explicit error message. Show the real message; fall
        // back to the i18n string only as a defensive last resort.
        const backendErr = res.error ?? t("settings.models.pullError");
        const detail = res.url ? ` (${res.url})` : "";
        setError(`${backendErr}${detail}`);
        console.warn("[Flowntier] pull models failed", res);
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
          {t("settings.models.customModels", { count: customModels.length })}
        </h4>
        <div className="flex items-center gap-1">
          <button
            type="button"
            onClick={() => setManualOpen((v) => !v)}
            className="rounded-md border border-border bg-surface-1 px-2 py-0.5 text-[11px] text-text-secondary hover:border-chief hover:text-chief"
          >
            ➕ {t("settings.models.manualAdd.title")}
          </button>
          <button
            type="button"
            onClick={() => void pull()}
            disabled={busy}
            className="rounded-md border border-chief/40 bg-chief/10 px-2 py-0.5 text-[11px] text-chief hover:bg-chief/20 disabled:opacity-50"
          >
            {busy ? t("settings.action.save") : "🔄 " + t("settings.providers.discoverModels")}
          </button>
        </div>
      </div>

      {manualOpen && (
        <div className="mb-2 rounded border border-border bg-surface-1 p-2 text-xs">
          <div className="mb-1 font-medium">{t("settings.models.manualAdd.title")}</div>
          <div className="grid grid-cols-2 gap-2">
            <label className="flex flex-col gap-0.5">
              <span className="text-text-secondary">{t("settings.models.manualAdd.id")}</span>
              <input
                type="text"
                value={manualId}
                onChange={(e) => setManualId(e.target.value)}
                placeholder="MiniMax-Text-01"
                className="rounded border border-border bg-surface-0 px-1.5 py-0.5 font-mono text-[11px]"
              />
            </label>
            <label className="flex flex-col gap-0.5">
              <span className="text-text-secondary">
                {t("settings.models.manualAdd.displayName")}
              </span>
              <input
                type="text"
                value={manualDisplay}
                onChange={(e) => setManualDisplay(e.target.value)}
                placeholder={manualId || "MiniMax M3"}
                className="rounded border border-border bg-surface-0 px-1.5 py-0.5 text-[11px]"
              />
            </label>
            <label className="flex flex-col gap-0.5">
              <span className="text-text-secondary">{t("settings.models.manualAdd.thinking")}</span>
              <select
                value={manualThinking}
                onChange={(e) =>
                  setManualThinking(e.target.value as "" | "low" | "medium" | "high")
                }
                className="rounded border border-border bg-surface-0 px-1.5 py-0.5 text-[11px]"
              >
                {/* v0.4.22 (fix 4): empty value = "未设置".
                    The chairman asked us to stop forcing a
                    thinking strength on every model entry. */}
                <option value="">
                  {t("settings.models.thinking.default", { defaultValue: "未设置" })}
                </option>
                <option value="low">low</option>
                <option value="medium">medium</option>
                <option value="high">high</option>
              </select>
            </label>
            <label className="flex flex-col gap-0.5">
              <span className="text-text-secondary">{t("settings.models.manualAdd.context")}</span>
              <input
                type="number"
                min={1}
                max={2000000}
                // v0.4.22 (fix 4): empty string when unset so
                // the input doesn't show "0" by default and
                // the user can leave the field blank.
                value={manualContext ?? ""}
                onChange={(e) => {
                  const v = e.target.value;
                  if (v === "") {
                    setManualContext(null);
                    return;
                  }
                  const n = Number(v);
                  setManualContext(Number.isFinite(n) ? n : null);
                }}
                placeholder={t("settings.models.contextPlaceholder")}
                className="rounded border border-border bg-surface-0 px-1.5 py-0.5 text-[11px]"
              />
            </label>
          </div>
          {manualErr && (
            <p role="alert" className="mt-1 text-[11px] text-status-failed">
              {manualErr}
            </p>
          )}
          <div className="mt-2 flex items-center justify-end gap-2">
            <button
              type="button"
              onClick={() => {
                setManualOpen(false);
                setManualErr(null);
              }}
              className="rounded px-2 py-0.5 text-[11px] text-text-secondary hover:text-primary"
            >
              {t("settings.action.cancel")}
            </button>
            <button
              type="button"
              onClick={submitManual}
              className="rounded bg-chief px-2 py-0.5 text-[11px] text-white hover:bg-chief/90"
            >
              {t("settings.models.manualAdd.submit")}
            </button>
          </div>
        </div>
      )}

      {customModels.length === 0 ? (
        <div className="text-[11px] text-text-secondary">
          {t("settings.models.emptyCustomModels", { provider: providerDisplay })}
        </div>
      ) : (
        <ul className="grid grid-cols-2 gap-1 text-xs">
          {customModels.map((m) => (
            <li
              key={m.id}
              className="flex items-center gap-1 rounded bg-surface-1 px-2 py-1 font-mono"
            >
              <span className="flex-1 truncate" title={m.id}>
                {m.display_name}
              </span>
              <button
                type="button"
                onClick={() => onRemove(m.id)}
                className="text-[10px] text-text-secondary hover:text-status-failed"
                aria-label={t("settings.action.remove")}
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
          {t("settings.models.clearAll")}
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
                {t("settings.models.pullTitle", { provider: providerDisplay })}
              </h3>
              <button
                type="button"
                onClick={() => setOpen(false)}
                disabled={busy}
                className="text-text-secondary hover:text-primary disabled:opacity-30"
                aria-label={t("settings.action.close")}
              >
                ×
              </button>
            </header>

            <div className="flex-1 overflow-y-auto p-4">
              {busy ? (
                <div className="flex items-center justify-center py-8 text-sm text-text-secondary">
                  {t("settings.models.callingApi", { provider: providerDisplay })}
                </div>
              ) : error ? (
                <div
                  role="alert"
                  aria-live="polite"
                  className="rounded border border-status-failed/40 bg-status-failed/10 p-3 text-xs text-status-failed"
                >
                  {error}
                </div>
              ) : fetched.length === 0 ? (
                <div className="py-4 text-sm text-text-secondary">
                  {t("settings.models.noModels")}
                </div>
              ) : (
                <>
                  <div className="mb-2 text-[11px] text-text-secondary">
                    {t("settings.models.foundCount", { count: fetched.length })}
                  </div>
                  <div className="mb-3 flex items-center gap-2">
                    <button
                      type="button"
                      onClick={() => setPicked(new Set(fetched.map((m) => m.id)))}
                      className="text-[11px] text-chief hover:underline"
                    >
                      {t("settings.models.all")}
                    </button>
                    <span className="text-text-secondary">·</span>
                    <button
                      type="button"
                      onClick={() => setPicked(new Set())}
                      className="text-[11px] text-text-secondary hover:underline"
                    >
                      {t("settings.models.none")}
                    </button>
                    <span className="ml-auto text-[11px] text-text-secondary">
                      {t("settings.models.selectedCount", { count: picked.size })}
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
                            picked_ ? "border-chief bg-chief/10" : "border-border bg-surface-2"
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
                              {have && (
                                <span className="ml-1 text-[10px] text-status-done">
                                  {t("settings.models.alreadyAdded")}
                                </span>
                              )}
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
                {t("settings.models.addSelected", { count: picked.size })}
              </button>
            </footer>
          </div>
        </div>
      )}
    </div>
  );
}
