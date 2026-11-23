import { useTranslation } from "react-i18next";
import { Card } from "@flowntier/ui";
import type { RoleInfo } from "../../lib/api.js";
import type { AvailableModel } from "./types.js";
import { getRoleLabel, modelOptionLabel, modelShortLabel } from "./helpers.js";

export interface RoleAssignmentCardProps {
  role: RoleInfo;
  availableModels: AvailableModel[];
  saving: boolean;
  onDefaultChange: (model: string) => void;
  onFallbackChange: (chain: string[]) => void;
}

/**
 * Per-role card: one default model + an editable ordered fallback
 * chain. Mirrors cc-switch's provider-card-with-failover pattern but
 * scoped to agent roles (chief / critic / worker) rather than to
 * external CLI apps.
 *
 * Saving is a single round-trip: the full `roles` array is sent to
 * `update_router_roles`, so adding/removing/reordering all commit in
 * one backend call.
 */
export function RoleAssignmentCard({
  role,
  availableModels,
  saving,
  onDefaultChange,
  onFallbackChange,
}: RoleAssignmentCardProps) {
  const { t } = useTranslation();
  const inChain = new Set(role.fallback_chain);

  // Filter out the default and the existing chain entries from the
  // "add fallback" dropdown so we don't offer the user nonsense options.
  const addable = availableModels.filter((m) => {
    const ref = `${m.provider}:${m.model}`;
    return ref !== role.default_model && !inChain.has(ref);
  });

  const move = (idx: number, dir: -1 | 1) => {
    const next = [...role.fallback_chain];
    const j = idx + dir;
    if (j < 0 || j >= next.length) return;
    const tmp = next[idx] as string;
    next[idx] = next[j] as string;
    next[j] = tmp;
    onFallbackChange(next);
  };

  const removeAt = (idx: number) => {
    onFallbackChange(role.fallback_chain.filter((_, i) => i !== idx));
  };

  const addModel = (ref: string) => {
    if (ref === role.default_model || inChain.has(ref)) return;
    onFallbackChange([...role.fallback_chain, ref]);
  };

  return (
    <Card className="!p-3">
      <div className="flex items-center gap-3">
        <div className="w-20 shrink-0">
          <div className="text-sm font-medium">{getRoleLabel(t, role.role)}</div>
          <div className="font-mono text-[10px] text-text-secondary">{role.role}</div>
        </div>
        <div className="flex-1">
          <div className="mb-1 text-[10px] uppercase tracking-wide text-text-secondary">
            {t("settings.models.default")}
          </div>
          <select
            value={role.default_model}
            onChange={(e) => onDefaultChange(e.target.value)}
            disabled={saving}
            className="w-full rounded-md border border-border bg-surface-1 px-2 py-1.5 text-xs focus:border-chief focus:outline-none disabled:opacity-50"
          >
            {availableModels.length === 0 ? (
              <option value={role.default_model}>{t("settings.models.emptyOption")}</option>
            ) : (
              availableModels.map((m) => {
                const ref = `${m.provider}:${m.model}`;
                return (
                  <option key={ref} value={ref}>
                    {modelOptionLabel(m)}
                  </option>
                );
              })
            )}
          </select>
        </div>
      </div>

      <div className="mt-3 border-t border-border pt-3">
        <div className="mb-1 flex items-center justify-between">
          <div className="text-[10px] uppercase tracking-wide text-text-secondary">
            {t("settings.models.fallbackChain", { count: role.fallback_chain.length })}
          </div>
          {addable.length > 0 && (
            <select
              defaultValue=""
              onChange={(e) => {
                if (e.target.value) {
                  addModel(e.target.value);
                  e.target.value = "";
                }
              }}
              disabled={saving}
              className="rounded border border-border bg-surface-1 px-1.5 py-0.5 text-[11px] focus:border-chief focus:outline-none disabled:opacity-50"
            >
              <option value="">+ 添加回退</option>
              {addable.map((m) => {
                const ref = `${m.provider}:${m.model}`;
                return (
                  <option key={ref} value={ref}>
                    {modelOptionLabel(m)}
                  </option>
                );
              })}
            </select>
          )}
        </div>

        {role.fallback_chain.length === 0 ? (
          <div className="text-[11px] text-text-secondary">
            {t("settings.models.emptyFallback")}
          </div>
        ) : (
          <ol className="space-y-1">
            {role.fallback_chain.map((ref, idx) => {
              const m = availableModels.find((x) => `${x.provider}:${x.model}` === ref);
              const label = m ? modelShortLabel(m) : ref;
              return (
                <li
                  key={`${ref}-${idx}`}
                  className="flex items-center gap-2 rounded border border-border bg-surface-1 px-2 py-1"
                >
                  <span className="w-6 shrink-0 text-center text-[10px] font-mono text-text-secondary">
                    {idx + 1}
                  </span>
                  <span className="flex-1 truncate font-mono text-[11px]" title={ref}>
                    {label}
                  </span>
                  <button
                    type="button"
                    onClick={() => move(idx, -1)}
                    disabled={idx === 0 || saving}
                    className="rounded px-1.5 py-0.5 text-[10px] text-text-secondary hover:bg-surface-2 hover:text-primary disabled:opacity-30"
                    aria-label={t("settings.action.moveUp")}
                  >
                    ↑
                  </button>
                  <button
                    type="button"
                    onClick={() => move(idx, 1)}
                    disabled={idx === role.fallback_chain.length - 1 || saving}
                    className="rounded px-1.5 py-0.5 text-[10px] text-text-secondary hover:bg-surface-2 hover:text-primary disabled:opacity-30"
                    aria-label={t("settings.action.moveDown")}
                  >
                    ↓
                  </button>
                  <button
                    type="button"
                    onClick={() => removeAt(idx)}
                    disabled={saving}
                    className="rounded px-1.5 py-0.5 text-[10px] text-text-secondary hover:bg-status-failed/20 hover:text-status-failed disabled:opacity-30"
                    aria-label={t("settings.action.delete")}
                  >
                    ×
                  </button>
                </li>
              );
            })}
          </ol>
        )}
      </div>
    </Card>
  );
}
