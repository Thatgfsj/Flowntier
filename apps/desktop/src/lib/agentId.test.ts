// apps/desktop/src/lib/agentId.test.ts
//
// v0.4.29: minimal coverage for the shared agent_id decoder.
// Uses vitest's `expect` so it runs under `pnpm exec vitest run`.
//
// Run: `pnpm exec vitest run apps/desktop/src/lib/agentId.test.ts`

import { describe, expect, it } from "vitest";
import { agentIdToConsoleSource, agentIdToHeadRole, decodeAgentId } from "./agentId.js";

describe("decodeAgentId", () => {
  it("maps chief / critic-a / critic-b / worker single-token ids", () => {
    expect(decodeAgentId("agent:chief")).toEqual({ kind: "role", role: "chief" });
    expect(decodeAgentId("agent:critic:a")).toEqual({ kind: "role", role: "critic-a" });
    expect(decodeAgentId("agent:critic:b")).toEqual({ kind: "role", role: "critic-b" });
    expect(decodeAgentId("agent:worker")).toEqual({ kind: "role", role: "worker" });
  });

  it("splits worker task suffix off the prefix", () => {
    expect(decodeAgentId("agent:worker:t0")).toEqual({ kind: "worker_task", taskId: "t0" });
    expect(decodeAgentId("agent:worker:t12")).toEqual({ kind: "worker_task", taskId: "t12" });
  });

  it("falls back to worker role when the suffix is empty", () => {
    expect(decodeAgentId("agent:worker:")).toEqual({ kind: "role", role: "worker" });
  });

  it("treats agent:system as system (no card)", () => {
    expect(decodeAgentId("agent:system")).toEqual({ kind: "system" });
  });

  it("marks truly unknown ids as unknown (preserving the raw value)", () => {
    expect(decodeAgentId("agent:planner")).toEqual({ kind: "unknown", raw: "agent:planner" });
    expect(decodeAgentId("something:else")).toEqual({ kind: "unknown", raw: "something:else" });
  });

  it("treats empty / null / undefined as unknown", () => {
    expect(decodeAgentId("")).toEqual({ kind: "unknown", raw: "" });
    expect(decodeAgentId(null)).toEqual({ kind: "unknown", raw: "null" });
    expect(decodeAgentId(undefined)).toEqual({ kind: "unknown", raw: "undefined" });
  });
});

describe("agentIdToHeadRole", () => {
  it("returns the role for single-token head agents", () => {
    expect(agentIdToHeadRole("agent:chief")).toBe("chief");
    expect(agentIdToHeadRole("agent:critic:a")).toBe("critic-a");
    expect(agentIdToHeadRole("agent:critic:b")).toBe("critic-b");
    expect(agentIdToHeadRole("agent:worker")).toBe("worker");
  });

  it("returns null for per-task workers (the reducer uses workerTaskStatus instead)", () => {
    expect(agentIdToHeadRole("agent:worker:t0")).toBeNull();
  });

  it("returns null for system / unknown ids", () => {
    expect(agentIdToHeadRole("agent:system")).toBeNull();
    expect(agentIdToHeadRole("agent:planner")).toBeNull();
    expect(agentIdToHeadRole(null)).toBeNull();
  });
});

describe("agentIdToConsoleSource", () => {
  it("maps every worker task id to the worker source bucket", () => {
    expect(agentIdToConsoleSource("agent:worker")).toBe("worker");
    expect(agentIdToConsoleSource("agent:worker:t0")).toBe("worker");
    expect(agentIdToConsoleSource("agent:worker:t42")).toBe("worker");
  });

  it("passes chief / critic-a / critic-b through unchanged", () => {
    expect(agentIdToConsoleSource("agent:chief")).toBe("chief");
    expect(agentIdToConsoleSource("agent:critic:a")).toBe("critic-a");
    expect(agentIdToConsoleSource("agent:critic:b")).toBe("critic-b");
  });

  it("collapses unknown / system to the system bucket", () => {
    expect(agentIdToConsoleSource("agent:system")).toBe("system");
    expect(agentIdToConsoleSource("agent:planner")).toBe("system");
    expect(agentIdToConsoleSource("")).toBe("system");
  });
});
