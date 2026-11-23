import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Card } from "@flowntier/ui";
import { resetQuota } from "../../lib/api.js";
import type { QuotaStatusEntry } from "../../lib/api.js";

// ── v0.4.20 (event 000056): quota status block ─────────────────────
// Top-level "角色额度状态" card. Lists every quota_failures row
// from the Rust pipe (polled every 30s while Settings is open)
// and gives the chairman a 重置 button per row.
//
// Behaviour:
//   - empty list → renders a friendly "正常" placeholder so the
//     chairman knows the panel exists but no rows are present
//   - status="failed" → amber dot · 本次失败
//   - status="pending_5h_wait" → orange dot · 等下一个 5h 刷新点
//   - status="rate_limited" → red X · 已停 (click 重置 to lift)

export interface QuotaStatusBlockProps {
  rows: QuotaStatusEntry[];
  onReset: () => void | Promise<void>;
}

export function QuotaStatusBlock({ rows, onReset }: QuotaStatusBlockProps) {
  const { t } = useTranslation();
  const [busy, setBusy] = useState<string | null>(null);

  const handleReset = async (row: QuotaStatusEntry) => {
    const key = `${row.role_id}:${row.model_id}`;
    setBusy(key);
    try {
      const r = await resetQuota(row.role_id, row.model_id);
      if (r.ok) await onReset();
    } catch (e) {
      console.error("[QuotaStatusBlock] reset failed:", e);
    } finally {
      setBusy(null);
    }
  };

  return (
    <Card>
      <h3 className="mb-2 text-xs font-semibold uppercase tracking-wide text-text-secondary">
        {t("settings.roles.quota.heading")}
      </h3>
      {rows.length === 0 ? (
        <p className="px-1 text-[10px] text-text-secondary">{t("settings.roles.quota.empty")}</p>
      ) : (
        <ol className="flex flex-col gap-1">
          {rows.map((row) => {
            const status = row.status;
            const labelKey =
              status === "failed"
                ? "failed"
                : status === "pending_5h_wait"
                  ? "pending_5h_wait"
                  : status === "rate_limited"
                    ? "rate_limited"
                    : "failed";
            const dotColor =
              status === "failed"
                ? "bg-status-warn"
                : status === "pending_5h_wait"
                  ? "bg-status-pending"
                  : status === "rate_limited"
                    ? "bg-status-failed"
                    : "bg-text-secondary";
            const key = `${row.role_id}:${row.model_id}`;
            return (
              <li
                key={key}
                className="flex items-center justify-between rounded-md border border-border bg-surface-2 px-2 py-1.5 text-[11px]"
              >
                <span className="flex items-center gap-2">
                  <span className={`h-1.5 w-1.5 rounded-full ${dotColor}`} />
                  <span className="font-mono text-text-secondary">{row.role_id}</span>
                  <span className="text-text-tertiary">·</span>
                  <span className="font-mono">{row.model_id}</span>
                  <span className="text-text-tertiary">·</span>
                  <span>{t(`settings.roles.quota.${labelKey}`)}</span>
                  {row.attempt_count > 1 && (
                    <span className="text-text-tertiary">×{row.attempt_count}</span>
                  )}
                </span>
                {status === "rate_limited" && (
                  <button
                    onClick={() => void handleReset(row)}
                    disabled={busy === key}
                    className="rounded-md border border-status-failed/40 bg-status-failed/10 px-2 py-0.5 text-[10px] text-status-failed hover:bg-status-failed/20 disabled:opacity-40"
                  >
                    {busy === key ? "…" : t("settings.roles.quota.reset")}
                  </button>
                )}
              </li>
            );
          })}
        </ol>
      )}
    </Card>
  );
}
