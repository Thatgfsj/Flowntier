// apps/desktop/src/lib/agentId.ts
//
// v0.4.29: shared decoder for the `agent_id` strings emitted on
// the `wf:event` named pipe. Replaces the four near-identical
// if/else chains previously sprinkled across App.tsx (applyEvent
// console branch + agentStatusToRole caller), BottomConsole.tsx
// (agentToSource), and LeftRoster.tsx (worker prefix check).
//
// Single source of truth — when a future agent prefix is added
// (e.g. 'agent:planner', 'agent:reporter'), update both the
// chief / critic-a / critic-b / worker map and the ConsoleSource
// union here in one commit.

import type { ConsoleSource } from "@flowntier/ui";

/** The four roles the dashboard tracks as first-class cards.
 *  Matches the keys of `AgentStatusMap` in workflowReducer.ts. */
export type AgentRole = "chief" | "critic-a" | "critic-b" | "worker";

/**
 * Result of decoding an `agent_id` from a Tauri event.
 *
 * - `kind: 'role'`     → mapped to one of the 4 dashboard cards.
 * - `kind: 'worker_task'` → a Phase-5 worker (the runtime tags
 *   each event with `task_id` ∈ {t0, t1, …}); the dashboard
 *   renders one card per task via `workerTaskStatus[taskId]`.
 * - `kind: 'system'`   → orchestrator / runtime internal logs;
 *   no card. Renders as `system` in the console.
 * - `kind: 'unknown'`  → unrecognised prefix; defaults to
 *   `system` in the console, no card update.
 */
export type AgentIdResolution =
  | { kind: "role"; role: AgentRole }
  | { kind: "worker_task"; taskId: string }
  | { kind: "system" }
  | { kind: "unknown"; raw: string };

/**
 * Decode an `agent_id` string (e.g. 'agent:chief', 'agent:critic:a',
 * 'agent:critic:b', 'agent:worker', 'agent:worker:t0', 'agent:system')
 * into the structured resolution the reducer + UI need.
 *
 * The runtime encodes agent IDs as:
 *   - 'agent:chief'            (chief)
 *   - 'agent:critic:a'         (BugHunter / critic-a)
 *   - 'agent:critic:b'         (Reviewer / critic-b)
 *   - 'agent:worker'           (single-worker mode — no task id)
 *   - 'agent:worker:t{idx}'    (Phase-5 per-task worker)
 *   - 'agent:system'           (orchestrator runtime logs)
 *
 * IMPORTANT: per `crates/agent-core/src/event.rs`, the prefix is
 * always the literal string 'agent:' followed by a single token
 * ('chief', 'critic', 'worker', 'system'). The colon between
 * 'critic' and the letter ('a'/'b') is part of the agent_id —
 * never split on ':'.
 */
export function decodeAgentId(agentId: string | null | undefined): AgentIdResolution {
  if (typeof agentId !== "string" || agentId.length === 0) {
    return { kind: "unknown", raw: String(agentId) };
  }
  // Worker with task suffix — split on the FIRST colon after the
  // 'worker' token so 'agent:worker:t0' → worker task t0.
  if (agentId.startsWith("agent:worker:")) {
    const taskId = agentId.slice("agent:worker:".length);
    if (taskId.length > 0) {
      return { kind: "worker_task", taskId };
    }
    return { kind: "role", role: "worker" };
  }
  if (agentId === "agent:worker") {
    return { kind: "role", role: "worker" };
  }
  if (agentId === "agent:chief") {
    return { kind: "role", role: "chief" };
  }
  if (agentId === "agent:critic:a") {
    return { kind: "role", role: "critic-a" };
  }
  if (agentId === "agent:critic:b") {
    return { kind: "role", role: "critic-b" };
  }
  if (agentId === "agent:system") {
    return { kind: "system" };
  }
  return { kind: "unknown", raw: agentId };
}

/**
 * Map a decoded agent id straight to the dashboard source label
 * (`ConsoleSource` — used by BottomConsole.tsx to colour-code
 * console lines). Unknown / system / worker_task all collapse to
 * the same source bucket the previous inline if/else used.
 */
export function agentIdToConsoleSource(agentId: string | null | undefined): ConsoleSource {
  const r = decodeAgentId(agentId);
  switch (r.kind) {
    case "role":
      return r.role;
    case "worker_task":
      return "worker";
    case "system":
      return "system";
    case "unknown":
      return "system";
  }
}

/**
 * Convenience: does this agent id correspond to a dashboard card?
 * Returns the role id (`'chief'` / `'critic-a'` / `'critic-b'` /
 * `'worker'`) for single-role agents, or `null` for workers-with-
 * task-id (which the dashboard tracks per-task, not per-role) and
 * for system / unknown ids.
 */
export function agentIdToHeadRole(agentId: string | null | undefined): AgentRole | null {
  const r = decodeAgentId(agentId);
  return r.kind === "role" ? r.role : null;
}
