/**
 * Workflow simulator — drives the Mission Control UI through a
 * scripted 8-phase workflow without any real LLM. Used in v0.2
 * until the Python runtime's WebSocket bridge lands.
 *
 * Each `step()` publishes a delta to the host via callbacks. The
 * host is responsible for translating those deltas into
 * `WfEvent` / `PhaseState` / `TaskState` / `AgentStatus` updates.
 */

export type SimPhaseId =
  | 'requirement'
  | 'planning'
  | 'plan_review'
  | 'dispatch'
  | 'development'
  | 'review'
  | 'repair'
  | 'delivery';

export type SimTaskState =
  | 'PENDING'
  | 'DISPATCHED'
  | 'IN_PROGRESS'
  | 'SUBMITTED'
  | 'UNDER_REVIEW'
  | 'REPAIR_REQUESTED'
  | 'REPAIRING'
  | 'APPROVED'
  | 'DONE'
  | 'FAILED'
  | 'ABORTED';

export interface SimTask {
  id: string;
  title: string;
  owner: string;
  fileHint?: string;
  /** Duration in ms that this task takes in `IN_PROGRESS`. */
  durationMs: number;
  /** Outcome of the task: true = DONE, false = REPAIR_REQUESTED. */
  passesFirstTry: boolean;
}

export interface SimAgentStatus {
  chief: 'idle' | 'thinking' | 'speaking';
  'critic-a': 'idle' | 'thinking' | 'speaking';
  'critic-b': 'idle' | 'thinking' | 'speaking';
  worker: 'idle' | 'thinking' | 'speaking';
}

export interface SimEvent {
  /** Console log line (agent_id, level, message). */
  log?: { agent_id: string; level: 'info' | 'warn' | 'error' | 'debug'; message: string };
  /** Phase changed to this name + new state. */
  phase?: { name: SimPhaseId; state: 'pending' | 'active' | 'done' | 'failed' };
  /** Task transitioned. */
  task?: { id: string; state: SimTaskState };
  /** Agent status change. */
  agent?: Partial<SimAgentStatus>;
  /** Milestone label (shown on the timeline). */
  milestone?: string;
  /** Workflow is done. */
  done?: { status: 'DONE' | 'FAILED' | 'ABORTED' };
}

export interface SimulatorCallbacks {
  onEvent: (event: SimEvent) => void;
  onComplete: () => void;
}

export interface SimOptions {
  /** Speed multiplier; 1.0 = default (~12s total), 2.0 = twice as fast. */
  speed?: number;
  /** If true, the third task fails its first review (triggers REPAIR). */
  triggerRepair?: boolean;
}

const TASKS: SimTask[] = [
  {
    id: 't1',
    title: '后端：实现 /login 接口',
    owner: '执行员 1',
    fileHint: 'src/auth/login.py',
    durationMs: 1600,
    passesFirstTry: true,
  },
  {
    id: 't2',
    title: '前端：LoginForm 组件',
    owner: '执行员 2',
    fileHint: 'src/components/LoginForm.tsx',
    durationMs: 1400,
    passesFirstTry: true,
  },
  {
    id: 't3',
    title: '数据库：users 表迁移',
    owner: '执行员 3',
    fileHint: 'migrations/0001_users.sql',
    durationMs: 1200,
    passesFirstTry: false, // will be repaired
  },
  {
    id: 't4',
    title: '测试：登录流程端到端',
    owner: '执行员 4',
    fileHint: 'tests/e2e/test_login.py',
    durationMs: 1000,
    passesFirstTry: true,
  },
];

/** Schedule `fn` to run after `ms * speed` ms (cancellable). */
function delay(ms: number, speed: number, fn: () => void): () => void {
  const t = window.setTimeout(fn, ms / Math.max(0.1, speed));
  return () => window.clearTimeout(t);
}

