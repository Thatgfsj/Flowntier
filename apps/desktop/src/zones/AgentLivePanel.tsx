// apps/desktop/src/zones/AgentLivePanel.tsx
//
// v0.4.29 (Phase C): the floating panel that surfaces an
// agent's live state when the user hovers over a roster card.
// Solves the chairman's "角色思考/工作展示不清晰" complaint
// (audit 000129) — the small agent card on the left only has
// room for a status pill, but the user wants to know what the
// agent is *actually doing* right now.
//
// Data sources:
//   - `useAgentStatus()` + `state.activePhase` for the head
//     status (thinking / speaking / idle).
//   - `useEvents()` for the last 5 text_delta / tool_*
//     events from this role (timeline).
//   - `useEvents()` for the latest unpaired tool_started
//     (the "currently working on" line).
//   - `usePhaseStates()` for the currently active phase name.

import { useMemo, type ReactElement } from 'react';
import { useTranslation } from 'react-i18next';
import { Tooltip } from '@flowntier/ui';
import type { WfEvent } from '@flowntier/shared';
import {
  useAgentStatus,
  useEvents,
  usePhaseStates,
} from '../contexts/WorkflowContext.js';
import { PHASES } from '../contexts/workflowReducer.js';
import { agentIdToHeadRole } from '../lib/agentId.js';

// ── Helpers ────────────────────────────────────────────────────────

/** WfEvent variants that carry `agent_id`. Used to narrow the
 *  discriminated union before reading role/agent metadata. */
function agentIdOf(e: WfEvent): string | undefined {
  switch (e.kind) {
    case 'text_delta':
    case 'tool_started':
    case 'tool_finished':
    case 'console':
    case 'token_usage':
      return e.agent_id;
    default:
      return undefined;
  }
}

export interface AgentLivePanelProps {
  /** Dashboard role key. The panel weaves together events
   *  tagged with the matching agent_id. */
  role: 'chief' | 'critic-a' | 'critic-b' | 'worker';
}

