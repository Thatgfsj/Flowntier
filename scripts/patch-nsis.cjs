// BUG-FRONTEND-RT-15 (event 000045): post-build script that
// patches the Tauri-generated NSIS installer to also check
// (and kill) the sidecar binary (flowntier_runtime.exe)
// before the file copy. Tauri 2.x only checks the main
// flowntier.exe — but the sidecar also locks the file.
//
// Usage: node src-tauri/binaries/patch-nsis.js
// (called from tauri.conf.json postBuildCommand)

// Tauri 2.x places the generated installer.nsi at the
// WORKSPACE root (target/release/nsis/x64/), not under the
// app folder. So we resolve from the script's parent (the
// binaries/ dir) up to the workspace root.
const fs = require('fs');
const path = require('path');
const SCRIPT_DIR = __dirname;
// scripts/ is at the workspace root, so just go up one level
const WORKSPACE = path.resolve(SCRIPT_DIR, '..');

const TARGETS = [
  path.join(WORKSPACE, 'target/release/nsis/x64/installer.nsi'),
  path.join(WORKSPACE, 'target/release/bundle/nsis/x64/installer.nsi'),
];

const SIDECAR_NAME = 'flowntier_runtime.exe';
const PRODUCT_NAME = 'Flowntier sidecar';

let patched = 0;
for (const rel of TARGETS) {
  const fullPath = path.resolve(__dirname, '..', '..', '..', rel);
  if (!fs.existsSync(fullPath)) continue;
  let content = fs.readFileSync(fullPath, 'utf8');
  if (content.includes('BUG-FRONTEND-RT-15 marker')) {
    console.log(`  (already patched) ${fullPath}`);
    continue;
  }
  const marker =
    '!insertmacro CheckIfAppIsRunning "${MAINBINARYNAME}.exe" "${PRODUCTNAME}"';
  const idx = content.indexOf(marker);
  if (idx < 0) {
    console.warn(`  marker not found in ${fullPath}`);
    continue;
  }
  // Insert a second CheckIfAppIsRunning for the sidecar, right
  // after the first call (and before "Copy main executable").
  const injection =
    `!insertmacro CheckIfAppIsRunning "${marker.replace('$MAINBINARYNAME.exe', SIDECAR_NAME)}" "${PRODUCT_NAME}"\n` +
    '  ; BUG-FRONTEND-RT-15 marker';
  // Easier: just splice in the sidecar check after the main
  // CheckIfAppIsRunning line.
  const lines = content.split('\n');
  const out = [];
  for (let i = 0; i < lines.length; i++) {
    out.push(lines[i]);
    if (lines[i].includes('!insertmacro CheckIfAppIsRunning "${MAINBINARYNAME}.exe"')) {
      out.push(
        '',
        `  ; BUG-FRONTEND-RT-15 (event 000045): also kill the`,
        `  ; sidecar binary if it's still running. Tauri only checks`,
        `  ; the main app; the sidecar (flowntier_runtime.exe) keeps`,
        `  ; a file handle on the binary that blocks the overwrite.`,
        `  !insertmacro CheckIfAppIsRunning "${SIDECAR_NAME}" "${PRODUCT_NAME}"`,
        '  ; end BUG-FRONTEND-RT-15 marker'
      );
    }
  }
  const next = out.join('\n');
  fs.writeFileSync(fullPath, next, 'utf8');
  console.log(`  patched: ${fullPath}`);
  patched++;
}
console.log(patched > 0
  ? `Done — patched ${patched} NSIS source file(s). Rebuild to embed.`
  : 'No NSIS source files were patched.');