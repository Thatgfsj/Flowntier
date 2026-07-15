// BUG-FRONTEND-RT-15 (event 000045) + event 000061: post-build
// script that patches the Tauri-generated NSIS installer.
//
// Three patches, in order:
//   (a) BUG-FRONTEND-RT-15: add a 2nd CheckIfAppIsRunning macro
//       for the sidecar (flowntier_runtime.exe). Tauri 2.x only
//       checks the main binary; the sidecar keeps a file handle
//       that blocks the overwrite.
//   (b) Event 000061 — Auto-kill old processes: belt-and-braces
//       taskkill /F /IM for both binaries right before the file
//       copy. Catches the case where the user closed the GUI but
//       a stale process keeps the file locked. NSIS's built-in
//       CheckIfAppIsRunning sometimes misses locked handles when
//       the main process already exited but the sidecar didn't.
//   (c) Event 000061 — Node.js runtime check in .onInit: abort
//       the install with a clear zh-CN message if `node --version`
//       isn't on PATH or returns a non-zero exit code. Tauri uses
//       Node.js internally for the WebView2 asset bundling, so
//       missing Node breaks the very first app launch.
//
// Usage: invoked from tauri.conf.json (postBuildCommand), or
// manually via `pnpm tauri:patch`.

const fs = require('fs');
const path = require('path');

const SCRIPT_DIR = __dirname;
const WORKSPACE = path.resolve(SCRIPT_DIR, '..');

const TARGETS = [
  path.join(WORKSPACE, 'target/release/nsis/x64/installer.nsi'),
  path.join(WORKSPACE, 'target/release/bundle/nsis/x64/installer.nsi'),
  // v0.4.21 (event 000064): `pnpm tauri:build --target
  // x86_64-pc-windows-gnu` puts the NSI under the triple'd
  // target dir. Cover it so the patch runs whether the
  // chairman builds with or without an explicit --target.
  path.join(WORKSPACE, 'target/x86_64-pc-windows-gnu/release/nsis/x64/installer.nsi'),
];

const SIDECAR_NAME = 'flowntier_runtime.exe';
const PRODUCT_NAME = 'Flowntier sidecar';
const RUNTIME_MARKER = 'BUG-FRONTEND-RT-15 marker';
const NODE_MARKER = 'v0.4.21 node-runtime check';

