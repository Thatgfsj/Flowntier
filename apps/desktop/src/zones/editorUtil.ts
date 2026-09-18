/**
 * Pure helpers for the v0.4.36 EditorPane, extracted so they can
 * be unit-tested without mounting CodeMirror.
 *
 * `pickLanguageSupport` maps a file extension onto the CodeMirror
 * language packs the desktop app actually bundles (see
 * apps/desktop/package.json — lang-rust / lang-javascript /
 * lang-json / lang-markdown / lang-python). Files with any other
 * (or no) extension get a plain-text editor: `null` means "no
 * language support", and the component simply omits the language
 * extension instead of crashing.
 *
 * Why a string union instead of importing @codemirror/* here:
 * keeping this module dependency-free means the vitest run stays
 * in the cheap `node` environment and the component owns all
 * CodeMirror wiring.
 */

/** Language kinds the editor can highlight, one per bundled pack. */
export type EditorLanguageKind = "rust" | "javascript" | "json" | "markdown" | "python";

/**
 * Map a file extension (without the leading dot, case-insensitive)
 * to a bundled CodeMirror language, or `null` when the extension is
 * unknown / empty.
 *
 * Mapping (v0.4.36 spec):
 *   rs              → rust
 *   js/jsx/ts/tsx   → javascript (tsx variants included via options)
 *   json            → json
 *   md/markdown     → markdown
 *   py              → python
 *   anything else   → null (no language extension)
 */
export function pickLanguageSupport(ext: string): EditorLanguageKind | null {
  const e = ext.toLowerCase();
  if (e === "rs") return "rust";
  if (e === "js" || e === "jsx" || e === "ts" || e === "tsx") return "javascript";
  if (e === "json") return "json";
  if (e === "md" || e === "markdown") return "markdown";
  if (e === "py") return "python";
  return null;
}

/**
 * Extract the extension (lowercased, no dot) from a file path.
 * Handles both `/` and `\` separators because the workspace tree
 * hands out server-built paths on Windows. Dotfiles like
 * `.gitignore` have no extension → `""`.
 */
export function extOf(path: string): string {
  const base = path.split(/[\\/]/).pop() ?? "";
  const dot = base.lastIndexOf(".");
  // No dot, leading dot (dotfile), or trailing dot → no extension.
  if (dot <= 0) return "";
  return base.slice(dot + 1).toLowerCase();
}
