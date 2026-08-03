// apps/desktop/src/contexts/DisabledModelsContext.tsx
//
// v0.4.35 (event 000135): the disabled-models Set used to live
// in a per-instance useState inside `useDisabledModels()` in
// hooks/useDisabledModels.ts. Every component that called the
// hook had its own Set — Settings mounted three times (one per
// provider detail row), each with an empty Set, so a fresh
// provider showed "× delete" buttons next to models that had
// already been hidden in a prior session.
//
// This lifts the state into a Context+Provider mounted once at
// the App root (alongside WorkflowProvider). On mount the
// Provider fetches `listDisabledModels()` from the new backend
// endpoint and seeds the Set. Subsequent `disable` / `enable`
// calls mutate the Set optimistically and persist through the
// existing API. The provider filter on the server side remains
// authoritative — disable failures no longer roll back the Set
// because the server is the source of truth.
//
// The exported `useDisabledModels()` hook signature is
// unchanged, so call sites (Settings.tsx:299) don't need to
// change their import path. The old hook file is now a thin
// re-export shim — see hooks/useDisabledModels.ts.

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from 'react';
import {
  disableProviderModel,
  enableProviderModel,
  listDisabledModels,
} from '../lib/api.js';

type Key = `${string}::${string}`;

function k(providerId: string, modelId: string): Key {
  return `${providerId}::${modelId}`;
}

export interface DisabledModelsContextValue {
  isDisabled: (providerId: string, modelId: string) => boolean;
  disable: (providerId: string, modelId: string) => Promise<void>;
  enable: (providerId: string, modelId: string) => Promise<void>;
  count: number;
  busy: boolean;
}

const DisabledModelsContext = createContext<DisabledModelsContextValue | null>(null);

export interface DisabledModelsProviderProps {
  children: ReactNode;
}

export function DisabledModelsProvider({ children }: DisabledModelsProviderProps) {
  const [set, setSet] = useState<Set<Key>>(() => new Set());
  const [busy, setBusy] = useState(false);

  // Hydrate from server once on mount. The server's
  // `disabled_models` table is the authoritative list; everything
  // we do after this is optimistic on top of that.
  useEffect(() => {
    let cancelled = false;
    listDisabledModels()
      .then((res) => {
        if (cancelled) return;
        const next = new Set<Key>();
        for (const row of res.models) {
          next.add(k(row.provider_id, row.model_id));
        }
        setSet(next);
      })
      .catch((err) => {
        // Logged but not fatal: Settings still works without the
        // initial Set (× icons appear, click still persists).
        console.error('[DisabledModels] hydrate failed:', err);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const isDisabled = useCallback(
    (providerId: string, modelId: string) => set.has(k(providerId, modelId)),
    [set],
  );

  const disable = useCallback(async (providerId: string, modelId: string) => {
    setBusy(true);
    const key = k(providerId, modelId);
    // Optimistic add — the server's PUT is idempotent so a
    // double-click (e.g. double-trigger from rapid state changes)
    // is safe to retry without un-adding.
    setSet((prev) => {
      if (prev.has(key)) return prev;
      const next = new Set(prev);
      next.add(key);
      return next;
    });
    try {
      await disableProviderModel(providerId, modelId);
    } catch (err) {
      // Don't roll back — the server's persisted state is the
      // truth. The catalog filter (handlers.rs list_models)
      // already excludes any disabled row regardless of what
      // the in-memory Set thinks. Next hydrate will resync.
      console.error('[DisabledModels] disable failed:', err);
    } finally {
      setBusy(false);
    }
  }, []);

  const enable = useCallback(async (providerId: string, modelId: string) => {
    setBusy(true);
    const key = k(providerId, modelId);
    setSet((prev) => {
      if (!prev.has(key)) return prev;
      const next = new Set(prev);
      next.delete(key);
      return next;
    });
    try {
      await enableProviderModel(providerId, modelId);
    } catch (err) {
      console.error('[DisabledModels] enable failed:', err);
    } finally {
      setBusy(false);
    }
  }, []);

  const value = useMemo<DisabledModelsContextValue>(
    () => ({
      isDisabled,
      disable,
      enable,
      count: set.size,
      busy,
    }),
    [isDisabled, disable, enable, set.size, busy],
  );

  return (
    <DisabledModelsContext.Provider value={value}>
      {children}
    </DisabledModelsContext.Provider>
  );
}

export function useDisabledModels(): DisabledModelsContextValue {
  const ctx = useContext(DisabledModelsContext);
  if (!ctx) {
    throw new Error(
      'useDisabledModels() must be used inside <DisabledModelsProvider>',
    );
  }
  return ctx;
}