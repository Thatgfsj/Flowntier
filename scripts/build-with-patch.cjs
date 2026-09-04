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
// v0.4.33 (audit 000131): read VERSION from package.json
// instead of hardcoding — every previous build since v0.4.27
// stamped this string with the old version and silently
// shadowed the actual productVersion on disk.
const PKG_VERSION = (() => {
  try {
    const pkg = JSON.parse(fs.readFileSync(
      path.join(ROOT, 'apps/desktop/package.json'), 'utf8'));
    return pkg.version || '0.0.0';
  } catch { return '0.0.0'; }
})();
const BUNDLE_OUT = path.join(
  ROOT,
  TAURI_TARGET ? `target/${TAURI_TARGET}/release` : 'target/release',
  `bundle/nsis/Flowntier_${PKG_VERSION}_x64-setup.exe`
);

function step(name, cmd, args, opts = {}) {
  console.log(`\n=== ${name} ===`);
  console.log(`$ ${cmd} ${args.join(' ')}`);
  const r = spawnSync(cmd, args, { stdio: 'inherit', cwd: opts.cwd || ROOT, shell: true, ...opts });
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

// Phase 0 (v0.4.33 — audit 000131, root fix): re-copy the
// freshly compiled `flowntier-runtime.exe` from target/release
// into `apps/desktop/src-tauri/binaries/` with the expected
// `flowntier_runtime-<target-triple>.exe` naming. Without
// this, the sidecar that the installer packages stays
// pinned to whatever was last manually placed in `binaries/`
// — every `cargo build -p pipe-server` afterwards is invisible
// to end users, and runtime-only fixes (dispatcher wildcard,
// etc.) never ship.
//
// Tauri 2 has an internal sidecar auto-copy path, but it
// compares by content hash and silently skips the copy when
// it thinks the artifact is current. Empirically that path
// has been a no-op in this repo since at least v0.4.27, so
// we do it explicitly here to guarantee a fresh sidecar.
function copySidecar() {
  const targetTriple = TAURI_TARGET || (() => {
    // Read rustc's host triple as a best-effort default.
    try {
      const r = spawnSync('rustc', ['-vV'], { encoding: 'utf8' });
      const m = r.stdout && r.stdout.match(/host:\s*(\S+)/);
      if (m) return m[1];
    } catch {}
    return 'x86_64-pc-windows-msvc';
  })();

  const src = TAURI_TARGET
    ? path.join(ROOT, `target/${TAURI_TARGET}/release/flowntier-runtime.exe`)
    : path.join(ROOT, 'target/release/flowntier-runtime.exe');
  const dstDir = path.join(ROOT, 'apps/desktop/src-tauri/binaries');
  const dst = path.join(dstDir, `flowntier_runtime-${targetTriple}.exe`);

  if (!fs.existsSync(src)) {
    console.error(`Sidecar source not found: ${src}\nRun \`cargo build --release -p pipe-server\` first.`);
    process.exit(1);
  }
  fs.mkdirSync(dstDir, { recursive: true });
  fs.copyFileSync(src, dst);
  console.log(`\n=== Phase 0: sidecar copied ===\n  ${src}\n  → ${dst}\n`);
}
copySidecar();

// Phase 1: build everything except bundle.
const pnpmCmd = (() => {
  try {
    const t = spawnSync('pnpm', ['--version'], { shell: true });
    if (t.status === 0) return 'pnpm';
  } catch {}
  return 'npx -y pnpm';
})();
const phase1Args = ['exec', 'tauri', 'build', '--no-bundle'];
if (TAURI_TARGET) phase1Args.push('--target', TAURI_TARGET);
step('Phase 1: tauri build --no-bundle', pnpmCmd, phase1Args, { cwd: TAURI_DIR });

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