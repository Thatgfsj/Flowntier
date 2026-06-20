import { ConsoleLine, type ConsoleSource } from '@aco/ui';
import type { WfEvent, LogLevel } from '@aco/shared';

export interface BottomConsoleProps {
  events: readonly WfEvent[];
}

const LEVEL_LABELS: Record<LogLevel, string> = {
  error: '错误',
  warn: '警告',
  info: '信息',
  debug: '调试',
  trace: '追踪',
};

function agentToSource(agentId: string): ConsoleSource {
  if (agentId === 'agent:chief') return 'chief';
  if (agentId === 'agent:critic:a') return 'critic-a';
  if (agentId === 'agent:critic:b') return 'critic-b';
  if (agentId.startsWith('agent:worker:')) return 'worker';
  return 'system';
}

function shortTime(iso: string): string {
  // ISO -> HH:MM:SS
  return iso.slice(11, 19);
}

export function BottomConsole({ events }: BottomConsoleProps) {
  // Show last 200 events; the rest are available in the workflow log.
  const visible = events.slice(-200);
  return (
    <section
      className="h-40 shrink-0 overflow-y-auto border-t border-border bg-surface-2 p-2 font-mono text-[13px]"
      aria-label="控制台日志"
    >
      {visible.length === 0 ? (
        <div className="p-2 text-text-secondary">控制台空闲。</div>
      ) : (
        <ol className="flex flex-col gap-0.5">
          {visible.map((e, i) => {
            if (e.kind !== 'console') return null;
            // ConsoleEvent doesn't have ts; use current time as fallback
            const ts = shortTime(new Date().toISOString());
            return (
              <li key={i}>
                <ConsoleLine
                  ts={ts}
                  source={agentToSource(e.agent_id)}
                  text={`[${LEVEL_LABELS[e.level]}] ${e.message}`}
                />
              </li>
            );
          })}
        </ol>
      )}
    </section>
  );
}
