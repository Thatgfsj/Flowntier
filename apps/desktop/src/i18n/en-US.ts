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

  // ── Buttons / actions ──────────────────────────
  'settings.action.cancel': 'Cancel',
  'settings.action.close': 'Close',
  'settings.action.save': 'Saving...',
  'settings.action.savedAt': 'Saved · {{time}}',
  'settings.action.delete': 'Delete',
  'settings.action.deleteCustom': 'Delete relay station',
  'settings.action.remove': 'Remove',
  'settings.action.moveUp': 'Move up',
  'settings.action.moveDown': 'Move down',
  'settings.action.addFallback': '+ Add fallback',
  'settings.action.confirmYes': '✓ Yes',
  'settings.action.confirmNo': '✗ No (set <{{targetModel}}> as primary)',

  // ── Confirmation dialogs ───────────────────────
  'settings.confirm.removeModel.title': 'Delete model {{name}}?',
  'settings.confirm.removeModel.body':
    'This model will be removed from {{provider}}\u2019s fallback chain. Continue?',
  'settings.confirm.deleteCustom.title': 'Delete relay station {{name}}?',
  'settings.confirm.deleteCustom.body':
    'This relay station and its API key will be deleted. Continue?',

  // ── Quick add AI (presets) ──────────────────────
  'settings.quickAdd.openai.compatible': 'OpenAI SDK compatible',
  'settings.quickAdd.anthropic.compatible': 'Anthropic SDK compatible',
  'settings.quickAdd.kind.openai': 'OpenAI SDK compatible',
  'settings.quickAdd.kind.anthropic': 'Anthropic SDK compatible',
  'settings.quickAdd.saved': '✓ Saved and active',
  'settings.quickAdd.added': '✓ Added successfully',
  'settings.quickAdd.addedShort': '✓ added',
  'settings.quickAdd.keyPlaceholder': 'sk-...',
  'settings.quickAdd.modelCount': '{{count}} models · {{keyEnv}}',
  'settings.quickAdd.errorMissingName': 'Please enter a display name',
  'settings.quickAdd.errorMissingKey': 'Please enter an API key',
  'settings.quickAdd.errorInvalidKey': 'API key has an invalid format (letters, digits, underscore only)',

  // ── Custom provider form ────────────────────────
  'settings.custom.nameLabel': 'Display name',
  'settings.custom.namePlaceholder': 'My relay station',
  'settings.custom.idLabel': 'ID (used in URL)',
  'settings.custom.idPlaceholder': 'my_relay',
  'settings.custom.baseUrlLabel': 'Base URL',
  'settings.custom.baseUrlPlaceholder': 'https://your-relay.com/v1',
  'settings.custom.apiKeyLabel': 'API key',
  'settings.custom.apiKeyPlaceholder': 'sk-...',
  'settings.custom.kindLabel': 'Protocol',
  'settings.custom.kind.openai': 'OpenAI SDK compatible',
  'settings.custom.kind.anthropic': 'Anthropic SDK compatible',

  // ── Models tab ─────────────────────────────────
  'settings.models.default': 'Default model',
  'settings.models.local': 'local',
  'settings.models.list': 'Model list',
  'settings.models.noModels': 'This provider has no models to fetch.',
  'settings.models.emptyFallback':
    '(No fallback yet; if the primary model fails, the role will return an error.)',
  'settings.models.newModelId': 'model-id',
  'settings.models.newModelName': 'Display name (optional)',

  // ── CompatHints (per-provider 兼容接口 hint) ─────────
  'settings.compat.title': 'Compatible interfaces (pick one)',
  'settings.compat.restartHint': 'Set these environment variables (or add <MINIMAX_API_KEY> in the panel below and the runtime will inject os.environ), then restart the runtime.',

  // ── Model manager dialog ─────────────────────
  'settings.models.fallbackChain': 'Fallback chain ({{count}})',
  'settings.models.pullError': 'Pull failed',
  'settings.models.callingApi': 'Calling {{provider}} API...',
  'settings.models.foundCount': 'Found {{count}} models. Tick to add; already-added models are marked with a checkmark.',
  'settings.models.emptyOption': '(No models available — first add an AI provider above)',
  'settings.models.emptyCustomModels': 'No custom models yet. Click "Discover models" to pull the available list from the {{provider}} official API, then tick the ones you want to add.',
  'settings.models.clearAll': 'Clear all custom models',
  'settings.roles.chief': 'Chief',
  'settings.roles.worker': 'Worker',
  'settings.roles.reporter': 'Reporter',
  'settings.quickAdd.errorSaveFailed': 'Save failed',
  'settings.quickAdd.errorNoModels': 'Please add at least one model',
  'settings.providers.siliconflow.desc': 'SiliconFlow',
  'settings.about.title': 'About Flowntier',
  'settings.about.version': 'Version: v{{version}}{{build}}',
  'settings.about.clearedNotice': 'Local data cleared. Next launch will return to the first-run wizard.',
  'settings.about.clearData': 'Clear local data',
  'settings.about.clearDataConfirmTitle': 'Clear all local data?',
  'settings.about.clearDataConfirmBody': 'This deletes everything in %APPDATA%\flowntier\: API keys, custom providers, run logs, error logs. Cannot be undone.',
  'settings.about.clearDataConfirmYes': '✓ Yes, clear everything',
  'settings.about.clearDataConfirmNo': '✗ No',
  'settings.about.clearDataError': 'Clear failed: {{error}}',
  // ── Error fallbacks (Polish 12) ──────────────────
  'settings.error.saveFailed': 'Save failed',
  'settings.error.alreadyExists': 'Model {{id}} already exists',
  'settings.error.invalidId': 'ID may only contain lowercase letters, digits, and underscore',
  'settings.error.missingApiKey': 'Please enter an API key',
  'settings.error.invalidBaseUrl': 'Base URL must start with http:// or https://',
  'settings.error.deleteCustomFailed': 'Failed to delete relay station',
  'settings.error.deleteRole': 'Failed to delete role',
  'settings.error.tErr': 'Error: {{error}}',
  // ── Polish 13: remaining strings ──────────────────
  'settings.providers.addAI': 'Add AI provider',
  'settings.roles.criticA': 'Critic A',
  'settings.roles.criticB': 'Critic B',
  'settings.field.keyConfigured': 'Key configured',
  'settings.models.customModels': 'Custom models ({{count}})',
  'settings.models.addSelected': 'Add {{count}}',
  'settings.models.alreadyAdded': '✓ added',
  'settings.models.all': 'Select all',
  'settings.models.none': 'Clear selection',
  'settings.models.selectedCount': '{{count}} selected',
  'settings.models.pullTitle': 'Pull {{provider}} models',
  'settings.models.modelExists': 'Model {{id}} already exists',
  'settings.error.customAdd': 'Add custom relay station',
  'settings.error.alreadyAdded': 'Already added',
  'workdir.title': 'Set work directory',
  'workdir.subtitleFirst': 'Pick a local directory as the project root before the AI starts working. Each new project will be auto-created as a sub-directory here.',
  'workdir.subtitleSettings': 'Change the work directory. New projects will be auto-created as sub-directories here.',
  'workdir.placeholder': '/Users/me/projects',
  'workdir.browse': 'Browse',
  'workdir.hint': 'Tip: can be an empty directory. Sub-directories are auto-created per task.',
  'workdir.skip': 'Set later',
  'workdir.confirmFirst': 'Confirm and start',
  'workdir.confirmSettings': 'Save',
  'workdir.errorEmpty': 'Please type a path or click Browse',
  'workdir.errorPick': 'Failed to pick directory: {{error}}',
  'workdir.pickTitle': 'Pick work directory',
  // ── Drift banner (Phase 5) ───────────────────────────
  'drift.message':
    '⚠ Sidecar runtime version (v{{sidecar}}) is older than the shell expects (v{{expected}}). Some features may be unavailable. Please rebuild the sidecar.',
  'drift.dismiss': 'Dismiss',
};

export default enUS;