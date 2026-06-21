/**
 * PluginsPanel — shows available plugins and allows invocation.
 */

import { useState, useEffect, useCallback } from 'react';
import { Card } from '@aco/ui';
import { listPlugins, invokePlugin, type PluginDescriptor } from '../lib/api.js';

interface PluginResult {
  status: string;
  message?: string;
  [key: string]: unknown;
}

export function PluginsPanel() {
  const [plugins, setPlugins] = useState<PluginDescriptor[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [selectedPlugin, setSelectedPlugin] = useState<string | null>(null);
  const [selectedAction, setSelectedAction] = useState<string | null>(null);
  const [argsInput, setArgsInput] = useState('{}');
  const [result, setResult] = useState<PluginResult | null>(null);
  const [invoking, setInvoking] = useState(false);

  const fetchPlugins = useCallback(async () => {
    try {
      setLoading(true);
      setError(null);
      setPlugins(await listPlugins());
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to fetch plugins');
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => { void fetchPlugins(); }, [fetchPlugins]);

  const handleInvoke = async () => {
    if (!selectedPlugin || !selectedAction) return;
    try {
      setInvoking(true);
      setResult(null);
      let args: Record<string, unknown> = {};
      try { args = JSON.parse(argsInput); } catch { setResult({ status: 'error', message: 'Invalid JSON' }); return; }
      const data = await invokePlugin(selectedPlugin, { action: selectedAction, ...args });
      setResult(data as PluginResult);
    } catch (e) {
      setResult({ status: 'error', message: e instanceof Error ? e.message : 'Invocation failed' });
    } finally {
      setInvoking(false);
    }
  };

  if (loading) return <Card><div className="text-sm text-text-secondary">加载插件中...</div></Card>;
  if (error) return <Card><div className="text-sm text-status-failed">错误: {error}</div><button onClick={fetchPlugins} className="mt-2 text-xs text-status-active hover:underline">重试</button></Card>;

  const selected = plugins.find((p) => p.name === selectedPlugin);

  return (
    <Card>
      <h3 className="mb-2 text-sm font-semibold">插件</h3>
      <div className="mb-3 flex flex-wrap gap-1.5">
        {plugins.map((p) => (
          <button key={p.name} type="button" onClick={() => { setSelectedPlugin(p.name); setSelectedAction(null); setResult(null); }}
            className={`rounded px-2 py-1 text-xs transition-colors ${selectedPlugin === p.name ? 'bg-status-active text-white' : 'bg-surface-3 text-text-secondary hover:bg-surface-2'}`}>
            {p.name}
          </button>
        ))}
      </div>

      {selected && (
        <div className="space-y-2">
          <p className="text-xs text-text-secondary">{selected.description}</p>
          <div>
            <label className="mb-1 block text-xs font-medium">操作</label>
            <div className="flex flex-wrap gap-1">
              {selected.actions.map((a) => (
                <button key={a} type="button" onClick={() => { setSelectedAction(a); setResult(null); }}
                  className={`rounded px-1.5 py-0.5 text-[10px] transition-colors ${selectedAction === a ? 'bg-status-active text-white' : 'bg-surface-3 text-text-secondary hover:bg-surface-2'}`}>
                  {a}
                </button>
              ))}
            </div>
          </div>

          {selectedAction && (
            <>
              <div>
                <label className="mb-1 block text-xs font-medium">参数 (JSON)</label>
                <textarea value={argsInput} onChange={(e) => setArgsInput(e.target.value)}
                  className="h-20 w-full rounded border border-border bg-surface-1 p-2 font-mono text-xs" placeholder='{"key": "value"}' />
              </div>
              <button type="button" onClick={handleInvoke} disabled={invoking}
                className="rounded bg-status-active px-3 py-1.5 text-xs text-white hover:bg-status-active/80 disabled:opacity-50">
                {invoking ? '执行中...' : '执行'}
              </button>
            </>
          )}

          {result && (
            <div className="rounded bg-surface-2 p-2">
              <div className="mb-1 text-xs font-medium">
                结果: <span className={result.status === 'ok' ? 'text-status-done' : 'text-status-failed'}>{result.status}</span>
              </div>
              <pre className="max-h-40 overflow-auto whitespace-pre-wrap font-mono text-[11px] text-primary">{JSON.stringify(result, null, 2)}</pre>
            </div>
          )}
        </div>
      )}

      {!selected && <p className="text-xs text-text-secondary">选择一个插件查看详情</p>}
    </Card>
  );
}
