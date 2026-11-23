import { useState } from "react";
import { useTranslation } from "react-i18next";
import { saveSecret } from "../../lib/api.js";
import { tErr } from "../../lib/errs.js";

// ── Quick Add AI ─────────────────────────────────────────────────

// v0.4.30 (audit 000130): secret names moved to the
// `flowntier/<id>` internal namespace so they cannot be confused
// with shell env vars. The previous `*_API_KEY` names happened to
// match shell env var conventions, which made the chairman worry
// that an old env var was being read. Migration 0008 renames any
// pre-existing keychain rows.
export const QUICK_PROVIDERS = [
  {
    id: "openai",
    name: "OpenAI",
    secretName: "flowntier/openai",
    placeholder: "sk-...",
    description: "GPT-5, GPT-5 Mini",
    color: "#10a37f",
  },
  {
    id: "anthropic",
    name: "Anthropic",
    secretName: "flowntier/anthropic",
    placeholder: "sk-ant-...",
    description: "Claude Opus, Sonnet, Haiku",
    color: "#d97706",
  },
  {
    id: "google",
    name: "Google Gemini",
    secretName: "flowntier/google",
    placeholder: "AIza...",
    description: "Gemini 2.5 Pro, Flash",
    color: "#4285f4",
  },
  {
    id: "deepseek",
    name: "DeepSeek",
    secretName: "flowntier/deepseek",
    placeholder: "sk-...",
    description: "DeepSeek Chat, Reasoner",
    color: "#6366f1",
  },
  {
    id: "minimax",
    name: "MiniMax",
    secretName: "flowntier/minimax",
    placeholder: "eyJ...",
    description: "MiniMax M3",
    color: "#f97316",
  },
  {
    id: "kimi",
    name: "Kimi (月之暗面)",
    secretName: "flowntier/kimi",
    placeholder: "sk-...",
    description: "Kimi K2",
    color: "#8b5cf6",
  },
  {
    id: "zhipu",
    name: "GLM (智谱)",
    secretName: "flowntier/glm",
    placeholder: "",
    description: "GLM-4",
    color: "#059669",
  },
  {
    id: "mimo",
    name: "MIMO (小米)",
    secretName: "flowntier/mimo",
    placeholder: "sk-...",
    description: "小米 MIMO",
    color: "#ff6900",
  },
  {
    id: "siliconflow",
    name: "SiliconFlow",
    secretName: "flowntier/siliconflow",
    placeholder: "sk-...",
    description: "SiliconFlow (硅基流动)",
    color: "#0ea5e9",
  },
];

export function QuickAddAI({ onSaved }: { onSaved: () => void }) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [selected, setSelected] = useState<string | null>(null);
  const [apiKey, setApiKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState(false);

  const provider = QUICK_PROVIDERS.find((p) => p.id === selected);

  const handleSave = async () => {
    if (!provider || !apiKey.trim()) return;
    setBusy(true);
    setError(null);
    try {
      console.log("[QuickAddAI] saving:", provider.secretName);
      const result = await saveSecret(provider.secretName, apiKey.trim());
      console.log("[QuickAddAI] save result:", result);
      if (!result || !result.saved) {
        setError(t("settings.quickAdd.errorInvalidKey"));
        return;
      }
      if (result.warning) {
        // Key persisted, but seed to os.environ failed — non-fatal.
        console.warn("[QuickAddAI] seed warning:", result.warning);
      }
      setSuccess(true);
      setApiKey("");
      onSaved();
      setTimeout(() => {
        setOpen(false);
        setSuccess(false);
        setSelected(null);
      }, 1500);
    } catch (e) {
      console.error("[QuickAddAI] save failed:", e);
      setError(tErr(t, e, "settings.error.saveFailed"));
    } finally {
      setBusy(false);
    }
  };

  if (!open) {
    return (
      <button
        type="button"
        onClick={() => setOpen(true)}
        className="flex w-full items-center justify-center gap-2 rounded-lg border border-chief/30 bg-chief/10 px-4 py-2.5 text-sm font-medium text-chief transition-colors hover:bg-chief/20"
      >
        <span className="text-lg">+</span>
        {t("settings.providers.addAI")}
      </button>
    );
  }

  return (
    <div className="rounded-lg border border-border bg-surface-1 p-4">
      <div className="mb-3 flex items-center justify-between">
        <h3 className="text-sm font-semibold text-primary">{t("settings.quickAdd.title")}</h3>
        <button
          type="button"
          onClick={() => {
            setOpen(false);
            setSelected(null);
            setApiKey("");
            setError(null);
            setSuccess(false);
          }}
          className="text-xs text-text-secondary hover:text-primary"
        >
          {t("settings.action.cancel")}
        </button>
      </div>

      <div className="mb-3 max-h-[300px] overflow-y-auto">
        <div className="grid grid-cols-2 gap-2">
          {QUICK_PROVIDERS.map((p) => (
            <button
              key={p.id}
              type="button"
              onClick={() => {
                setSelected(p.id);
                setApiKey("");
                setError(null);
              }}
              className={`flex items-center gap-2 rounded-md border p-2.5 text-left transition-colors ${selected === p.id ? "border-chief bg-surface-2" : "border-border bg-surface-1 hover:border-text-secondary"}`}
            >
              <div className="h-3 w-3 shrink-0 rounded-full" style={{ backgroundColor: p.color }} />
              <div className="min-w-0">
                <div className="text-xs font-medium">{p.name}</div>
                <div className="truncate text-[10px] text-text-secondary">{p.description}</div>
              </div>
            </button>
          ))}
        </div>
      </div>

      {provider && (
        <div className="space-y-2">
          <label className="block text-xs text-text-secondary">
            {provider.name} {t("settings.custom.apiKeyLabel")}
          </label>
          <input
            type="password"
            value={apiKey}
            onChange={(e) => setApiKey(e.target.value)}
            placeholder={provider.placeholder}
            className="w-full rounded border border-border bg-surface-2 px-3 py-2 font-mono text-sm placeholder:text-text-secondary focus:border-chief focus:outline-none focus:ring-2 focus:ring-chief/50"
            onKeyDown={(e) => {
              if (e.key === "Enter" && apiKey.trim()) void handleSave();
            }}
          />
          {/* v0.4.30 (audit 000130): explicit note that the
              value lands in the encrypted keychain SQLite and
              the runtime never reads the shell env var of the
              same name. Reuses the same string as the tooltip
              next to the secret_name field in the detail
              panel. */}
          <p className="text-[10px] text-text-secondary">{t("settings.custom.keyNotEnv")}</p>
          {error && (
            <p role="alert" aria-live="polite" className="text-xs text-status-failed">
              {error}
            </p>
          )}
          {success && (
            <p role="status" aria-live="polite" className="text-xs text-status-done">
              {t("settings.quickAdd.saved")}
            </p>
          )}
          <button
            type="button"
            onClick={handleSave}
            disabled={busy || !apiKey.trim()}
            className="w-full rounded bg-chief px-3 py-2 text-sm font-medium text-black hover:brightness-110 disabled:opacity-50"
          >
            {busy
              ? t("settings.action.save")
              : t("settings.secrets.saveKeyFor", { provider: provider.name })}
          </button>
        </div>
      )}
    </div>
  );
}
