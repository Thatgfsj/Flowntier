// apps/desktop/src/hooks/useDisabledModels.ts
//
// v0.4.35 (event 000135): state moved to DisabledModelsContext
// so the in-memory Set is shared across all consumers (Settings
// mounts one row per provider; previously each row had its own
// empty Set, so a model hidden in a prior session appeared
// "undeleted" on a fresh mount).
//
// This file is now a compatibility shim — it re-exports the
// hook from the new location so existing imports keep working
// without churn:
//
//   import { useDisabledModels } from '../hooks/useDisabledModels';
//
// continues to resolve. New code should import directly from
// `../contexts/DisabledModelsContext.js` if it doesn't need the
// shim path.
export { useDisabledModels } from "../contexts/DisabledModelsContext.js";
