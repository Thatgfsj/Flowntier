/**
 * StartupScreen — blocks until the runtime is reachable.
 *
 * Runs health checks in a loop. Once connected, calls onReady exactly once.
 * Uses a ref to avoid re-triggering the effect on callback identity changes.
 */

import { useCallback, useEffect, useRef, useState } from 'react';

const RUNTIME_URL = 'http://127.0.0.1:7317';
const MAX_ATTEMPTS = 30;
const CHECK_INTERVAL = 2000;

interface StartupScreenProps {
  onReady: () => void;
}

export function StartupScreen({ onReady }: StartupScreenProps) {
  const [status, setStatus] = useState<'connecting' | 'retrying' | 'failed'>('connecting');
  const [attempts, setAttempts] = useState(0);
  const onReadyRef = useRef(onReady);
  onReadyRef.current = onReady;

  useEffect(() => {
    let cancelled = false;
    let done = false;

    const checkOnce = async (): Promise<boolean> => {
      try {
        const controller = new AbortController();
        const timer = setTimeout(() => controller.abort(), 3000);
        const r = await fetch(`${RUNTIME_URL}/health`, { signal: controller.signal });
        clearTimeout(timer);
        return r.ok;
      } catch {
        return false;
      }
    };

    const loop = async () => {
      for (let i = 1; i <= MAX_ATTEMPTS; i++) {
        if (cancelled || done) return;
        setAttempts(i);

        if (await checkOnce()) {
          if (!cancelled && !done) {
            done = true;
            onReadyRef.current();
          }
          return;
        }

        setStatus(i > 3 ? 'retrying' : 'connecting');
        await new Promise(r => setTimeout(r, CHECK_INTERVAL));
      }

      if (!cancelled) setStatus('failed');
    };

    void loop();
    return () => { cancelled = true; };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const handleRetry = useCallback(() => {
    setStatus('connecting');
    setAttempts(0);
    window.location.reload();
  }, []);

  return (
    <div className="flex h-screen items-center justify-center bg-surface-1">
      <div className="flex flex-col items-center gap-6">
        <div className="text-4xl font-bold text-primary">Agent Company OS</div>
        <div className="text-sm text-text-secondary">AI 软件公司操作系统</div>

        <div className="mt-4 flex flex-col items-center gap-3">
          {status !== 'failed' && (
            <div className="h-8 w-8 animate-spin rounded-full border-2 border-border border-t-chief" />
          )}

          {status === 'connecting' && (
            <div className="text-sm text-text-secondary">
              正在启动 Python 运行时... ({attempts}/{MAX_ATTEMPTS})
            </div>
          )}

          {status === 'retrying' && (
            <div className="text-sm text-status-warn">
              启动较慢，请稍候... ({attempts}/{MAX_ATTEMPTS})
            </div>
          )}

          {status === 'failed' && (
            <div className="flex flex-col items-center gap-3">
              <div className="text-sm text-status-failed">
                无法连接到 Python 运行时
              </div>
              <button
                type="button"
                onClick={handleRetry}
                className="rounded bg-chief px-4 py-2 text-sm text-white hover:bg-chief/90"
              >
                重试
              </button>
            </div>
          )}
        </div>

        {status !== 'failed' && (
          <div className="w-64 overflow-hidden rounded-full bg-surface-3">
            <div
              className="h-1 bg-chief transition-all duration-500"
              style={{ width: `${Math.min(100, (attempts / MAX_ATTEMPTS) * 100)}%` }}
            />
          </div>
        )}
      </div>
    </div>
  );
}
