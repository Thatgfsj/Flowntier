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
  | TaskStatusEvent
  | WorkflowCompleteEvent
  | ReviewerVerdictEvent
  | RepairLoopEvent;

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
  /** BUG-FRONTEND-RT-3 (event 000029): optional status for
   *  completion detection. 'completed' on the delivery milestone
   *  signals the workflow is done; the webview's applyEvent
   *  unblocks the cmd bar immediately without waiting for the
   *  Rust-side polling loop. */
  readonly status?: 'started' | 'in_progress' | 'completed' | 'failed';
}

export interface UserQueryEvent {
  readonly kind: 'user_query';
  readonly query_id: string;
  readonly question: string;
  readonly options: readonly string[];
}

/** BUG-FRONTEND-RT-3 (event 000029): emitted by the Rust runtime
 *  once the agent finishes the report and the workflow is
 *  complete. Webview's applyEvent handler unblocks the cmd
 *  bar immediately on seeing this. Optional in the schema —
 *  Rust-side may not always emit it (e.g. on a hard crash). */
export interface WorkflowCompleteEvent {
  readonly kind: 'workflow_complete';
  readonly wf_id: string;
  readonly status: 'DONE' | 'FAILED' | 'ABORTED';
  readonly summary?: string;
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

/** v0.4.22 (event 000112): reviewer / bug-hunter verdict, the
 *  pipe that replaces the previous hard-coded
 *  "PASS / 置信度 0.87" ReviewerCard on the webview. The
 *  Rust orchestrator emits one of these per critic per
 *  review phase (PlanReview + FinalReview), so a single
 *  workflow can produce up to four events. The webview uses
 *  the last-seen `phase === 'final-review'` event to bind.
 *
 *  Confidence is currently always 0.0 and `issues` is empty
 *  pending event 000115 (structured JSON reviewer prompt). */
export interface ReviewerVerdictEvent {
  readonly kind: 'reviewer_verdict';
  readonly wf_id: string;
  /** `"plan-review"` | `"final-review"`. */
  readonly phase: string;
  /** Critic id: `"agent:critic:a"` (BugHunter) or
   *  `"agent:critic:b"` (Reviewer). */
  readonly role: string;
  /** `"PASS"` | `"REPAIR"` | `"REWRITE"` (verbatim `verdict_of`). */
  readonly verdict: string;
  /** 0.0..=1.0. Currently 0.0 placeholder. */
  readonly confidence: number;
  /** Per-issue notes — empty pending event 000115. */
  readonly issues: readonly string[];
  /** First 200 chars of the critic's first sentence. */
  readonly summary: string;
}

/** v0.4.22 (event 000113): emitted by the orchestrator once
 *  each time it enters Phase 7 (repair) and decides to re-run
 *  Phase 5 (develop). The webview's repair panel renders
 *  "修复循环 N / max" from this event without having to
 *  re-parse `ReviewerVerdict` history. */
export interface RepairLoopEvent {
  readonly kind: 'repair_loop';
  readonly wf_id: string;
  /** 1-based index of this repair round. */
  readonly loop_index: number;
  /** Cap configured at workflow start (`max_repair_loops`). */
  readonly max_loops: number;
  readonly verdict_a: string;
  readonly verdict_b: string;
  readonly issues_a: readonly string[];
  readonly issues_b: readonly string[];
}
