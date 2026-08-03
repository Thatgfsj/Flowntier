// apps/desktop/src/contexts/workflowReducer.test.ts
//
// v0.4.29: minimal coverage of the reduced `applyEvent` switch.
// The reducer used to live entirely inside App.tsx and was
// effectively untested — every event-driven UX bug was a
// manual-TestRail pass. Six cases here:
//   1. chief text_delta flips role to 'speaking' with `since`
//   2. chief tool_started flips role to 'thinking'
//   3. worker-task text_delta updates workerTaskStatus[taskId]
//   4. TICK flips stale non-idle role back to idle after 8s
//   5. TICK does NOT flip a freshly-updated role
//   6. reviewer_verdict final-review sets reviewVerdict.coerced
//   7. task_status upsert preserves title across state-only updates
//   8. RESET clears workflow-only state but keeps workdir
//   9. START_WORKFLOW busy=true + clears events
//  10. unknown agent_id on text_delta does NOT crash

import { describe, expect, it } from 'vitest';
import {
  INITIAL_STATE,
  workflowReducer,
  type RootState,
} from './workflowReducer.js';
import type { WfEvent } from '@flowntier/shared';

function reducer(state: RootState | undefined, event: WfEvent): RootState {
  return workflowReducer(state ?? INITIAL_STATE, { type: 'EVENT', event });
}

const baseChiefSpeech: WfEvent = {
  kind: 'text_delta',
  agent_id: 'agent:chief',
  agent_display: '主管',
  delta: '正在为需求阶段生成产出...',
  task_id: null,
};

const baseChiefToolStarted: WfEvent = {
  kind: 'tool_started',
  agent_id: 'agent:chief',
  agent_display: '主管',
  call: { id: 'tool-1', name: 'write_file', args: { path: 'plan.md' } },
  task_id: null,
};

const baseWorkerText: WfEvent = {
  kind: 'text_delta',
  agent_id: 'agent:worker:t0',
  agent_display: '执行者 t0',
  delta: 'fetching deps...',
  task_id: 't0',
};

const baseFinalReviewVerdict: WfEvent = {
  kind: 'reviewer_verdict',
  wf_id: 'wf-test',
  role: 'agent:critic:a',
  phase: 'final-review',
  verdict: 'PASS',
  summary: '通过',
  confidence: 0.93,
  issues: [],
};

const baseTaskStatus: WfEvent = {
  kind: 'task_status',
  ts: '2026-07-31T00:00:00Z',
  task_id: 't0',
  task_title: '写 dom 解析',
  task_status: 'RUNNING',
};

const denyTaskStatus: WfEvent = {
  kind: 'task_status',
  ts: '2026-07-31T00:00:00Z',
  task_id: 't0',
  task_title: '',
  task_status: 'DONE',
  task_summary: 'dom 解析完成',
};

describe('workflowReducer', () => {
  it('chief text_delta flips agentStatus.chief to speaking with since', () => {
    const r = reducer(INITIAL_STATE, baseChiefSpeech);
    expect(r.agentStatus.chief.status).toBe('speaking');
    expect(r.agentStatus.chief.since).toBeGreaterThan(0);
    expect(r.agentStatus['critic-a'].status).toBe('idle');
    expect(r.events).toHaveLength(1);
  });

  it('chief tool_started flips agentStatus.chief to thinking', () => {
    const r = reducer(INITIAL_STATE, baseChiefToolStarted);
    expect(r.agentStatus.chief.status).toBe('thinking');
  });

  it('worker-task text_delta updates workerTaskStatus[task_id]', () => {
    const r = reducer(INITIAL_STATE, baseWorkerText);
    expect(r.workerTaskStatus['t0']).toBe('speaking');
    // chief and critic remain idle — worker is its own bucket
    expect(r.agentStatus.chief.status).toBe('idle');
  });

  it('TICK flips a stale non-idle role back to idle', () => {
    const started = reducer(INITIAL_STATE, baseChiefSpeech);
    const later = started.agentStatus.chief.since + 9_000;
    const r = workflowReducer(started, { type: 'TICK', now: later });
    expect(r.agentStatus.chief.status).toBe('idle');
  });

  it('TICK does NOT flip a freshly-updated role', () => {
    const started = reducer(INITIAL_STATE, baseChiefSpeech);
    const now = started.agentStatus.chief.since + 1_000;
    const r = workflowReducer(started, { type: 'TICK', now });
    expect(r.agentStatus.chief.status).toBe('speaking');
  });

  it('reviewer_verdict final-review coerces verdict via PASS/REPAIR/REWRITE', () => {
    const r = reducer(INITIAL_STATE, baseFinalReviewVerdict);
    expect(r.reviewVerdict?.verdict).toBe('PASS');
    expect(r.reviewVerdict?.summary).toBe('通过');
    expect(r.reviewerVerdicts).toHaveLength(1);
  });

  it('task_status upsert preserves title across state-only updates', () => {
    const a = reducer(INITIAL_STATE, baseTaskStatus);
    expect(a.tasks).toHaveLength(1);
    expect(a.tasks[0]?.title).toBe('写 dom 解析');
    expect(a.tasks[0]?.state).toBe('RUNNING');
    const b = reducer(a, denyTaskStatus);
    expect(b.tasks).toHaveLength(1);
    expect(b.tasks[0]?.title).toBe('写 dom 解析');
    expect(b.tasks[0]?.state).toBe('DONE');
    expect(b.tasks[0]?.summary).toBe('dom 解析完成');
  });

  it('RESET clears workflow-only state but keeps workdir and busy', () => {
    const dirty: RootState = {
      ...INITIAL_STATE,
      busy: true,
      workdir: 'C:/work',
      tasks: [
        { id: 't0', title: 'x', owner: '', state: 'pending' },
      ],
      events: [baseChiefSpeech],
    };
    const r = workflowReducer(dirty, { type: 'RESET' });
    // busy is preserved — START_WORKFLOW is the call that flips it.
    expect(r.busy).toBe(true);
    expect(r.tasks).toHaveLength(0);
    expect(r.events).toHaveLength(0);
    expect(r.workdir).toBe('C:/work');
  });

  it('START_WORKFLOW flips busy=true and clears events', () => {
    const dirty: RootState = {
      ...INITIAL_STATE,
      busy: false,
      events: [baseChiefSpeech],
    };
    const r = workflowReducer(dirty, { type: 'START_WORKFLOW' });
    expect(r.busy).toBe(true);
    expect(r.events).toHaveLength(0);
  });

  it('unknown agent_id on text_delta does not crash', () => {
    const weird: WfEvent = {
      kind: 'text_delta',
      agent_id: 'agent:something-weird',
      agent_display: '???',
      delta: '???',
      task_id: null,
    };
    const r = reducer(INITIAL_STATE, weird);
    expect(r.agentStatus.chief.status).toBe('idle');
    expect(r.events).toHaveLength(1);
  });
});
