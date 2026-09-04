import type { ReactNode } from 'react';
import { cn } from '../lib/cn.js';

export interface ReasoningBubbleProps {
  agentName: string;
  roleColorClass: string;
  step: string;
  body: string;
  /** Optional pre-formatted transcript. */
  transcript?: ReactNode;
  /** Relative time string, e.g. "3s ago". */
  ago?: string;
  className?: string;
}

/**
 * Card showing an agent's reasoning step. See `docs/UI_GUIDELINES.md` §6.4.
 */
export function ReasoningBubble({
  agentName,
  roleColorClass,
  step,
  body,
  transcript,
  ago,
  className,
}: ReasoningBubbleProps) {
  return (
    <article
      className={cn(
        'relative overflow-hidden rounded-xl border border-white/8 bg-surface-2/90 p-4 shadow-sm backdrop-blur-md transition-all hover:border-white/15',
        'before:absolute before:inset-x-0 before:top-0 before:h-0.5 before:bg-gradient-to-r before:from-transparent before:via-current before:to-transparent',
        roleColorClass,
        className,
      )}
    >
      <header className="mb-1 flex items-baseline justify-between gap-2">
        <div>
          <span className="font-semibold">{agentName}</span>
          <span className="ml-2 text-xs text-text-secondary">— {step}</span>
        </div>
        {ago && <time className="text-xs text-text-secondary tabular-nums">{ago}</time>}
      </header>
      <p className="text-sm">{body}</p>
      {transcript && (
        <details className="mt-2">
          <summary className="cursor-pointer text-xs text-text-secondary hover:text-primary">
            Show full transcript
          </summary>
          <div className="mt-2 rounded-md bg-surface-2 p-2 text-xs">{transcript}</div>
        </details>
      )}
    </article>
  );
}
