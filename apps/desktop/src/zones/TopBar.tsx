/**
 * Z1 — top bar. Title + user menu only.
 * The command input was moved to the bottom (CommandDock) in v0.2.
 */
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
}

export function TopBar({
  projectName,
  subtitle,
  onSettingsClick,
  onChatClick,
  chatOpen,
}: TopBarProps) {
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
