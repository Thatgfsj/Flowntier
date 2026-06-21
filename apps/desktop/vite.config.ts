import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';

// Tauri 2 dev mode: the WebView is a real Tauri webview, so the real
// `@tauri-apps/api/*` packages work end-to-end (the IPC bridge is
// injected by Tauri at runtime). Aliasing them to no-op stubs breaks
// invoke() — it would return `null` and the app could not save secrets,
// start workflows, or do anything else.
//
// The previous "stub in dev" hack was a workaround for opening the
// Vite dev URL in a plain browser, where `window.__TAURI_INTERNALS__`
// is missing. We don't ship that workflow: use `pnpm tauri:dev` to
// launch the real WebView, and use `pnpm build && pnpm tauri:build`
// for production installers.
export default defineConfig({
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: '127.0.0.1',
    hmr: {
      protocol: 'ws',
      host: '127.0.0.1',
      port: 1421,
    },
    watch: {
      // Don't watch the Rust side; Tauri handles that.
      ignored: ['**/src-tauri/**'],
    },
  },
  envPrefix: ['VITE_', 'TAURI_'],
  build: {
    target: 'es2022',
    minify: 'esbuild',
    sourcemap: true,
  },
});
