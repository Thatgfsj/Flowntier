/**
 * Event types emitted on the ACO event bus.
 *
 * Mirror of `crates/event-bus/src/lib.rs` `WfEvent` and the
 * Python `runtime/event_bus.py` `WfEvent`.
 *
 * Versioned as `workflow-event/v0.1`.
 */

export const PROTOCOL_VERSION = 'workflow-event/v0.1' as const;

export type LogLevel = 'trace' | 'debug' | 'info' | 'warn' | 'error';

export type WfEvent =
  | TransitionEvent
  | TokenUsageEvent
  | ConsoleEvent
  | MilestoneEvent
  | UserQueryEvent
  | TaskStatusEvent;

export type TaskStatusKind =
  | 'PENDING'
  | 'DISPATCHED'
  | 'RUNNING'
  | 'DONE'
  | 'APPROVED'
  | 'FAILED'
  | 'REPAIRING'
  | 'AWAITING_REVIEW';

export interface TransitionEvent {
  readonly kind: 'transition';
  readonly wf_id: string;
  readonly from: string | null;
  readonly to: string;
  readonly event: string;
  readonly actor: string;
  /** ISO 8601 timestamp. */
  readonly ts: string;
}

export interface TokenUsageEvent {
  readonly kind: 'token_usage';
  readonly agent_id: string;
  readonly provider: string;
  readonly model: string;
  readonly input_tokens: number;
  readonly output_tokens: number;
  readonly cached_tokens: number;
  readonly cost_usd: number | null;
}

export interface ConsoleEvent {
  readonly kind: 'console';
  readonly agent_id: string;
  readonly level: LogLevel;
  readonly message: string;
  /** ISO 8601 timestamp when the event was emitted (optional;
   *  older event payloads from before v0.4 may not include it).
   *  PerTaskConsole uses this for the per-line timestamp; when
   *  absent we fall back to wall-clock NOW. */
  readonly ts?: string;
}

export interface MilestoneEvent {
  readonly kind: 'milestone';
  readonly phase: string;
  readonly label: string;
}

export interface UserQueryEvent {
  readonly kind: 'user_query';
  readonly query_id: string;
  readonly question: string;
  readonly options: readonly string[];
}

export interface TaskStatusEvent {
  readonly kind: 'task_status';
  /** ISO 8601 timestamp. */
  readonly ts: string;
  readonly task_id: string;
  readonly task_title: string;
  readonly task_status: TaskStatusKind;
  readonly task_summary?: string;
  readonly task_files?: readonly string[];
}
