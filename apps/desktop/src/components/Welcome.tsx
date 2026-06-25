/**
 * Welcome — 3-step first-run flow.
 *
 * Renders only on first launch (or after a factory reset). The
 * "first run" flag lives in the kv table; once dismissed, it
 * stays false across launches.
 *
 * Steps:
 *   1. Provider quick-add. If the user has 0 saved API keys, the
 *      full QuickAddAI wizard from Settings.tsx is rendered inline.
 *      If they have at least one, the step is skipped.
 *   2. Sample workflow. A pre-baked "implement POST /auth/login"
 *      task loads via load_sample_workflow(); one click submits
 *      it as a real workflow via run_agent_task.
 *   3. Enter workspace. Marks first_run=false and hands control
 *      back to App.
 *
 * Visually, three big card-steps with a thin progress bar at the
 * top. The user can skip a step (e.g. they want to set up a key
 * later) but they can't skip step 3 (that's the "I accept the
 * app is open for real" button).
 */
import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

interface WelcomeProps {
  onComplete: () => void;
}

interface SampleWorkflow {
  name: string;
  display_name: string;
  description: string;
  user_request: string;
  expected_tasks: string[];
}

export function Welcome({ onComplete }: WelcomeProps) {
  const [step, setStep] = useState<1 | 2 | 3>(1);
  const [providers, setProviders] = useState<
    { id: string; display_name: string; has_secret: boolean }[]
  >([]);
  const [sample, setSample] = useState<SampleWorkflow | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [submitErr, setSubmitErr] = useState<string | null>(null);
  const [submitOk, setSubmitOk] = useState<string | null>(null);

  // Step 1: load providers so we know if any has a key.
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const resp = await invoke<{ providers: Array<{ id: string; display_name: string; has_secret: boolean }> }>(
          'list_providers',
        );
        if (!cancelled) {
          setProviders(resp.providers);
          const anyHasKey = resp.providers.some((p) => p.has_secret);
          if (anyHasKey) {
            // Skip step 1 entirely.
            setStep(2);
          }
        }
      } catch (e) {
        console.warn('[Welcome] list_providers failed:', e);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  // Step 2: load the sample on entry.
  useEffect(() => {
    if (step !== 2 || sample) return;
    let cancelled = false;
    void (async () => {
      try {
        const wf = await invoke<SampleWorkflow>('load_sample_workflow');
        if (!cancelled) setSample(wf);
      } catch (e) {
        console.warn('[Welcome] load_sample_workflow failed:', e);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [step, sample]);

  const submitSample = async () => {
    if (!sample) return;
    setSubmitting(true);
    setSubmitErr(null);
    try {
      const resp = await invoke<{ id: string }>('start_workflow', {
        request: { text: sample.user_request },
      });
      setSubmitOk(resp.id);
    } catch (e) {
      setSubmitErr(String(e));
    } finally {
      setSubmitting(false);
    }
  };

  const finish = async () => {
    try {
      await invoke('first_run_complete');
    } catch (e) {
      console.warn('[Welcome] first_run_complete failed:', e);
    }
    onComplete();
  };

  return (
    <div className="flex h-screen w-screen flex-col items-center justify-start overflow-y-auto bg-surface-1 px-6 py-10 text-text-primary">
      {/* Top progress bar: 3 dots */}
      <div className="mb-8 flex items-center gap-2">
        <ProgressDot active={step >= 1} label="添加供应商" />
        <Connector />
        <ProgressDot active={step >= 2} label="示例任务" />
        <Connector />
        <ProgressDot active={step >= 3} label="开始使用" />
      </div>

      <div className="w-full max-w-2xl">
        {step === 1 && (
          <Step1
            providers={providers}
            onSkip={() => setStep(2)}
            onSaved={() => setStep(2)}
          />
        )}
        {step === 2 && (
          <Step2
            sample={sample}
            submitting={submitting}
            submitErr={submitErr}
            submitOk={submitOk}
            onSubmit={submitSample}
            onSkip={() => setStep(3)}
            onBack={() => setStep(1)}
          />
        )}
        {step === 3 && <Step3 onEnter={finish} onBack={() => setStep(2)} />}
      </div>
    </div>
  );
}

// ── Step 1 ──────────────────────────────────────────────────────────────

function Step1(props: {
  providers: { id: string; display_name: string; has_secret: boolean }[];
  onSkip: () => void;
  onSaved: () => void;
}) {
  const noneSaved = props.providers.length > 0 && !props.providers.some((p) => p.has_secret);
  return (
    <Card title="第 1 步 — 添加一个 AI 供应商" subtitle="首次使用需要至少一个 API Key。可以现在添加，也可以稍后到设置中添加。">
      {props.providers.length === 0 ? (
        <p className="text-sm text-text-secondary">加载供应商列表…</p>
      ) : noneSaved ? (
        <ProviderQuickAdd onSaved={props.onSaved} />
      ) : (
        <p className="text-sm text-text-secondary">
          ✓ 检测到至少一个供应商已配置。继续。
        </p>
      )}
      <Footer
        onBack={undefined}
        onNext={props.onSkip}
        nextLabel="跳过此步"
      />
    </Card>
  );
}

// Minimal inline provider picker. For brevity, only the 4 most
// common presets; full list in Settings.tsx.
function ProviderQuickAdd(props: { onSaved: () => void }) {
  const presets: Array<{ id: string; display_name: string; secret_name: string; base_url: string }> = [
    { id: 'openai', display_name: 'OpenAI', secret_name: 'OPENAI_API_KEY', base_url: 'https://api.openai.com/v1' },
    { id: 'anthropic', display_name: 'Anthropic', secret_name: 'ANTHROPIC_API_KEY', base_url: 'https://api.anthropic.com' },
    { id: 'google', display_name: 'Google AI (Gemini)', secret_name: 'GOOGLE_API_KEY', base_url: 'https://generativelanguage.googleapis.com/v1beta/openai' },
    { id: 'deepseek', display_name: 'DeepSeek', secret_name: 'DEEPSEEK_API_KEY', base_url: 'https://api.deepseek.com' },
  ];
  const [picked, setPicked] = useState<(typeof presets)[number] | null>(null);
  const [key, setKey] = useState('');
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  if (!picked) {
    return (
      <div className="grid grid-cols-2 gap-3">
        {presets.map((p) => (
          <button
            key={p.id}
            type="button"
            onClick={() => setPicked(p)}
            className="rounded-md border border-border bg-surface-2 px-4 py-3 text-left transition-colors hover:bg-surface-3 focus:outline-none focus:ring-2 focus:ring-accent/50"
          >
            <div className="text-sm font-medium text-text-primary">{p.display_name}</div>
            <div className="mt-1 text-xs text-text-secondary">{p.secret_name}</div>
          </button>
        ))}
      </div>
    );
  }

  const save = async () => {
    if (!key.trim()) return;
    setBusy(true);
    setErr(null);
    try {
      await invoke('save_secret', { name: picked.secret_name, value: key.trim() });
      props.onSaved();
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="space-y-3">
      <div className="flex items-baseline gap-2 text-sm">
        <span className="text-text-secondary">已选:</span>
        <span className="font-medium text-text-primary">{picked.display_name}</span>
        <button
          type="button"
          onClick={() => {
            setPicked(null);
            setKey('');
          }}
          className="text-xs text-text-secondary underline hover:text-text-primary"
        >
          换一个
        </button>
      </div>
      <input
        type="password"
        value={key}
        onChange={(e) => setKey(e.target.value)}
        placeholder={`${picked.secret_name}（粘到此处）`}
        className="w-full rounded-md border border-border bg-surface-2 px-3 py-2 font-mono text-xs focus:outline-none focus:ring-2 focus:ring-accent/50"
        autoFocus
      />
      {err && <p className="text-xs text-error">{err}</p>}
      <div className="flex justify-end">
        <button
          type="button"
          onClick={() => void save()}
          disabled={busy || !key.trim()}
          className="rounded-md bg-accent px-4 py-2 text-sm font-medium text-white transition-opacity hover:opacity-90 disabled:opacity-50"
        >
          {busy ? '保存中…' : '保存并继续'}
        </button>
      </div>
    </div>
  );
}

// ── Step 2 ──────────────────────────────────────────────────────────────

function Step2(props: {
  sample: SampleWorkflow | null;
  submitting: boolean;
  submitErr: string | null;
  submitOk: string | null;
  onSubmit: () => void;
  onSkip: () => void;
  onBack: () => void;
}) {
  return (
    <Card title="第 2 步 — 试试示例任务" subtitle="下面是一个真实的工作流示例，会走完 首席 → 规划 → 工匠 → 审查 → 汇报 五个角色。">
      {!props.sample ? (
        <p className="text-sm text-text-secondary">加载示例任务…</p>
      ) : (
        <>
          <div className="rounded-md border border-border bg-surface-2 p-3 text-sm">
            <div className="font-medium text-text-primary">{props.sample.display_name}</div>
            <p className="mt-2 whitespace-pre-line text-text-secondary">
              {props.sample.description}
            </p>
            <details className="mt-3">
              <summary className="cursor-pointer text-xs text-text-secondary hover:text-text-primary">
                查看任务内容
              </summary>
              <pre className="mt-2 whitespace-pre-wrap text-xs text-text-secondary">
                {props.sample.user_request}
              </pre>
            </details>
          </div>
          {props.submitOk && (
            <p className="mt-3 rounded-md border border-success bg-success/10 px-3 py-2 text-xs text-success">
              ✓ 已提交，工作流 id: {props.submitOk}
            </p>
          )}
          {props.submitErr && (
            <p className="mt-3 rounded-md border border-error bg-error/10 px-3 py-2 text-xs text-error">
              提交失败: {props.submitErr}
            </p>
          )}
        </>
      )}
      <Footer
        onBack={props.onBack}
        onNext={props.onSubmit}
        nextLabel={props.submitting ? '提交中…' : '提交示例任务'}
        nextDisabled={props.submitting || !!props.submitOk}
        onSkip={props.onSkip}
        skipLabel="跳过，先到工作台"
      />
    </Card>
  );
}

// ── Step 3 ──────────────────────────────────────────────────────────────

function Step3(props: { onEnter: () => void; onBack: () => void }) {
  return (
    <Card title="第 3 步 — 进入工作台" subtitle="准备好了。可以随时到设置中添加更多供应商或自定义路由站。">
      <ul className="space-y-2 text-sm text-text-primary">
        <li>• 在底部命令栏键入指令，比如「给项目加单元测试」</li>
        <li>• 设置 → 供应商 管理 API Key 和自定义路由站</li>
        <li>• 设置 → 关于 查看版本号 + 日志路径 + 上报问题</li>
      </ul>
      <Footer
        onBack={props.onBack}
        onNext={props.onEnter}
        nextLabel="进入工作台 →"
      />
    </Card>
  );
}

// ── Shared UI ─────────────────────────────────────────────────────────

function Card(props: { title: string; subtitle?: string; children: React.ReactNode }) {
  return (
    <div className="rounded-lg border border-border bg-surface-1 p-6 shadow-sm">
      <h2 className="text-xl font-semibold text-text-primary">{props.title}</h2>
      {props.subtitle && (
        <p className="mt-1 text-sm text-text-secondary">{props.subtitle}</p>
      )}
      <div className="mt-5">{props.children}</div>
    </div>
  );
}

function ProgressDot(props: { active: boolean; label: string }) {
  return (
    <div className="flex flex-col items-center gap-1">
      <div
        className={`h-3 w-3 rounded-full transition-colors ${
          props.active ? 'bg-accent' : 'bg-border'
        }`}
      />
      <span className={`text-xs ${props.active ? 'text-text-primary' : 'text-text-secondary'}`}>
        {props.label}
      </span>
    </div>
  );
}

function Connector() {
  return <div className="h-px w-12 bg-border" />;
}

function Footer(props: {
  onBack: (() => void) | undefined;
  onNext: () => void;
  nextLabel: string;
  nextDisabled?: boolean;
  onSkip?: () => void;
  skipLabel?: string;
}) {
  return (
    <div className="mt-6 flex items-center justify-between">
      {props.onBack ? (
        <button
          type="button"
          onClick={props.onBack}
          className="text-sm text-text-secondary hover:text-text-primary"
        >
          ← 上一步
        </button>
      ) : (
        <span />
      )}
      <div className="flex items-center gap-3">
        {props.onSkip && (
          <button
            type="button"
            onClick={props.onSkip}
            className="text-sm text-text-secondary hover:text-text-primary"
          >
            {props.skipLabel ?? '跳过'}
          </button>
        )}
        <button
          type="button"
          onClick={props.onNext}
          disabled={props.nextDisabled}
          className="rounded-md bg-accent px-4 py-2 text-sm font-medium text-white transition-opacity hover:opacity-90 disabled:opacity-50"
        >
          {props.nextLabel}
        </button>
      </div>
    </div>
  );
}