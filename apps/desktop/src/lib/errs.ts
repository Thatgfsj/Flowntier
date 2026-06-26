/**
 * Localized error-message helper.
 *
 * `tErr(t, error, fallbackKey)` renders a localized error message
 * from a thrown value. The fallback template `settings.error.tErr`
 * wraps the raw `error.message` in a `{{error}}` placeholder;
 * if the i18n key is missing, i18next returns the raw key
 * (which is fine for development).
 *
 * Use this anywhere a `try`/`catch` block has a hardcoded Chinese
 * (or English) fallback string. Pass an i18n key that
 * best describes the failure mode, e.g.:
 *
 *   try { ... } catch (e) {
 *     setError(tErr(t, e, 'settings.error.saveFailed'));
 *   }
 *
 * If the i18n key is missing, the user sees the raw key. Add the
 * key to both zh-CN.ts and en-US.ts.
 */
import type { TFunction } from 'i18next';

export function tErr(
  t: TFunction,
  error: unknown,
  fallbackKey: string,
): string {
  const raw = error instanceof Error ? error.message : String(error);
  return t(fallbackKey, { error: raw });
}
