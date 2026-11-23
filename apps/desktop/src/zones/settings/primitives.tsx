import { useTranslation } from "react-i18next";

export function Field({ label, children }: { label: React.ReactNode; children: React.ReactNode }) {
  return (
    <div>
      <div className="mb-0.5 text-text-secondary">{label}</div>
      <div className="font-mono text-primary">{children}</div>
    </div>
  );
}

export function Toggle({
  on,
  onChange,
  disabled,
}: {
  on: boolean;
  onChange: (v: boolean) => void;
  disabled?: boolean;
}) {
  const { t } = useTranslation();
  return (
    <button
      type="button"
      onClick={(e) => {
        e.stopPropagation();
        onChange(!on);
      }}
      disabled={disabled}
      className={`relative h-5 w-9 rounded-full transition-colors focus:outline-none focus:ring-2 focus:ring-chief/50 ${on ? "bg-chief" : "bg-surface-3"} ${disabled ? "opacity-40 cursor-not-allowed" : ""}`}
      aria-pressed={on}
      aria-label={t("settings.providers.enabled")}
    >
      <span
        className={`absolute top-0.5 h-4 w-4 rounded-full bg-white transition-all ${on ? "left-4" : "left-0.5"}`}
      />
    </button>
  );
}

export function KeyBadge({ present }: { present: boolean }) {
  return present ? (
    <span className="rounded bg-status-done/20 px-1.5 py-0.5 text-[10px] text-status-done">
      Key ✓
    </span>
  ) : (
    <span className="rounded bg-status-warn/20 px-1.5 py-0.5 text-[10px] text-status-warn">
      Key ✗
    </span>
  );
}
