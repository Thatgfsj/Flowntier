/**
 * React hook for runtime connection status.
 */

import { useCallback, useEffect, useState } from 'react';
import { checkHealth } from './api.js';

export interface ConnectionStatus {
  connected: boolean;
  checking: boolean;
  retry: () => Promise<boolean>;
}

export function useConnection(interval = 5000): ConnectionStatus {
  const [connected, setConnected] = useState(false);
  const [checking, setChecking] = useState(false);

  const check = useCallback(async () => {
    setChecking(true);
    try {
      const ok = await checkHealth();
      setConnected(ok);
      return ok;
    } finally {
      setChecking(false);
    }
  }, []);

  const retry = useCallback(async () => {
    setChecking(true);
    try {
      // Try multiple times
      for (let i = 0; i < 5; i++) {
        if (await checkHealth()) { setConnected(true); return true; }
        await new Promise(r => setTimeout(r, 1000));
      }
      setConnected(false);
      return false;
    } finally {
      setChecking(false);
    }
  }, []);

  useEffect(() => {
    void check();
    const timer = setInterval(() => void check(), interval);
    return () => clearInterval(timer);
  }, [check, interval]);

  return { connected, checking, retry };
}
