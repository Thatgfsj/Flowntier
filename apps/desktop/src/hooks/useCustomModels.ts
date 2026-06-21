/**
 * useCustomModels — persists user-curated model additions per provider.
 *
 * When a user "pulls latest models" from a provider's API, the live
 * catalog goes well beyond the hard-coded `KNOWN_MODELS` preset
 * snapshot (e.g. DeepSeek ships new reasoning models between ACO
 * releases). We persist the user's chosen set in localStorage so it
 * survives restarts and is merged into the model picker on every
 * `listRouterModels` refresh.
 *
 * Schema:
 *   { [providerId: string]: { [modelId: string]: ProviderModel } }
 */

import { useCallback, useEffect, useState } from 'react';
import type { ProviderModel } from '../lib/api.js';

const STORAGE_KEY = 'aco.custom_models.v1';

type CustomModelMap = Record<string, Record<string, ProviderModel>>;

function loadAll(): CustomModelMap {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return {};
    return JSON.parse(raw) as CustomModelMap;
  } catch {
    return {};
  }
}

function saveAll(m: CustomModelMap): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(m));
  } catch {
    // Ignore quota errors; the user just loses persistence for this session.
  }
}

export function useCustomModels() {
  const [map, setMap] = useState<CustomModelMap>(() => loadAll());

  useEffect(() => {
    saveAll(map);
  }, [map]);

  const getForProvider = useCallback(
    (providerId: string): ProviderModel[] => {
      const entry = map[providerId];
      return entry ? Object.values(entry) : [];
    },
    [map],
  );

  const addMany = useCallback((providerId: string, models: ProviderModel[]) => {
    setMap((prev) => {
      const next: CustomModelMap = { ...prev };
      const cur = { ...(next[providerId] ?? {}) };
      for (const m of models) {
        if (m.id) cur[m.id] = m;
      }
      next[providerId] = cur;
      return next;
    });
  }, []);

  const remove = useCallback((providerId: string, modelId: string) => {
    setMap((prev) => {
      const cur = { ...(prev[providerId] ?? {}) };
      delete cur[modelId];
      return { ...prev, [providerId]: cur };
    });
  }, []);

  const clear = useCallback((providerId: string) => {
    setMap((prev) => {
      const next = { ...prev };
      delete next[providerId];
      return next;
    });
  }, []);

  // Total count across all providers — used as a stable dep for effects
  // that need to react to changes without re-running on every render.
  const totalCount = Object.values(map).reduce(
    (acc, mm) => acc + Object.keys(mm).length,
    0,
  );

  return { map, getForProvider, addMany, remove, clear, totalCount };
}
