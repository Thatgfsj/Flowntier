import type { RuntimeSnapshot, ThinkingStrength } from "./types.js";

export const EMPTY: RuntimeSnapshot = { providers: [], roles: [], available_models: [] };
export const THINKING_OPTIONS: { value: ThinkingStrength; labelKey: string }[] = [
  { value: "", labelKey: "settings.models.thinking.default" },
  { value: "low", labelKey: "settings.models.thinking.low" },
  { value: "medium", labelKey: "settings.models.thinking.medium" },
  { value: "high", labelKey: "settings.models.thinking.high" },
];
