import { useTranslation } from 'react-i18next';
import { ConsoleLine, type ConsoleSource } from '@flowntier/ui';
import type { WfEvent, LogLevel } from '@flowntier/shared';

export interface BottomConsoleProps {
  events: readonly WfEvent[];
}

// Log-level labels. The key (e.g. 'error') is the LogLevel enum
// value; the value is the i18n key. We resolve via t() at
// render time so the labels follow the current language.
const LEVEL_LABEL_KEYS: Record<LogLevel, string> = {
  error: 'bottomConsole.levels.error',
  warn: 'bottomConsole.levels.warn',
  info: 'bottomConsole.levels.info',
  debug: 'bottomConsole.levels.debug',
  trace: 'bottomConsole.levels.trace',
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
  const { t } = useTranslation();
  // Show last 200 events; the rest are available in the workflow log.
  const visible = events.slice(-200);
  const hasContent = visible.some((e) => e.kind === 'console');
  // v0.4.26 (event 000119): chairman flagged the empty panel
  // still leaving a ~160px gap at the bottom of ChatZone.
  // When there are no events to show we collapse the entire
  // section to height 0 so the layout stops showing a dead
  // strip of empty background. The header bar above the
  // console also goes away — there's nothing for the user to
  // look at when nothing has happened yet.
  if (!hasContent) {
    return null;
  }
  return (
    <section
      className="h-40 shrink-0 overflow-y-auto border-t border-border bg-surface-2 p-2 font-mono text-[13px]"
      aria-label={t('bottomConsole.tabs.log')}
    >
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
                text={`[${t(LEVEL_LABEL_KEYS[e.level])}] ${e.message}`}
              />
            </li>
          );
        })}
      </ol>
    </section>
  );
}
