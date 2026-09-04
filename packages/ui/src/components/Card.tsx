import type { HTMLAttributes, ReactNode } from 'react';
import { cn } from '../lib/cn.js';

export interface CardProps extends HTMLAttributes<HTMLDivElement> {
  children: ReactNode;
}

export function Card({ className, children, ...rest }: CardProps) {
  return (
    <div
      className={cn(
        'rounded-xl border border-white/8 bg-surface-2/90 p-4 shadow-md backdrop-blur-sm transition-all',
        className,
      )}
      {...rest}
    >
      {children}
    </div>
  );
}