/** Start the simulator. Returns a stop function. */
export function startSimulation(
  callbacks: SimulatorCallbacks,
  options: SimOptions = {},
): () => void {
  const speed = options.speed ?? 1.0;
  const triggerRepair = options.triggerRepair ?? true;
  const cancels: Array<() => void> = [];
  const cancelAll = () => {
    for (const c of cancels) c();
  };

  const fire = (event: SimEvent) => callbacks.onEvent(event);
  const log = (agent_id: string, message: string, level: 'info' | 'warn' | 'error' | 'debug' = 'info') =>
    fire({ log: { agent_id, level, message } });
  const phase = (name: SimPhaseId, state: 'pending' | 'active' | 'done') =>
    fire({ phase: { name, state } });
  const task = (id: string, state: SimTaskState) => fire({ task: { id, state } });
  const agent = (a: Partial<SimAgentStatus>) => fire({ agent: a });
  const milestone = (label: string) => fire({ milestone: label });

  // ── Phase 1: Requirement ─────────────────────────────────────
  log('agent:user', '收到用户请求：实现登录接口', 'info');
  cancels.push(delay(200, speed, () => milestone('收到用户请求')));

  cancels.push(delay(300, speed, () => phase('requirement', 'active')));
  cancels.push(
    delay(500, speed, () => {
      agent({ chief: 'thinking' });
      log('agent:chief', '分析用户意图：用户希望实现 POST /auth/login');
    }),
  );
  cancels.push(
    delay(1200, speed, () => {
      log('agent:chief', '需求已澄清，无歧义');
      agent({ chief: 'idle' });
      phase('requirement', 'done');
      phase('planning', 'active');
      milestone('开始规划');
    }),
  );

  // ── Phase 2: Planning ────────────────────────────────────────
  cancels.push(
    delay(1700, speed, () => {
      agent({ chief: 'thinking' });
      log('agent:chief', '草拟 4 个任务的计划：后端 /login、前端 LoginForm、数据库 users 表、测试');
    }),
  );
  cancels.push(
    delay(2600, speed, () => {
      log('agent:chief', '计划已生成');
      agent({ chief: 'idle' });
      phase('planning', 'done');
      phase('plan_review', 'active');
      milestone('计划已生成，提交审核');
    }),
  );

  // ── Phase 3: Plan Review ─────────────────────────────────────
  cancels.push(
    delay(3100, speed, () => {
      agent({ 'critic-a': 'thinking' });
      log('agent:critic:a', '正在审核计划：边界、接口、依赖关系...');
    }),
  );
  cancels.push(
    delay(4000, speed, () => {
      agent({ 'critic-a': 'idle' });
      agent({ 'critic-b': 'thinking' });
      log('agent:critic:b', '审核架构：模块划分是否合理');
    }),
  );
  cancels.push(
    delay(4800, speed, () => {
      log('agent:critic:b', 'PASS：模块边界清晰');
      agent({ 'critic-b': 'idle' });
      phase('plan_review', 'done');
      phase('dispatch', 'active');
      milestone('计划已批准');
    }),
  );

  // ── Phase 4: Dispatch + 5: Development ───────────────────────
  cancels.push(
    delay(5200, speed, () => {
      log('agent:chief', '派发任务给 4 个执行员');
      for (const t of TASKS) task(t.id, 'DISPATCHED');
    }),
  );
  cancels.push(
    delay(5400, speed, () => {
      phase('dispatch', 'done');
      phase('development', 'active');
    }),
  );

  // Sequential task execution (parallel in production, but for the
  // simulator we run them in order so the UI state is legible).
  let cursor = 5800;
  for (let i = 0; i < TASKS.length; i++) {
    const t = TASKS[i]!;
    const start = cursor;
    cancels.push(
      delay(start, speed, () => {
        task(t.id, 'IN_PROGRESS');
        agent({ worker: 'thinking' });
        log('agent:worker', `开始任务：${t.title}`);
      }),
    );
    const endSubmit = start + t.durationMs;
    cancels.push(
      delay(endSubmit, speed, () => {
        task(t.id, 'SUBMITTED');
        log('agent:worker', `完成：${t.title}`);
      }),
    );
    const endReview = endSubmit + 600;
    cancels.push(
      delay(endReview, speed, () => {
        task(t.id, 'UNDER_REVIEW');
        agent({ 'critic-a': 'thinking' });
        log('agent:critic:a', `正在评审：${t.title}`);
      }),
    );
    const shouldFail = triggerRepair && !t.passesFirstTry;
    const endCritic = endReview + 500;
    if (shouldFail) {
      cancels.push(
        delay(endCritic, speed, () => {
          task(t.id, 'REPAIR_REQUESTED');
          log('agent:critic:a', '发现问题：缺少主键约束');
          agent({ 'critic-a': 'idle' });
        }),
      );
      const endRepair = endCritic + 500;
      cancels.push(
        delay(endRepair, speed, () => {
          task(t.id, 'REPAIRING');
          log('agent:worker', '修复中...');
        }),
      );
      const endDone = endRepair + 800;
      cancels.push(
        delay(endDone, speed, () => {
          task(t.id, 'DONE');
          log('agent:worker', '修复完成');
        }),
      );
      cursor = endDone + 100;
    } else {
      cancels.push(
        delay(endCritic, speed, () => {
          task(t.id, 'APPROVED');
          log('agent:critic:a', 'PASS');
          agent({ 'critic-a': 'idle' });
        }),
      );
      cursor = endCritic + 100;
    }
  }

  // ── Phase 6: Review (final) ─────────────────────────────────
  cancels.push(
    delay(cursor, speed, () => {
      task(TASKS[TASKS.length - 1]!.id, 'DONE');
      for (const t of TASKS) task(t.id, 'DONE');
      log('agent:chief', '所有任务完成，开始最终评审');
      phase('development', 'done');
      phase('review', 'active');
      milestone('进入最终评审');
    }),
  );

  // ── Phase 7: Repair (skipped unless something was repaired
  //    — in this simulation, t3 was repaired, so the workflow
  //    already had a repair sub-loop, no extra review pass needed) ─

  // ── Phase 8: Delivery ───────────────────────────────────────
  const deliveryAt = cursor + 1000;
  cancels.push(
    delay(deliveryAt, speed, () => {
      phase('review', 'done');
      phase('delivery', 'active');
      log('agent:chief', '生成最终交付摘要');
      agent({ chief: 'thinking' });
    }),
  );
  cancels.push(
    delay(deliveryAt + 800, speed, () => {
      log('agent:chief', '摘要已生成：4 个任务全部完成');
      agent({ chief: 'idle' });
      phase('delivery', 'done');
      milestone('✓ 全部完成');
    }),
  );
  cancels.push(
    delay(deliveryAt + 1100, speed, () => {
      fire({ done: { status: 'DONE' } });
      callbacks.onComplete();
    }),
  );

  return cancelAll;
}
