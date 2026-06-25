/**
 * updater — wraps @tauri-apps/plugin-updater with sane defaults
 * and a "banner" shape that the TopBar can render.
 *
 * Behaviour:
 *   - checkForUpdate()  fires once on app start (non-blocking).
 *     If an update is found, it goes into the AppState updateBanner
 *     slot; the TopBar renders the banner; clicking it calls
 *     installUpdate() which downloads + applies + relaunches.
 *
 *   - Errors are logged but never thrown to the UI — a failed update
 *     check should not crash the app. The user can manually trigger
 *     a check via Settings → About → "Check for updates".
 *
 * Update endpoint comes from `tauri.conf.json` `bundle.updater.endpoints`,
 * which is wired in Phase 1.4 of the v0.4-delivery plan.
 */

import { check, type Update } from '@tauri-apps/plugin-updater';
import { ask, message } from '@tauri-apps/plugin-dialog';

export interface UpdateBanner {
  /** Update is available, version strictly greater than current. */
  available: boolean;
  /** New version string (e.g. "0.4.1"). */
  version?: string;
  /** Human-readable release notes (markdown → plaintext). */
  notes?: string;
  /** Estimated download size in bytes; undefined when unknown. */
  size?: number;
  /** True when the last check errored out. */
  error?: boolean;
}

export const NO_UPDATE: UpdateBanner = { available: false };

/**
 * One-shot update check. Returns a banner for the UI.
 * Never throws — always returns a banner.
 */
export async function checkForUpdate(): Promise<UpdateBanner> {
  try {
    const update = await check();
    if (!update) return NO_UPDATE;
    return {
      available: true,
      version: update.version,
      ...(update.body ? { notes: update.body } : {}),
      // `Update.size` is not part of the public API in v2;
      // size info is added when the plugin exposes it.
    };
  } catch (err) {
    // Most common: no network, GitHub rate-limited, signature
    // mismatch. Log and let the UI show "could not check".
    console.warn('[flowntier] update check failed:', err);
    return { available: false, error: true };
  }
}

/**
 * Download and install an update. Prompts the user to confirm
 * before downloading. On Windows/macOS the install requires a
 * relaunch; we ask via @tauri-apps/plugin-dialog first.
 */
export async function installUpdate(update: Update): Promise<void> {
  const yes = await ask(
    `Flowntier ${update.version} is ready to install. The app will restart.\n\nProceed?`,
    {
      title: 'Update available',
      kind: 'info',
      okLabel: 'Install and restart',
      cancelLabel: 'Later',
    },
  );
  if (!yes) return;

  try {
    // downloadAndInstall() handles download + verify + apply.
    // On macOS / Linux this triggers a relaunch automatically.
    await update.downloadAndInstall();
  } catch (err) {
    await message(
      `Update failed to install: ${String(err)}\n\nPlease download manually from GitHub Releases.`,
      { title: 'Update failed', kind: 'error' },
    );
    throw err;
  }
}