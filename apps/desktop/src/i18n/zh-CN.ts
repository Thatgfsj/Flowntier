/**
 * Chinese (Simplified) — default language.
 *
 * v0.4 does NOT translate every existing component string; this
 * file only contains strings for new features added in the v0.4
 * release (ErrorBoundary, update banner, language toggle, etc.).
 * Migrating the existing TopBar / Settings / CommandDock strings
 * is a separate task — tracked in v0.5.
 *
 * The Chinese keys use the original hardcoded strings as values
 * so existing components can swap from literal "中文" → t('...')
 * one at a time without a flag-day.
 */
const zhCN = {
  // ── Language toggle ────────────────────────────────────
  'lang.label': '语言',
  'lang.zh-CN': '中文',
  'lang.en-US': 'English',

  // ── ErrorBoundary ──────────────────────────────────────
  'error.title': '出错了 v{{version}}',
  'error.subtitle': '应用遇到了一个未捕获的错误。下面的信息可以帮助排查问题。',
  'error.message': '错误消息',
  'error.componentStack': '组件堆栈',
  'error.action.copyLogs': '📋 复制日志',
  'error.action.restart': '🔄 重启应用',
  'error.action.report': '🐛 上报问题',
  'error.action.copySuccess': '日志已复制到剪贴板',
  'error.logLocation': '日志同时写入本地文件 {{path}}，可以随附在上报的问题里。',
  'error.copyFallback': '复制以下日志：',
  'error.reportFallback': '复制以下 URL 到浏览器打开：',

  // ── Update banner ──────────────────────────────────────
  'update.available': '⬆ 升级 v{{version}}',
  'update.tooltip': '点击下载并安装最新版本（应用会自动重启）',

  // ── Update install dialog ──────────────────────────────
  'update.confirmTitle': '更新可用',
  'update.confirmBody':
    'Flowntier {{version}} 已就绪可安装，应用将重启。\n\n是否继续？',
  'update.confirmInstall': '安装并重启',
  'update.confirmLater': '稍后',
  'update.failedTitle': '更新失败',
  'update.failedBody':
    '更新安装失败：{{error}}\n\n请从 GitHub Releases 手动下载安装包。',

  // ── TopBar ─────────────────────────────────────────────
  // v0.4.0-rc1 polish: migrated from hardcoded literals.
  'topbar.tagline': '· 智能体公司操作系统',
  'topbar.chat': 'Chat',
  'topbar.settings': '设置',

  // ── CommandDock ────────────────────────────────────────
  // v0.4.0-rc1 polish.
  'commandDock.placeholder': '向主理下达指令…  例如：实现 POST /auth/login 接口',
  'commandDock.submit': '提交',
  'commandDock.busy': '运行中…',
  'commandDock.empty': '在命令栏输入指令来开始。',

  // ── BottomConsole ──────────────────────────────────────
  'bottomConsole.tabs.log': '运行日志',
  'bottomConsole.tabs.events': '事件流',
  'bottomConsole.empty.log': '没有日志。',
  'bottomConsole.empty.events': '没有事件。',
  'bottomConsole.levels.error': '错误',
  'bottomConsole.levels.warn': '警告',
  'bottomConsole.levels.info': '信息',
  'bottomConsole.levels.debug': '调试',
  'bottomConsole.levels.trace': '追踪',

  // ── Settings (some new bits) ───────────────────────────
  'settings.language': '语言',
  'settings.sections.providers': 'AI 供应商',
  'settings.sections.secrets': 'API 密钥',
  'settings.sections.customProviders': '自定义路由站',
  'settings.sections.about': '关于',
  'settings.providers.addCustom': '添加自定义路由站',
  'settings.providers.noKey': '未配置',
  'settings.providers.configured': '已配置',
  'settings.providers.enabled': '已启用',
  'settings.providers.disabled': '已停用',
  'settings.providers.discoverModels': '拉取最新模型',
  'settings.secrets.addKey': '添加 API 密钥',
  'settings.secrets.placeholder': '粘贴 API 密钥 (sk-...)',
  'settings.secrets.never': '密钥不会离开本机。',
  'settings.headerSubtitle': '管理 LLM 供应商和角色模型',
  'settings.quickAdd.title': '添加 AI 供应商',
  'settings.customProvider.title': '添加自定义中转站',
  'settings.models.available': '可用模型',
  'settings.roles.title': '角色 → 模型 分配',

  // ── Drift banner (Phase 5) ───────────────────────────
  'drift.message':
    '⚠ Sidecar 运行时版本 (v{{sidecar}}) 低于 shell 期望 (v{{expected}}). 某些功能可能不可用。请重新构建 sidecar。',
  'drift.dismiss': '关闭',
};

export type Translations = typeof zhCN;
export default zhCN;