export function AgentLivePanel({ role }: AgentLivePanelProps) {
  const { t } = useTranslation();
  const agentStatus = useAgentStatus();
  const events = useEvents();
  const phaseStates = usePhaseStates();

  const entry = agentStatus[role];
  const status = entry.status;

  const lastActivity = useMemo(() => {
    // Walk events back-to-front; first match for this role wins.
    for (let i = events.length - 1; i >= 0; i--) {
      const e = events[i];
      if (!e) continue;
      const id = agentIdOf(e);
      if (id === undefined) continue;
      const r = agentIdToHeadRole(id);
      if (r !== role) continue;
      return e;
    }
    return null;
  }, [events, role]);

  const timeline = useMemo(() => {
    const out: Array<{ ts: string; label: string }> = [];
    for (let i = events.length - 1; i >= 0 && out.length < 5; i--) {
      const e = events[i];
      if (!e) continue;
      const id = agentIdOf(e);
      if (id === undefined) continue;
      if (agentIdToHeadRole(id) !== role) continue;
      const label = describeEvent(e);
      if (label) out.push({ ts: hhmm(new Date()), label });
    }
    return out;
  }, [events, role]);

  const currentTool = useMemo(() => {
    // Find the most recent tool_started without a matching
    // tool_finished (i.e. still in-flight).
    const finishedIds = new Set<string>();
    for (let i = events.length - 1; i >= 0; i--) {
      const e = events[i];
      if (!e) continue;
      if (e.kind !== 'tool_finished') continue;
      const id = e.agent_id;
      if (agentIdToHeadRole(id) !== role) continue;
      finishedIds.add(e.tool_call_id);
    }
    for (let i = events.length - 1; i >= 0; i--) {
      const e = events[i];
      if (!e) continue;
      if (e.kind !== 'tool_started') continue;
      if (agentIdToHeadRole(e.agent_id) !== role) continue;
      if (!finishedIds.has(e.call.id)) {
        return e.call;
      }
    }
    return null;
  }, [events, role]);

  const thinking = useMemo(() => {
    if (lastActivity?.kind === 'text_delta') {
      const txt = lastActivity.delta;
      return txt.length > 240 ? txt.slice(0, 240) + '…' : txt;
    }
    return null;
  }, [lastActivity]);

  const activePhaseName = (() => {
    for (const p of PHASES) {
      if (phaseStates[p.name] === 'active') return p.name;
    }
    return null;
  })();

  const idleSeconds = Math.max(0, Math.floor((Date.now() - entry.since) / 1000));

  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center justify-between gap-2">
        <span className="text-xs font-semibold text-text-primary">
          {t(`agentLive.role.${role}`)}
        </span>
        <span
          className={
            status === 'thinking'
              ? 'rounded bg-chief/20 px-1.5 py-0.5 font-mono text-[10px] font-semibold text-chief'
              : status === 'speaking'
                ? 'rounded bg-status-done/20 px-1.5 py-0.5 font-mono text-[10px] font-semibold text-status-done'
                : 'rounded bg-surface-3 px-1.5 py-0.5 font-mono text-[10px] text-text-secondary'
          }
        >
          {t(`agentLive.status.${status}`)}
        </span>
      </div>

      {activePhaseName && (
        <div className="text-[10px] font-mono uppercase tracking-wide text-text-secondary">
          {t(`phases.${activePhaseName}`)}
        </div>
      )}

      {thinking && (
        <div>
          <div className="text-[10px] uppercase tracking-wide text-text-secondary">
            {t('agentLive.thinking')}
          </div>
          <p className="mt-0.5 text-[11px] leading-snug text-text-primary">
            {thinking}
          </p>
        </div>
      )}

      {currentTool && (
        <div>
          <div className="text-[10px] uppercase tracking-wide text-text-secondary">
            {t('agentLive.working')}
          </div>
          <p className="mt-0.5 font-mono text-[11px] text-text-primary">
            {currentTool.name}
            {summariseArgs(currentTool.args) && (
              <span className="text-text-secondary">
                {' '}
                {summariseArgs(currentTool.args)}
              </span>
            )}
          </p>
        </div>
      )}

      {timeline.length > 0 && (
        <div>
          <div className="text-[10px] uppercase tracking-wide text-text-secondary">
            {t('agentLive.recent')}
          </div>
          <ol className="mt-0.5 flex flex-col gap-0.5">
            {timeline.map((row, i) => (
              <li key={i} className="flex gap-1.5 text-[11px]">
                <span className="font-mono text-text-tertiary">{row.ts}</span>
                <span className="text-text-primary">{row.label}</span>
              </li>
            ))}
          </ol>
        </div>
      )}

      {status === 'idle' && (
        <div className="text-[11px] text-text-tertiary">
          {t('agentLive.idleFor', { seconds: idleSeconds })}
        </div>
      )}
    </div>
  );
}

export function AgentLivePanelTooltip({
  role,
  children,
}: AgentLivePanelProps & { children: ReactElement }) {
  return (
    <Tooltip side="right" content={<AgentLivePanel role={role} />}>
      {children}
    </Tooltip>
  );
}

// ── helpers ───────────────────────────────────────────────────────

function describeEvent(e: WfEvent): string | null {
  switch (e.kind) {
    case 'text_delta':
      return e.delta.length > 60 ? e.delta.slice(0, 60) + '…' : e.delta;
    case 'tool_started':
      return `tool: ${e.call.name}`;
    case 'tool_finished':
      return `tool done: ${e.preview.slice(0, 60)}`;
    case 'console':
      return e.message.length > 60 ? e.message.slice(0, 60) + '…' : e.message;
    case 'milestone':
      return `milestone: ${e.label ?? e.phase ?? 'tick'}`;
    case 'task_status':
      return `task ${e.task_id}: ${e.task_status}`;
    default:
      return null;
  }
}

function summariseArgs(args: unknown): string {
  if (args === null || args === undefined) return '';
  if (typeof args === 'string') return args.length > 40 ? args.slice(0, 40) + '…' : args;
  try {
    const s = JSON.stringify(args);
    return s.length > 60 ? s.slice(0, 60) + '…' : s;
  } catch {
    return '';
  }
}

function hhmm(d: Date): string {
  const hh = String(d.getHours()).padStart(2, '0');
  const mm = String(d.getMinutes()).padStart(2, '0');
  return `${hh}:${mm}`;
}
