import { Fragment, useState } from "react";
import { useTranslation } from "react-i18next";
import { saveSecret, addCustomProvider } from "../../lib/api.js";
import type { ModelRow, ThinkingStrength } from "./types.js";
import { THINKING_OPTIONS } from "./constants.js";

export function CustomProviderForm({ onSaved }: { onSaved: () => void }) {
  const { t } = useTranslation();
  const KIND_OPTIONS = [
    { value: "anthropic", label: t("settings.custom.kind.anthropic") },
    { value: "openai", label: t("settings.custom.kind.openai") },
    { value: "openai_compat", label: "OpenAI 兼容 (AI SDK)" },
  ] as const;
  // BUG-FRONTEND-RT-12 (event 000041): each model now carries
  // its own `context_length` (free-text int) + `thinking_strength`
  // (one of 'low' | 'medium' | 'high'). Lets the chairman set
  // per-model values (e.g. 200k context + high thinking for the
  // flagship MiniMax-M3, 8k + low for a cheap fast model).
  const [open, setOpen] = useState(false);
  const [id, setId] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [kind, setKind] = useState<"anthropic" | "openai" | "openai_compat">("openai");
  const [baseUrl, setBaseUrl] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [models, setModels] = useState<ModelRow[]>([]);
  const [newModelId, setNewModelId] = useState("");
  const [newModelName, setNewModelName] = useState("");
  const [newModelContext, setNewModelContext] = useState("");
  // v0.4.22 (fix 4): default to '' ("未设置") so the user can
  // skip both fields entirely and let the runtime pick the
  // provider default. The previous default of 'medium' was
  // indistinguishable from a deliberate choice in the dropdown.
  const [newModelThinking, setNewModelThinking] = useState<ThinkingStrength>("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState(false);
  // v0.4.30 (audit 000130): which row of the local `models`
  // array is in inline-edit mode. Editing here mutates the
  // local state only — the final values are POSTed when the
  // user clicks 保存 (handleSubmit).
  const [editingModelId, setEditingModelId] = useState<string | null>(null);

  const reset = () => {
    setId("");
    setDisplayName("");
    setKind("openai");
    setBaseUrl("");
    setApiKey("");
    setModels([]);
    setNewModelId("");
    setNewModelName("");
    // v0.4.22 (fix 4): reset back to the empty / unset default
    // so the chairman's next entry is also "未设置", not the
    // previously-defaulted medium.
    setNewModelContext("");
    setNewModelThinking("");
    setError(null);
    setSuccess(false);
  };

  const addModelRow = () => {
    const mid = newModelId.trim();
    if (!mid) return;
    if (models.some((m) => m.id === mid)) {
      setError(t("settings.models.modelExists", { id: mid }));
      return;
    }
    // BUG-FRONTEND-RT-12: validate the free-text context length.
    // Accept empty (= use provider default) or a positive int.
    // Cap at 10M to keep the JSON reasonable.
    let ctx: number | null = null;
    const ctxRaw = newModelContext.trim();
    if (ctxRaw.length > 0) {
      const n = Number(ctxRaw);
      if (!Number.isFinite(n) || n <= 0 || n > 10_000_000) {
        setError(t("settings.models.invalidContextLength"));
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
    // v0.4.22 (fix 4): after adding the row, reset thinking
    // back to '' so consecutive adds don't carry over a stale
    // value. newModelContext is already reset below.
    setNewModelId("");
    setNewModelName("");
    setNewModelContext("");
    setNewModelThinking("");
    setError(null);
  };

  const removeModelRow = (mid: string) => setModels(models.filter((m) => m.id !== mid));

  const handleSubmit = async () => {
    // Validate
    const idTrim = id.trim().toLowerCase();
    if (!/^[a-z0-9_]+$/.test(idTrim)) {
      setError(t("settings.error.invalidId"));
      return;
    }
    if (!displayName.trim()) {
      setError(t("settings.quickAdd.errorMissingName"));
      return;
    }
    if (!baseUrl.trim() || !(baseUrl.startsWith("http://") || baseUrl.startsWith("https://"))) {
      setError(t("settings.error.invalidBaseUrl"));
      return;
    }
    if (!apiKey.trim()) {
      setError(t("settings.error.missingApiKey"));
      return;
    }
    if (models.length === 0) {
      setError(
        t("settings.quickAdd.errorMissingKey").replace(
          "Please enter an API key",
          "Please add at least one model",
        ),
      );
      return;
    }

    setBusy(true);
    setError(null);
    try {
      // 1. Save API key to keychain. v0.4.30 (audit 000130): use
      // the `flowntier/custom/<id>` internal namespace so the
      //    stored secret name cannot be confused with shell env
      //    vars. The Rust side reads with the same convention
      //    (see handlers.rs::resolve_role_provider).
      const envVarName = `flowntier/custom/${idTrim.toLowerCase()}`;
      const saveResult = await saveSecret(envVarName, apiKey.trim());
      if (!saveResult || !saveResult.saved) {
        setError(t("settings.error.saveFailed"));
        setBusy(false);
        return;
      }

      // 2. Register custom provider
      // v0.4.22 (fix 4): strip empty thinking_strength from
      // each model row so the backend doesn't persist a forced
      // default. context_length is already nullable, so we
      // just pass through.
      const modelsPayload = models.map((m) => {
        const base: {
          id: string;
          display_name: string;
          context_length: number | null;
          thinking_strength?: "low" | "medium" | "high";
        } = {
          id: m.id,
          display_name: m.display_name,
          context_length: m.context_length,
        };
        if (m.thinking_strength) base.thinking_strength = m.thinking_strength;
        return base;
      });
      await addCustomProvider({
        id: idTrim,
        display_name: displayName.trim(),
        kind,
        base_url: baseUrl.trim().replace(/\/+$/, ""),
        api_key_env: envVarName,
        models: modelsPayload,
      });

      setSuccess(true);
      onSaved();
      setTimeout(() => {
        setOpen(false);
        reset();
      }, 1200);
    } catch (e) {
      setError(e instanceof Error ? e.message : t("settings.error.saveFailed"));
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
        {t("settings.error.customAdd")}
      </button>
    );
  }

  return (
    <div className="rounded-lg border border-border bg-surface-1 p-4">
      <div className="mb-3 flex items-center justify-between">
        <h3 className="text-sm font-semibold text-primary">{t("settings.customProvider.title")}</h3>
        <button
          type="button"
          onClick={() => {
            setOpen(false);
            reset();
          }}
          className="text-xs text-text-secondary hover:text-primary"
        >
          {t("settings.action.cancel")}
        </button>
      </div>

      <div className="flex flex-col gap-2.5 text-xs">
        {/* ID */}
        <label className="flex flex-col gap-1">
          <span className="text-text-secondary">
            ID <span className="text-[10px]">(英文+数字+下划线)</span>
          </span>
          <input
            value={id}
            onChange={(e) => setId(e.target.value)}
            placeholder={t("settings.custom.idPlaceholder")}
            className="rounded border border-border bg-surface-2 px-2 py-1.5 text-primary outline-none focus:border-chief"
          />
        </label>

        {/* Display Name */}
        <label className="flex flex-col gap-1">
          <span className="text-text-secondary">{t("settings.custom.nameLabel")}</span>
          <input
            value={displayName}
            onChange={(e) => setDisplayName(e.target.value)}
            placeholder={t("settings.custom.namePlaceholder")}
            className="rounded border border-border bg-surface-2 px-2 py-1.5 text-primary outline-none focus:border-chief"
          />
        </label>

        {/* Protocol */}
        <label className="flex flex-col gap-1">
          <span className="text-text-secondary">{t("settings.custom.kindLabel")}</span>
          <select
            value={kind}
            onChange={(e) => setKind(e.target.value as typeof kind)}
            className="rounded border border-border bg-surface-2 px-2 py-1.5 text-primary outline-none focus:border-chief"
          >
            {KIND_OPTIONS.map((o) => (
              <option key={o.value} value={o.value}>
                {o.label}
              </option>
            ))}
          </select>
        </label>

        {/* Base URL */}
        <label className="flex flex-col gap-1">
          <span className="text-text-secondary">Base URL</span>
          <input
            value={baseUrl}
            onChange={(e) => setBaseUrl(e.target.value)}
            placeholder={t("settings.custom.baseUrlPlaceholder")}
            className="rounded border border-border bg-surface-2 px-2 py-1.5 text-primary outline-none focus:border-chief"
          />
        </label>

        {/* API Key label (i18n) */}
        <label className="flex flex-col gap-1">
          <span className="text-text-secondary">{t("settings.custom.apiKeyLabel")}</span>
          <input
            type="password"
            value={apiKey}
            onChange={(e) => setApiKey(e.target.value)}
            placeholder={t("settings.custom.apiKeyPlaceholder")}
            className="rounded border border-border bg-surface-2 px-2 py-1.5 text-primary outline-none focus:border-chief"
          />
        </label>

        {/* Models */}
        <div className="flex flex-col gap-1">
          <span className="text-text-secondary">{t("settings.models.list")}</span>
          {models.length > 0 && (
            <ul className="flex flex-col gap-1">
              {models.map((m) => {
                const isEditing = editingModelId === m.id;
                return (
                  <Fragment key={m.id}>
                    <li className="flex items-center gap-2 rounded bg-surface-2 px-2 py-1 text-[11px]">
                      <span className="min-w-0 flex-1 truncate text-primary" title={m.id}>
                        {m.display_name}{" "}
                        <span className="text-[10px] text-text-secondary">({m.id})</span>
                      </span>
                      <span className="shrink-0 text-[10px] text-text-secondary">
                        {m.context_length
                          ? `${m.context_length.toLocaleString()} ${t("settings.models.tokens")}`
                          : t("settings.models.defaultContext")}
                      </span>
                      <span className="shrink-0 rounded bg-surface-3 px-1.5 py-0.5 text-[10px] text-text-primary">
                        {t(`settings.models.thinking.${m.thinking_strength}`)}
                      </span>
                      {/* v0.4.30: edit display_name / context_length /
                          thinking_strength inline before POSTing. */}
                      <button
                        type="button"
                        onClick={() => setEditingModelId(isEditing ? null : m.id)}
                        title={String(t("settings.models.editRow", { name: m.display_name }))}
                        className="shrink-0 text-[10px] text-text-secondary hover:text-primary"
                      >
                        ✎
                      </button>
                      <button
                        type="button"
                        onClick={() => removeModelRow(m.id)}
                        className="shrink-0 text-[10px] text-status-failed hover:text-status-failed"
                      >
                        ✕
                      </button>
                    </li>
                    {isEditing && (
                      <li className="flex flex-wrap items-center gap-1.5 rounded border border-border bg-surface-1 px-2 py-1.5 text-[11px]">
                        <input
                          value={m.display_name}
                          onChange={(e) =>
                            setModels(
                              models.map((row) =>
                                row.id === m.id ? { ...row, display_name: e.target.value } : row,
                              ),
                            )
                          }
                          placeholder={String(t("settings.models.displayNamePlaceholder"))}
                          className="min-w-[120px] flex-1 rounded border border-border bg-surface-2 px-2 py-1 text-primary outline-none focus:border-chief"
                        />
                        <input
                          type="number"
                          min={0}
                          value={m.context_length != null ? String(m.context_length) : ""}
                          onChange={(e) =>
                            setModels(
                              models.map((row) =>
                                row.id === m.id
                                  ? {
                                      ...row,
                                      context_length:
                                        e.target.value.trim() === ""
                                          ? null
                                          : Number(e.target.value),
                                    }
                                  : row,
                              ),
                            )
                          }
                          placeholder={String(t("settings.models.contextLengthPlaceholder"))}
                          className="w-[110px] rounded border border-border bg-surface-2 px-2 py-1 font-mono outline-none focus:border-chief"
                        />
                        <select
                          value={m.thinking_strength}
                          onChange={(e) =>
                            setModels(
                              models.map((row) =>
                                row.id === m.id
                                  ? {
                                      ...row,
                                      thinking_strength: e.target.value as ThinkingStrength,
                                    }
                                  : row,
                              ),
                            )
                          }
                          className="rounded border border-border bg-surface-2 px-2 py-1 outline-none focus:border-chief"
                        >
                          <option value="">{t("settings.models.thinkingUnset")}</option>
                          <option value="low">{t("settings.models.thinking.low")}</option>
                          <option value="medium">{t("settings.models.thinking.medium")}</option>
                          <option value="high">{t("settings.models.thinking.high")}</option>
                        </select>
                        <button
                          type="button"
                          onClick={() => setEditingModelId(null)}
                          className="shrink-0 rounded bg-primary px-2 py-1 text-[10px] text-white hover:bg-primary/90"
                        >
                          {t("settings.models.saveOverride")}
                        </button>
                      </li>
                    )}
                  </Fragment>
                );
              })}
            </ul>
          )}
          {/* Per-model fields. Per the chairman: model name + display
              name + context length (free text) + thinking strength
              (dropdown). All four on one row to save space. */}
          <div className="flex flex-wrap items-center gap-1.5">
            <input
              value={newModelId}
              onChange={(e) => setNewModelId(e.target.value)}
              placeholder={t("settings.models.newModelId")}
              className="flex-1 min-w-[80px] rounded border border-border bg-surface-2 px-2 py-1 text-primary outline-none focus:border-chief"
            />
            <input
              value={newModelName}
              onChange={(e) => setNewModelName(e.target.value)}
              placeholder={t("settings.models.newModelName")}
              className="flex-1 min-w-[80px] rounded border border-border bg-surface-2 px-2 py-1 text-primary outline-none focus:border-chief"
            />
            <input
              value={newModelContext}
              onChange={(e) => setNewModelContext(e.target.value)}
              placeholder={t("settings.models.contextPlaceholder")}
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
                <option key={o.value} value={o.value}>
                  {t(o.labelKey)}
                </option>
              ))}
            </select>
            <button
              type="button"
              onClick={addModelRow}
              className="shrink-0 rounded bg-surface-3 px-2 py-1 text-text-secondary hover:text-primary"
            >
              +
            </button>
          </div>
          <p className="text-[10px] text-text-secondary">{t("settings.models.contextHint")}</p>
        </div>
      </div>

      {error && (
        <p role="alert" aria-live="polite" className="mt-2 text-[11px] text-status-failed">
          {error}
        </p>
      )}
      {success && (
        <p role="status" aria-live="polite" className="mt-2 text-[11px] text-status-done">
          {t("settings.error.alreadyAdded")}
        </p>
      )}

      <div className="mt-3 flex justify-end">
        <button
          type="button"
          onClick={handleSubmit}
          disabled={busy}
          className="rounded-md bg-chief px-4 py-1.5 text-xs font-medium text-white hover:bg-chief/90 disabled:opacity-30"
        >
          {busy ? t("settings.action.save") : t("settings.action.create")}
        </button>
      </div>
    </div>
  );
}
