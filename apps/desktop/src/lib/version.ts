/**
 * Build-time version info. Inlined by Vite via define config
 * (see apps/desktop/vite.config.ts); defaults here are only
 * used during dev.
 *
 * Keep the keys exactly matching those declared in vite.config.ts
 * `define`. Changing them in one place without the other results
 * in 'undefined is not a string' at runtime.
 */

declare const __FLOWNTIER_VERSION__: string;
declare const __FLOWNTIER_BUILD_SHA__: string;

export const appVersion: string =
  typeof __FLOWNTIER_VERSION__ === "string" && __FLOWNTIER_VERSION__.length > 0
    ? __FLOWNTIER_VERSION__
    : "0.4.0-dev";

export const buildSha: string =
  typeof __FLOWNTIER_BUILD_SHA__ === "string" && __FLOWNTIER_BUILD_SHA__.length > 0
    ? __FLOWNTIER_BUILD_SHA__
    : "local";
