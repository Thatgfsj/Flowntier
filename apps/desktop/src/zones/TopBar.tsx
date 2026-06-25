/**
 * Z1 — top bar. Title + update banner + user menu.
 * The command input was moved to the bottom (CommandDock) in v0.2.
 */
import type { UpdateBanner } from '../lib/updater';

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
  const showUpdate =
    updateBanner?.available === true && typeof updateBanner.version === 'string';
  return (
    <header className="flex h-14 shrink-0 items-center gap-4 border-b border-border bg-surface-2 px-4">
      <div className="flex items-baseline gap-2">
        <span className="font-semibold tracking-tight text-primary">{projectName}</span>
        <span className="text-xs text-text-secondary">· 智能体公司操作系统</span>
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
          title="点击下载并安装最新版本（应用会自动重启）"
        >
          ⬆ 升级 v{updateBanner!.version}
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
          Chat
        </button>
      )}
      <button
        type="button"
        onClick={onSettingsClick}
        className="rounded-md border border-border bg-surface-1 px-3 py-1.5 text-xs text-text-secondary transition-colors hover:text-primary focus:outline-none focus:ring-2 focus:ring-chief/50"
      >
        设置
      </button>
    </header>
  );
}
