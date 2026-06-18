import type { FormEvent } from 'react';

export interface CommandDockProps {
  commandInput: string;
  onCommandChange: (value: string) => void;
  onCommandSubmit: () => void;
  /** Whether the runtime is currently running. */
  busy?: boolean;
  /** Optional reset button label (shown when workflow is complete). */
  resetLabel?: string;
}

/**
 * Z5 — bottom command dock. Where the user types requests to the Chief.
 * Sits above the console, at the very bottom of the window.
 */
export function CommandDock({
  commandInput,
  onCommandChange,
  onCommandSubmit,
  busy = false,
  resetLabel,
}: CommandDockProps) {
  const handleSubmit = (e: FormEvent) => {
    e.preventDefault();
    onCommandSubmit();
  };

  const isReset = resetLabel !== undefined && resetLabel.length > 0;
  const canSubmit = isReset || (!busy && commandInput.trim().length > 0);

  return (
    <form
      onSubmit={handleSubmit}
      className="flex shrink-0 items-center gap-2 border-t border-border bg-surface-2 px-4 py-3"
      aria-label="命令输入栏"
    >
      <span className="shrink-0 font-mono text-xs text-text-secondary">首席 ▸</span>
      <input
        type="text"
        value={commandInput}
        onChange={(e) => onCommandChange(e.target.value)}
        placeholder={
          isReset
            ? '输入新指令以开始新工作流'
            : '向首席代理下达指令…  例如：实现 POST /auth/login 接口'
        }
        disabled={busy}
        className="flex-1 rounded-md border border-border bg-surface-1 px-3 py-2 text-sm placeholder:text-text-secondary focus:border-chief focus:outline-none focus:ring-2 focus:ring-chief/50 disabled:opacity-50"
        aria-label="向首席代理输入指令"
      />
      <button
        type="submit"
        disabled={!canSubmit}
        className="rounded-md bg-chief px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-chief/90 focus:outline-none focus:ring-2 focus:ring-chief/50 disabled:pointer-events-none disabled:opacity-50"
      >
        {isReset ? resetLabel : '发送'}
      </button>
    </form>
  );
}
