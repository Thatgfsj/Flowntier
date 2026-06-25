/**
 * English (United States) — secondary language.
 *
 * v0.4 ships a complete translation only for the strings added
 * with the v0.4 release. The legacy TopBar / Settings / CommandDock
 * text is intentionally not translated yet; the language toggle
 * primarily exists so non-Chinese users can at least see the
 * update banner, error screen, and installer errors in English.
 *
 * When adding new translatable strings, please add them to BOTH
 * this file and zh-CN.ts so the toggle never falls back to the
 * raw key.
 */
import type { Translations } from './zh-CN';

const enUS: Translations = {
  // ── Language toggle ────────────────────────────────────
  'lang.label': 'Language',
  'lang.zh-CN': '中文',
  'lang.en-US': 'English',

  // ── ErrorBoundary ──────────────────────────────────────
  'error.title': 'Something went wrong v{{version}}',
  'error.subtitle':
    'The app hit an uncaught error. The information below can help diagnose the issue.',
  'error.message': 'Error message',
  'error.componentStack': 'Component stack',
  'error.action.copyLogs': '📋 Copy logs',
  'error.action.restart': '🔄 Restart app',
  'error.action.report': '🐛 Report issue',
  'error.action.copySuccess': 'Logs copied to clipboard',
  'error.logLocation':
    'Logs are also written to {{path}} — please attach them in your report.',
  'error.copyFallback': 'Copy the following log:',
  'error.reportFallback': 'Copy this URL to your browser:',

  // ── Update banner ──────────────────────────────────────
  'update.available': '⬆ Upgrade to v{{version}}',
  'update.tooltip': 'Click to download and install (app will restart)',

  // ── Update install dialog ──────────────────────────────
  'update.confirmTitle': 'Update available',
  'update.confirmBody':
    'Flowntier {{version}} is ready to install. The app will restart.\n\nProceed?',
  'update.confirmInstall': 'Install and restart',
  'update.confirmLater': 'Later',
  'update.failedTitle': 'Update failed',
  'update.failedBody':
    'The update failed to install: {{error}}\n\nPlease download manually from GitHub Releases.',

  // ── TopBar (v0.4.0-rc1 polish) ─────────────────────────
  'topbar.tagline': '· Visual AI Software Company',
  'topbar.chat': 'Chat',
  'topbar.settings': 'Settings',

  // ── CommandDock ────────────────────────────────────────
  'commandDock.placeholder': 'Send a command to the Chief...  e.g. implement POST /auth/login',
  'commandDock.submit': 'Submit',
  'commandDock.busy': 'Running...',
  'commandDock.empty': 'Type a command in the bar to start.',

  // ── BottomConsole ──────────────────────────────────────
  'bottomConsole.tabs.log': 'Run log',
  'bottomConsole.tabs.events': 'Event stream',
  'bottomConsole.empty.log': 'No log entries yet.',
  'bottomConsole.empty.events': 'No events yet.',
  'bottomConsole.levels.error': 'ERROR',
  'bottomConsole.levels.warn': 'WARN',
  'bottomConsole.levels.info': 'INFO',
  'bottomConsole.levels.debug': 'DEBUG',
  'bottomConsole.levels.trace': 'TRACE',

  // ── Settings (some new bits) ───────────────────────────
  'settings.language': 'Language',
  'settings.sections.providers': 'AI Providers',
  'settings.sections.secrets': 'API keys',
  'settings.sections.customProviders': 'Custom relay stations',
  'settings.sections.about': 'About',
  'settings.providers.addCustom': 'Add custom relay station',
  'settings.providers.noKey': 'Not configured',
  'settings.providers.configured': 'Configured',
  'settings.providers.enabled': 'Enabled',
  'settings.providers.disabled': 'Disabled',
  'settings.providers.discoverModels': 'Discover models',
  'settings.secrets.addKey': 'Add API key',
  'settings.secrets.placeholder': 'Paste API key (sk-...)',
  'settings.secrets.never': 'Keys never leave this machine.',
  'settings.headerSubtitle': 'Manage LLM providers and per-role models',
  'settings.quickAdd.title': 'Add AI provider',
  'settings.customProvider.title': 'Add custom relay station',
  'settings.models.available': 'Available models',
  'settings.roles.title': 'Role -> model assignment',

  // ── Drift banner (Phase 5) ───────────────────────────
  'drift.message':
    '⚠ Sidecar runtime version (v{{sidecar}}) is older than the shell expects (v{{expected}}). Some features may be unavailable. Please rebuild the sidecar.',
  'drift.dismiss': 'Dismiss',
};

export default enUS;