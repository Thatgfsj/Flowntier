import type { HTMLAttributes, ReactNode } from 'react';
import { cn } from '../lib/cn.js';

export interface CardProps extends HTMLAttributes<HTMLDivElement> {
  children: ReactNode;
}

export function Card({ className, children, ...rest }: CardProps) {
  return (
    <div
      className={cn(
        // v0.4.24 (event 000119): lifted from surface-1 to
        // surface-2 so a Card visibly sits one step above the
        // surface-3 panel that hosts it, giving a clean "panel
        // → card → row" three-layer depth read.
        'rounded-lg border border-border bg-surface-2 p-4 shadow-sm',
        className,
      )}
      {...rest}
    >
      {children}
    </div>
  );
}
