/**
 * Z1 — top bar. Title + update banner + user menu.
 * The command input was moved to the bottom (CommandDock) in v0.2.
 *
 * v0.4.21 (event 000066): added the <ErrorBadge /> to the right
 * of the chat/settings buttons. Polls /api/errors/recent every
 * 10s and lights up red/yellow when the runtime emits any.
 * Chairman's directive: "日志弄详细一点" — this gives the
 * transient errors a UI affordance so they aren't lost.
 */
import { useTranslation } from 'react-i18next';
import type { UpdateBanner } from '../lib/updater';
import { SUPPORTED } from '../i18n/index.js';
import { ErrorBadge } from '../components/ErrorBadge.js';

export interface TopBarProps {
  projectName: string;
  /** Optional subtitle for the project (e.g. current workflow). */
  subtitle?: string;
  /** Optional onClick for the settings button. */
  onSettingsClick?: () => void;
  /** Optional onClick for the chat-zone toggle. */
  onChatClick?: () => void;
  /** Whether the chat zone is currently expanded. */
  chatOpen?: boolean;
  /** Update banner state from the auto-update check (Phase 1.3). */
  updateBanner?: UpdateBanner;
  /** Click handler for the update banner. */
  onUpdateClick?: () => void;
}

export function TopBar({
  projectName,
  subtitle,
  onSettingsClick,
  onChatClick,
  chatOpen,
  updateBanner,
  onUpdateClick,
}: TopBarProps) {
  const { t } = useTranslation();
  const showUpdate =
    updateBanner?.available === true && typeof updateBanner.version === 'string';
  return (
    <header className="flex h-14 shrink-0 items-center gap-4 border-b border-border bg-surface-2 px-4">
      <div className="flex items-baseline gap-2">
        <span className="font-semibold tracking-tight text-primary">{projectName}</span>
        <span className="text-xs text-text-secondary">{t('topbar.tagline')}</span>
      </div>
      {subtitle !== undefined && subtitle.length > 0 && (
        <span className="text-xs text-text-secondary">/ {subtitle}</span>
      )}
      <div className="flex-1" />
      {showUpdate && (
        <button
          type="button"
          onClick={onUpdateClick}
          className="rounded-md border border-accent bg-accent/10 px-3 py-1.5 text-xs text-accent transition-colors hover:bg-accent/20 focus:outline-none focus:ring-2 focus:ring-accent/50"
          title={t('update.tooltip')}
        >
          {t('update.available', { version: updateBanner!.version })}
        </button>
      )}
      {onChatClick && (
        <button
          type="button"
          onClick={onChatClick}
          aria-pressed={chatOpen}
          className={`rounded-md border px-3 py-1.5 text-xs transition-colors focus:outline-none focus:ring-2 focus:ring-chief/50 ${
            chatOpen
              ? 'border-chief bg-chief/10 text-chief'
              : 'border-border bg-surface-1 text-text-secondary hover:text-primary'
          }`}
        >
          {t('topbar.chat')}
        </button>
      )}
      <ErrorBadge />
      <button
        type="button"
        onClick={onSettingsClick}
        className="rounded-md border border-border bg-surface-1 px-3 py-1.5 text-xs text-text-secondary transition-colors hover:text-primary focus:outline-none focus:ring-2 focus:ring-chief/50"
      >
        {t('topbar.settings')}
      </button>
      <LanguageToggle />
    </header>
  );
}

/**
 * Tiny two-state language toggle. Cycles between zh-CN and en-US.
 * Persists to localStorage via i18n.on('languageChanged').
 */
function LanguageToggle() {
  const { i18n, t } = useTranslation();
  const current = i18n.language?.startsWith('en') ? 'en-US' : 'zh-CN';
  const next = current === 'zh-CN' ? 'en-US' : 'zh-CN';
  return (
    <button
      type="button"
      onClick={() => {
        // BUG-FRONTEND-RT-?? (event 000046): when the Settings
        // modal is open, the modal backdrop intercepts pointer
        // events. Dispatch a custom event that App listens
        // for to close the Settings modal first, so the locale
        // toggle always works regardless of modal state.
        window.dispatchEvent(new CustomEvent('flowntier:close-modals'));
        void i18n.changeLanguage(next);
      }}
      title={t('lang.label') + ': ' + (SUPPORTED.find((l) => l === next) ?? '')}
      className="rounded-md border border-border bg-surface-1 px-2 py-1 text-xs text-text-secondary transition-colors hover:text-primary focus:outline-none focus:ring-2 focus:ring-chief/50"
      aria-label={`Language: ${current}`}
    >
      🌐 {current === 'zh-CN' ? t('lang.zh-CN') : t('lang.en-US')}
    </button>
  );
}
