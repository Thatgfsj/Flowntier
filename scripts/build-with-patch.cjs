#!/usr/bin/env node
// build-with-patch.cjs — Two-phase Tauri build that patches
// installer.nsi AFTER Tauri generates it but BEFORE makensis
// compiles it.
//
// Why this exists:
// Tauri 2.x runs `build.beforeBundleCommand` BEFORE the bundler
// renders installer.nsi, so any patch to installer.nsi from that
// hook gets overwritten by Tauri writing the fresh template.
// The hook can verify and rewrite, but the rewritten file is
// overwritten too.
//
// Phase 1: `tauri build --no-bundle` — Tauri compiles binaries
//   and sidecars, generates installer.nsi template (no bundling).
// Phase 2: `node patch-nsis.cjs` — patches installer.nsi in place.
// Phase 3: invoke makensis directly on patched installer.nsi.
//   The output is renamed to the canonical Tauri bundle path.
//
// makensis is bundled with Tauri at:
//   C:/Users/<user>/AppData/Local/tauri/NSIS/makensis.exe
// Or fallback: NSIS installed at C:/Program Files/NSIS/.
//
// Usage:
//   node scripts/build-with-patch.cjs
//
// Or for explicit target:
//   TAURI_TARGET=x86_64-pc-windows-gnu node scripts/build-with-patch.cjs

const { spawnSync } = require('child_process');
const fs = require('fs');
const path = require('path');

const ROOT = path.resolve(__dirname, '..');
const TAURI_DIR = path.join(ROOT, 'apps', 'desktop');

const TAURI_TARGET = process.env.TAURI_TARGET || '';
const TARGET_SUBDIR = TAURI_TARGET
  ? `target/${TAURI_TARGET}/release/nsis/x64`
  : 'target/release/nsis/x64';

const INSTALLER_NSI = path.join(ROOT, TARGET_SUBDIR, 'installer.nsi');
const BUNDLE_OUT = path.join(
  ROOT,
  TAURI_TARGET ? `target/${TAURI_TARGET}/release` : 'target/release',
  'bundle/nsis/Flowntier_0.4.22_x64-setup.exe'
);

function step(name, cmd, args, opts = {}) {
  console.log(`\n=== ${name} ===`);
  console.log(`$ ${cmd} ${args.join(' ')}`);
  const r = spawnSync(cmd, args, { stdio: 'inherit', cwd: opts.cwd || ROOT, ...opts });
  if (r.status !== 0) {
    console.error(`Step failed: ${name} (exit ${r.status})`);
    process.exit(r.status || 1);
  }
}

function findMakensis() {
  const home = process.env.USERPROFILE || process.env.HOME || '';
  const candidates = [
    path.join(home, 'AppData/Local/tauri/NSIS/makensis.exe'),
    'C:/Program Files (x86)/NSIS/makensis.exe',
    'C:/Program Files/NSIS/makensis.exe',
  ];
  for (const c of candidates) {
    if (fs.existsSync(c)) return c;
  }
  return null;
}

// Phase 1: build everything except bundle.
const phase1Args = ['exec', 'tauri', 'build', '--no-bundle'];
if (TAURI_TARGET) phase1Args.push('--target', TAURI_TARGET);
step('Phase 1: tauri build --no-bundle', 'pnpm', phase1Args, { cwd: TAURI_DIR });

// Phase 2: patch installer.nsi (write v3 taskkill belt + sidecar check + node check).
step('Phase 2: patch-nsis.cjs', 'node', [path.join(__dirname, 'patch-nsis.cjs')]);

// Phase 3: invoke makensis on the patched installer.nsi.
const makensis = findMakensis();
if (!makensis) {
  console.error('Could not find makensis.exe. Install NSIS or run `pnpm tauri info`.');
  process.exit(1);
}

if (!fs.existsSync(INSTALLER_NSI)) {
  console.error(`installer.nsi not found at ${INSTALLER_NSI}`);
  process.exit(1);
}

const nsisDir = path.dirname(INSTALLER_NSI);
console.log(`\n=== Phase 3: makensis ${INSTALLER_NSI} ===`);
console.log(`$ "${makensis}" "${INSTALLER_NSI}" (cwd: ${nsisDir})`);
const r = spawnSync(makensis, [INSTALLER_NSI], { stdio: 'inherit', cwd: nsisDir });
if (r.status !== 0) {
  console.error(`makensis failed: ${r.status}`);
  process.exit(r.status);
}

const nsisOutput = path.join(nsisDir, 'nsis-output.exe');
if (!fs.existsSync(nsisOutput)) {
  console.error(`makensis did not produce nsis-output.exe`);
  process.exit(1);
}

fs.mkdirSync(path.dirname(BUNDLE_OUT), { recursive: true });
fs.copyFileSync(nsisOutput, BUNDLE_OUT);
console.log(`\nCopied to ${BUNDLE_OUT}`);

console.log('\n=== Done ===');
console.log(`Patched setup.exe: ${BUNDLE_OUT}`);