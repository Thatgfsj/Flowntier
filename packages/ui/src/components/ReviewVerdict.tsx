import { cn } from '../lib/cn.js';

export type Verdict = 'PASS' | 'REPAIR' | 'REWRITE';

export interface ReviewIssue {
  severity: 'MAJOR' | 'MINOR' | 'NIT';
  file?: string;
  line?: number;
  message: string;
  suggested_fix?: string;
}

export interface ReviewVerdictProps {
  verdict: Verdict;
  confidence: number;
  issues: readonly ReviewIssue[];
  summary: string;
  /** BUG-FRONTEND-RT-5 (event 000031): optional localized label
   *  for the verdict pill. Falls back to the raw enum when
   *  omitted. Pass e.g. `t('reviewVerdict.verdict.PASS')`. */
  verdictLabel?: string;
  /** Optional localized label for the confidence field. */
  confidenceLabel?: string;
  className?: string;
}

const verdictColor: Record<Verdict, string> = {
  PASS: 'bg-status-done text-white',
  REPAIR: 'bg-status-warn text-white',
  REWRITE: 'bg-status-failed text-white',
};

const severityColor: Record<ReviewIssue['severity'], string> = {
  MAJOR: 'border-l-status-failed',
  MINOR: 'border-l-status-warn',
  NIT: 'border-l-text-secondary',
};

export function ReviewVerdict({
  verdict,
  confidence,
  issues,
  summary,
  verdictLabel,
  confidenceLabel,
  className,
}: ReviewVerdictProps) {
  return (
    <div className={cn('rounded-md border border-border bg-surface-1 p-3', className)}>
      <div className="flex items-center justify-between gap-2">
        <span
          className={cn(
            'rounded-full px-3 py-1 text-sm font-bold tracking-wide',
            verdictColor[verdict],
          )}
        >
          {verdictLabel ?? verdict}
        </span>
        <span className="text-xs text-text-secondary tabular-nums">
          {confidenceLabel ?? `confidence ${confidence.toFixed(2)}`}
        </span>
      </div>
      <p className="mt-2 text-sm">{summary}</p>
      {issues.length > 0 && (
        <ul className="mt-3 space-y-1">
          {issues.map((iss, i) => (
            <li
              key={i}
              className={cn(
                'border-l-2 pl-2 text-xs',
                severityColor[iss.severity],
              )}
            >
              <span className="font-mono text-[10px] uppercase tracking-wide">
                [{iss.severity}]
              </span>{' '}
              {iss.file && (
                <span className="font-mono">
                  {iss.file}
                  {iss.line !== undefined && `:${iss.line}`}{' '}
                </span>
              )}
              {iss.message}
              {iss.suggested_fix && (
                <pre className="mt-1 rounded bg-surface-2 p-1 text-[10px]">
                  {iss.suggested_fix}
                </pre>
              )}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
