/**
 * Event types emitted on the ACO event bus.
 *
 * v0.4.22 (event 000116): renamed from "WfEvent mirror of
 * `crates/event-bus/src/lib.rs`" to a direct mirror of
 * `crates/agent-core/src/event.rs::AgentEvent` — the runtime's
 * named-pipe events stream carries `AgentEvent` (via
 * `serde(tag = "kind", rename_all = "snake_case")`), not
 * `WfEvent`. The `WfEvent` name is preserved for backward
 * compatibility with downstream consumers that already imported
 * the type. The previous `crates/event-bus/src/lib.rs::WfEvent`
 * enum (Transition / TokenUsage / Console / Milestone /
 * UserQuery / TaskStatus) is dead code that was removed in
 * event 000116; the carrier is now `AgentEvent` directly.
 *
 * Cross-language schema contract — see
 * `crates/agent-core/src/event.rs::AGENT_EVENT_KINDS` and the
 * `tests/event_kind_set_matches_actual_serde_tags` test. The
 * mirror on this side is [`WF_EVENT_KINDS`] plus the vitest
 * suite at `tests/event-kinds.test.ts`. When you add a variant
 * on either side, mirror it on the other in the same commit.
 *
 * Versioned as `workflow-event/v0.1`.
 */

export const PROTOCOL_VERSION = 'workflow-event/v0.1' as const;

export type LogLevel = 'trace' | 'debug' | 'info' | 'warn' | 'error';

/** v0.4.22 (event 000116): single source of truth for every
 *  `kind` literal in the [`WfEvent`] union. Mirrors
 *  `crates/agent-core/src/event.rs::AGENT_EVENT_KINDS` — when
 *  adding an event, update BOTH sides in the same commit; the
 *  Rust test `event_kind_set_matches_actual_serde_tags` and
 *  the vitest `tests/event-kinds.test.ts` will fail otherwise.
 *
 *  `as const` so TypeScript narrows the array elements to
 *  literal strings; the vitest compares sorted sets to the
 *  Rust BTreeSet.
 */
export const WF_EVENT_KINDS = [
  'text_delta',
  'tool_started',
  'tool_finished',
  'phase_transition',
  'token_usage',
  'done',
  'reviewer_verdict',
  'repair_loop',
] as const;

export type WfEventKind = typeof WF_EVENT_KINDS[number];

export type WfEvent =
  | TransitionEvent
  | TokenUsageEvent
  | ConsoleEvent
  | MilestoneEvent
  | UserQueryEvent
  | TaskStatusEvent
  | ReviewerVerdictEvent
  | RepairLoopEvent;

export type TaskStatusKind =
  | 'PENDING'
  | 'DISPATCHED'
  | 'RUNNING'
  | 'DONE'
  | 'FAILED'
  | 'REPAIRING'
  /**
   * v0.4.22 (event 000116): never emitted by the Rust
   * orchestrator. The runtime's task_status column only
   * carries the six kinds above; `APPROVED` was historically
   * a UI-only state used by `RightPanel` / `PlanGraph` to
   * colour code approved-after-review rows. Kept here as a
   * literal so existing TS narrowing still works, but the
   * runtime never sends it over the wire.
   * @deprecated Use `'DONE'` — the runtime collapses approve
   * into done. */
  | 'APPROVED'
  /**
   * v0.4.22 (event 000116): also never emitted by the Rust
   * orchestrator. The pre-event-000116 design called for a
   * review-then-approve flow, but the implementation went
   * straight from REPAIRING → DONE (or DONE → REPAIRING via
   * the event 000113 loop). Kept here as a literal for
   * backward compatibility.
   * @deprecated Use `'RUNNING'` while the critic is reviewing. */
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

/** v0.4.22 (event 000116): REMOVED — `WorkflowCompleteEvent`
 *  used to live here with `kind: 'workflow_complete'`, but no
 *  Rust variant in `crates/agent-core/src/event.rs::AgentEvent`
 *  ever emits it. The webview gets the equivalent signal from
 *  `kind: 'done'` (event 000069 follow-up). Removing the orphan
 *  prevents `applyEvent` from carrying a branch that can never
 *  fire. */

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
