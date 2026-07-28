/**
 * useDisabledModels — tracks the user's per-provider model deletions.
 *
 * When a model is no longer needed (e.g. an expired mimo API key, a
 * deprecated preview model), the user clicks the × in the Settings
 * provider detail view and we call the backend
 * `PUT /api/providers/{id}/models/{model}/disable` endpoint. The
 * backend persists the disabled pair in SQLite
 * (`disabled_models` table, see migration 0006) and filters it out
 * of every subsequent `listRouterModels` / `listModels` response.
 *
 * For optimistic UI we maintain a local Set<(providerId, modelId)>
 * and mirror it into the server via `disableProviderModel` /
 * `enableProviderModel`. We deliberately do NOT mirror to
 * localStorage: the server already persists, and crossing storage
 * boundaries invites drift on the host/guest side of the Tauri
 * bridge.
 *
 * v0.4.22 (event 000110): backend filtering already excludes
 * disabled models from the catalog, so this hook is a thin write-side
 * facade. The frontend still wants the Set for tooltip / restore UI.
 */

import { useCallback, useEffect, useState } from 'react';
import { disableProviderModel, enableProviderModel } from '../lib/api.js';

type Key = `${string}::${string}`;

function k(providerId: string, modelId: string): Key {
  return `${providerId}::${modelId}`;
}

export function useDisabledModels() {
  const [set, setSet] = useState<Set<Key>>(() => new Set());
  const [busy, setBusy] = useState(false);

  const isDisabled = useCallback(
    (providerId: string, modelId: string) => set.has(k(providerId, modelId)),
    [set],
  );

  const disable = useCallback(async (providerId: string, modelId: string) => {
    setBusy(true);
    try {
      await disableProviderModel(providerId, modelId);
      setSet((prev) => {
        const next = new Set(prev);
        next.add(k(providerId, modelId));
        return next;
      });
    } finally {
      setBusy(false);
    }
  }, []);

  const enable = useCallback(async (providerId: string, modelId: string) => {
    setBusy(true);
    try {
      await enableProviderModel(providerId, modelId);
      setSet((prev) => {
        const next = new Set(prev);
        next.delete(k(providerId, modelId));
        return next;
      });
    } finally {
      setBusy(false);
    }
  }, []);

  const count = set.size;

  // Reset on uninstall-clear via window event (defensive; not used yet).
  useEffect(() => {
    return () => { /* no-op cleanup */ };
  }, []);

  return { isDisabled, disable, enable, count, busy };
}
