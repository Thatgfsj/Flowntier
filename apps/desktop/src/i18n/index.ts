/**
 * i18n bootstrap for Flowntier desktop.
 *
 * Strategy for v0.4:
 *   - Default language: zh-CN (matches the current user-facing
 *     strings; user can switch to en-US via the TopBar toggle).
 *   - Only NEW strings introduced with the v0.4 release are
 *     translated (ErrorBoundary, update banner, language toggle).
 *     The legacy Chinese strings in TopBar / Settings /
 *     CommandDock are intentionally left un-translated for v0.4
 *     because a flag-day refactor of every component would be
 *     too risky this close to release.
 *   - Persistence: localStorage key 'flowntier.lang'. If missing,
 *     we honour navigator.language when it's 'en', otherwise
 *     fall back to 'zh-CN'.
 *   - <html lang> attribute is synced so screen readers and
 *     webview devtools show the right language.
 */
import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import zhCN from "./zh-CN";
import enUS from "./en-US";

const STORAGE_KEY = "flowntier.lang";
const SUPPORTED = ["zh-CN", "en-US"] as const;
export type SupportedLang = (typeof SUPPORTED)[number];

function detectInitialLang(): SupportedLang {
  // 1. localStorage (user-set, takes priority)
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (stored && (SUPPORTED as readonly string[]).includes(stored)) {
      return stored as SupportedLang;
    }
  } catch {
    // localStorage may be unavailable in some embedded webviews;
    // fall through to navigator.language.
  }
  // 2. navigator.language — only switch if explicitly English;
  //    all other locales default to zh-CN (existing UI is
  //    Chinese-only).
  if (typeof navigator !== "undefined") {
    const nl = navigator.language.toLowerCase();
    if (nl.startsWith("en")) return "en-US";
  }
  return "zh-CN";
}

const initialLang = detectInitialLang();

void i18n
  .use(initReactI18next)
  .init({
    resources: {
      "zh-CN": { translation: zhCN },
      "en-US": { translation: enUS },
    },
    lng: initialLang,
    fallbackLng: "zh-CN",
    // Don't warn on missing keys — for v0.4 we have many strings
    // that aren't translated yet (the legacy Chinese UI). The
    // translator reads the raw key, which is at least visible
    // and findable.
    saveMissing: false,
    // v0.4.22 (event 000118, follow-up): when BOTH the active
    // language and the fallback (zh-CN) miss a key, return
    // the raw key string instead of an empty string. The old
    // default was empty string, which made the UI render
    // literal empty spans in place of every missing label —
    // far worse than showing "phases.requirement" so the user
    // can spot the gap.
    returnEmptyString: false,
    interpolation: { escapeValue: false }, // React already escapes
    react: { useSuspense: false },
  })
  .catch((e) => {
    console.warn("[i18n] init failed:", e);
  });

// Sync <html lang> with the active language so screen readers,
// webview devtools, and copy-paste between apps use the right
// language tag.
function syncHtmlLang(lang: SupportedLang): void {
  if (typeof document !== "undefined" && document.documentElement) {
    document.documentElement.lang = lang;
  }
}

syncHtmlLang(initialLang);

i18n.on("languageChanged", (lang) => {
  syncHtmlLang(lang as SupportedLang);
  try {
    localStorage.setItem(STORAGE_KEY, lang);
  } catch {
    // ignore
  }
});

export { SUPPORTED };
export default i18n;