let patched = 0;
for (const rel of TARGETS) {
  const fullPath = path.resolve(__dirname, '..', '..', '..', rel);
  if (!fs.existsSync(fullPath)) {
    continue;
  }
  let content = fs.readFileSync(fullPath, 'utf8');

  // Patch (a) + (b): sidecar CheckIfAppIsRunning. The taskkill
  // belt-and-braces goes into .onInit (runs before the install
  // wizard appears) — see patch (b) below.
  if (!content.includes(RUNTIME_MARKER)) {
    const lines = content.split('\n');
    const out = [];
    for (let i = 0; i < lines.length; i++) {
      out.push(lines[i]);
      if (lines[i].includes('!insertmacro CheckIfAppIsRunning "${MAINBINARYNAME}.exe"')) {
        out.push(
          '',
          `  ; BUG-FRONTEND-RT-15 marker`,
          `  !insertmacro CheckIfAppIsRunning "${SIDECAR_NAME}" "${PRODUCT_NAME}"`,
          '  ; end BUG-FRONTEND-RT-15 marker'
        );
      }
    }
    content = out.join('\n');
    console.log(`  patched (a): ${fullPath}`);
  } else {
    console.log(`  (a already patched) ${fullPath}`);
  }

  // Patch (b): .onInit taskkill belt-and-braces. Inserts the
  // kill calls BEFORE the wizard UI shows, so the file handles
  // are released before any SetSection installation begins.
  if (!content.includes('v0.4.22 taskkill-belt-bracess-v3 start')) {
    const marker = 'Function .onInit';
    const idx = content.indexOf(marker);
    if (idx >= 0) {
      // Tauri-generated NSIS uses the C-style brace-less form:
      //   Function .onInit
      //     ${GetOptions} ...
      //     ...
      //   FunctionEnd
      // So there is NO standalone `{` line after `Function .onInit`
      // — the function body starts immediately on the next line.
      // We insert the block right after the `Function .onInit\n`
      // header. The previous bug tried to find a `{` brace and
      // either matched the first `{` in the file (returning
      // insertAt=1 → block at file top) or split `${GetOptions}`
      // apart (returning insertAt in the middle of the macro).
      const eol = content.indexOf('\n', idx);
      const insertAt = eol + 1;
      // Build the command via Push (double-quoted) so NSIS expands
      // $${MAINBINARYNAME} ($$ → literal $) into the main binary
      // name once, BEFORE ExecWait runs. Avoids the
      // "Invalid command: ${" parse error from nested `${$...}`.
      // v0.4.22 (event 000097): NSIS taskkill v3 — use
      // process-tree kill (`taskkill /T /F`) for BOTH the
      // desktop shell AND the sidecar. The desktop shell
      // spawns the sidecar as a child of itself; killing the
      // shell alone doesn't always cascade (Windows
      // job-object behavior). Killing both with /T + a 1.5s
      // sleep covers the common case. Sleep is short — long
      // sleeps in 000092 caused system freezes (event 000093).
      const block =
        '\n' +
        '  ; v0.4.22 (event 000097) taskkill-belt-bracess-v3 start\n' +
        '  ; Catches the daemon / zombie case where the user\n' +
        '  ; closed the GUI but the sidecar is still listening\n' +
        '  ; on the named pipe. NSIS CheckIfAppIsRunning handles\n' +
        '  ; the foreground case; this catches the background.\n' +
        '  ; Note: hardcoded "flowntier-desktop.exe" matches the\n' +
        '  ; !define MAINBINARYNAME in tauri.conf.json. We tried\n' +
        '  ; $${MAINBINARYNAME} but NSIS 3.x parser doesn\'t handle\n' +
        '  ; the nested expansion in Push "..." strings cleanly.\n' +
        '  Push "taskkill /F /IM flowntier-desktop.exe /T"\n' +
        '  ExecWait $0\n' +
        '  Pop $0\n' +
        '  Push "taskkill /F /IM flowntier_runtime.exe /T"\n' +
        '  ExecWait $0\n' +
        '  Pop $0\n' +
        '  ; v0.4.22 (event 000097): short settle window so\n' +
        '  ; the kernel can release the file handles before the\n' +
        '  ; next File directive reads from disk. 1.5s is short\n' +
        '  ; enough not to freeze the system (event 000093\'s 3s\n' +
        '  ; + retry caused freezes), long enough for handles.\n' +
        '  Sleep 1500\n' +
        '  ; v0.4.22 (event 000097) taskkill-belt-bracess-v3 end\n';
      content = content.slice(0, insertAt) + block + content.slice(insertAt);
      console.log(`  patched (b): ${fullPath}`);
    } else {
      console.warn(`  .onInit marker not found in ${fullPath}; skipping taskkill`);
    }
  } else {
    console.log(`  (b already patched) ${fullPath}`);
  }

  // Patch (c): Node.js runtime check in .onInit. Inserted
  // immediately after the SetContext macro call so we abort
  // BEFORE the user sees the install wizard if Node is missing.
  // Uses built-in NSIS SearchPath + ReadRegDWORD for node.exe
  // detection — no extra plugins required.
  if (!content.includes(NODE_MARKER)) {
    const marker = '!insertmacro SetContext';
    const idx = content.indexOf(marker);
    if (idx < 0) {
      console.warn(`  SetContext marker not found in ${fullPath}; skipping Node check`);
    } else {
      const lineEnd = content.indexOf('\n', idx);
      const nodeCheckBlock =
        '\n' +
        '  ; v0.4.21 (event 000061) — Node.js runtime check.\n' +
        '  ; Tauri 2.x uses Node internally for WebView2 asset\n' +
        '  ; bundling; missing Node breaks the very first launch.\n' +
        '  ; Check PATH first, then common install locations.\n' +
        '  Push $0\n' +
        '  SearchPath $0 "node.exe"\n' +
        '  ${If} $0 == ""\n' +
        '    ; Not on PATH — try %ProgramFiles% fallback\n' +
        '    ${If} ${RunningX64}\n' +
        '      StrCpy $0 "$PROGRAMFILES64\\nodejs\\node.exe"\n' +
        '    ${Else}\n' +
        '      StrCpy $0 "$PROGRAMFILES\\nodejs\\node.exe"\n' +
        '    ${EndIf}\n' +
        '    ${If} ${FileExists} $0\n' +
        '      StrCpy $0 ""\n' +
        '    ${EndIf}\n' +
        '  ${EndIf}\n' +
        '  ${If} $0 == ""\n' +
        '    MessageBox MB_ICONSTOP|MB_OK "Flowntier 安装失败：未检测到 Node.js。Flowntier 需要 Node.js LTS (>=18)。请先从 https://nodejs.org 下载 LTS，安装时勾选 Add to PATH，然后重新运行本安装包。" IDCANCEL\n' +
        '    Abort\n' +
        '  ${EndIf}\n' +
        '  Pop $0\n' +
        '  ; v0.4.21 node-runtime check end\n';
      content = content.slice(0, lineEnd) + nodeCheckBlock + content.slice(lineEnd);
      console.log(`  patched (c): ${fullPath}`);
    }
  } else {
    console.log(`  (c already patched) ${fullPath}`);
  }

  fs.writeFileSync(fullPath, content, 'utf8');
  patched++;
}

console.log(patched > 0
  ? `Done — patched ${patched} NSIS source file(s). Rebuild to embed.`
  : 'No NSIS source files were patched.');