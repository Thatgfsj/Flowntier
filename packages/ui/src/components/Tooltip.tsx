import * as TooltipPrimitive from '@radix-ui/react-tooltip';
import { type ReactNode } from 'react';
import { cn } from '../lib/cn.js';

// v0.4.29 (Phase C): a thin Radix Tooltip wrapper. Hover delay
// is intentionally low (200ms) so the chairman can skim the
// roster without the cards feeling sluggish. Side defaults to
// 'right' because the left roster pane sits on the left edge of
// the workbench and the right panel needs more elbow room.

export interface TooltipProps {
  /** Trigger element. Must be a single ReactNode that
   *  forwards ref (Radix needs the ref to position the
   *  portal). */
  children: ReactNode;
  /** The content rendering inside the floating panel. */
  content: ReactNode;
  /** Which side of the trigger to render on. Default 'right'. */
  side?: 'top' | 'right' | 'bottom' | 'left';
  /** Open delay in ms. Default 200. */
  delayMs?: number;
  /** Additional classes for the content panel. */
  className?: string;
}

export function Tooltip({
  children,
  content,
  side = 'right',
  delayMs = 200,
  className,
}: TooltipProps) {
  return (
    <TooltipPrimitive.Provider delayDuration={delayMs}>
      <TooltipPrimitive.Root>
        <TooltipPrimitive.Trigger asChild>{children}</TooltipPrimitive.Trigger>
        <TooltipPrimitive.Portal>
          <TooltipPrimitive.Content
            side={side}
            sideOffset={6}
            className={cn(
              'z-50 max-w-xs rounded-md border border-border bg-surface-2 p-3 text-xs text-text-primary shadow-lg',
              'data-[state=delayed-open]:animate-in data-[state=closed]:animate-out',
              'data-[state=closed]:fade-out-0 data-[state=delayed-open]:fade-in-0',
              className,
            )}
          >
            {content}
          </TooltipPrimitive.Content>
        </TooltipPrimitive.Portal>
      </TooltipPrimitive.Root>
    </TooltipPrimitive.Provider>
  );
}
