import { useState } from "react";
import { useTranslation } from "react-i18next";
import { setModelOverride, clearModelOverride } from "../../lib/api.js";
import type { ProviderModel } from "../../lib/api.js";

// ── ModelOverrideEditor ───────────────────────────────────────────
// v0.4.30 (audit 000130): inline form for editing one
// (provider_id, model_id) row's metadata — context_length +
// thinking_strength. Saves via setModelOverride, refreshes the
// parent on success.

export function ModelOverrideEditor({
  providerId,
  model,
  onClose,
  onSaved,
}: {
  providerId: string;
  model: ProviderModel;
  onClose: () => void;
  onSaved: () => void;
}) {
  const { t } = useTranslation();
  // Seed with the current values (which may already be the
  // built-in fallback or a previously-saved override — either
  // way editing is the same UX).
  const [contextLength, setContextLength] = useState<string>(
    model.context_length != null ? String(model.context_length) : "",
  );
  const [thinking, setThinking] = useState<"low" | "medium" | "high" | "">(
    ((): "low" | "medium" | "high" | "" => {
      const v = (model.thinking_strength ?? "").toLowerCase();
      if (v === "low" || v === "medium" || v === "high") return v;
      return "";
    })(),
  );
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const save = async () => {
    setSaving(true);
    setError(null);
    try {
      const patch: {
        provider_id: string;
        model_id: string;
        context_length?: number | null;
        thinking_strength?: "low" | "medium" | "high" | null;
      } = { provider_id: providerId, model_id: model.id };
      // Empty string in the input means "explicit null" —
      // clears the override row for that column. Same for
      // thinking select.
      const ctxTrim = contextLength.trim();
      patch.context_length = ctxTrim === "" ? null : Number(ctxTrim);
      patch.thinking_strength = thinking === "" ? null : thinking;
      await setModelOverride(patch);
      onSaved();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  };

  const clear = async () => {
    if (
      !confirm(t("settings.models.clearOverrideConfirm", { name: model.display_name ?? model.id }))
    )
      return;
    setSaving(true);
    setError(null);
    try {
      await clearModelOverride(providerId, model.id);
      onSaved();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <li className="col-span-2 mt-1 rounded border border-border bg-surface-1 p-2 font-sans">
      <div className="grid grid-cols-2 gap-2">
        <label className="text-[11px]">
          <span className="block text-text-secondary">
            {t("settings.models.contextLengthLabel")}
          </span>
          <input
            type="number"
            min={0}
            value={contextLength}
            onChange={(e) => setContextLength(e.target.value)}
            placeholder={String(t("settings.models.contextLengthPlaceholder"))}
            disabled={saving}
            className="mt-1 w-full rounded border border-border bg-surface-2 px-2 py-1 font-mono text-xs"
          />
        </label>
        <label className="text-[11px]">
          <span className="block text-text-secondary">
            {t("settings.models.thinkingStrengthLabel")}
          </span>
          <select
            value={thinking}
            onChange={(e) => setThinking(e.target.value as "low" | "medium" | "high" | "")}
            disabled={saving}
            className="mt-1 w-full rounded border border-border bg-surface-2 px-2 py-1 text-xs"
          >
            <option value="">{t("settings.models.thinkingUnset")}</option>
            <option value="low">low</option>
            <option value="medium">medium</option>
            <option value="high">high</option>
          </select>
        </label>
      </div>
      {error && <p className="mt-1 text-[11px] text-status-failed">{error}</p>}
      <div className="mt-2 flex items-center justify-end gap-1">
        <button
          type="button"
          onClick={clear}
          disabled={saving}
          className="rounded px-2 py-1 text-[11px] text-text-secondary hover:bg-status-warn/15 hover:text-status-warn"
        >
          {t("settings.models.clearOverride")}
        </button>
        <button
          type="button"
          onClick={onClose}
          disabled={saving}
          className="rounded px-2 py-1 text-[11px] text-text-secondary hover:bg-surface-3"
        >
          {t("settings.models.cancelEdit")}
        </button>
        <button
          type="button"
          onClick={save}
          disabled={saving}
          className="rounded bg-primary px-2 py-1 text-[11px] text-white hover:bg-primary/90 disabled:opacity-50"
        >
          {t("settings.models.saveOverride")}
        </button>
      </div>
    </li>
  );
}
