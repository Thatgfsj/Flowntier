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

  // ── Buttons / actions ──────────────────────────
  'settings.action.cancel': '取消',
  'settings.action.close': '关闭',
  'settings.action.save': '保存中…',
  'settings.action.savedAt': '已保存 · {{time}}',
  'settings.action.delete': '删除',
  'settings.action.deleteCustom': '删除中转站',
  'settings.action.remove': '移除',
  'settings.action.moveUp': '上移',
  'settings.action.moveDown': '下移',
  'settings.action.addFallback': '+ 添加回退',
  'settings.action.confirmYes': '✓ 是',
  'settings.action.confirmNo': '✗ 否（设置 <{{targetModel}}> 为主模型）',

  // ── Confirmation dialogs ───────────────────────
  'settings.confirm.removeModel.title': '删除模型 {{name}}?',
  'settings.confirm.removeModel.body':
    '这个模型会从 {{provider}} 的回退链里移除。确定吗?',
  'settings.confirm.deleteCustom.title': '删除中转站 {{name}}?',
  'settings.confirm.deleteCustom.body':
    '该中转站和它的 API key 会被删除。确定吗?',

  // ── Quick add AI (presets) ──────────────────────
  'settings.quickAdd.openai.compatible': 'OpenAI SDK 兼容',
  'settings.quickAdd.anthropic.compatible': 'Anthropic SDK 兼容',
  'settings.quickAdd.kind.openai': 'AI SDK 兼容',
  'settings.quickAdd.kind.anthropic': 'Anthropic SDK 兼容',
  'settings.quickAdd.saved': '✓ 已保存并激活',
  'settings.quickAdd.added': '✓ 添加成功',
  'settings.quickAdd.addedShort': '✓已添加',
  'settings.quickAdd.keyPlaceholder': 'sk-...',
  'settings.quickAdd.modelCount': '{{count}} 个模型 · {{keyEnv}}',
  'settings.quickAdd.errorMissingName': '请填写显示名称',
  'settings.quickAdd.errorMissingKey': '请填写 API 密钥',
  'settings.quickAdd.errorInvalidKey': 'API 密钥格式无效（英文+数字+下划线）',

  // ── Custom provider form ────────────────────────
  'settings.custom.nameLabel': '显示名称',
  'settings.custom.namePlaceholder': '我的中转站',
  'settings.custom.idLabel': 'ID (URL 用)',
  'settings.custom.idPlaceholder': 'my_relay',
  'settings.custom.baseUrlLabel': 'Base URL',
  'settings.custom.baseUrlPlaceholder': 'https://your-relay.com/v1',
  'settings.custom.apiKeyLabel': 'API 密钥',
  'settings.custom.apiKeyPlaceholder': 'sk-...',
  'settings.custom.kindLabel': '协议',
  'settings.custom.kind.openai': 'OpenAI SDK 兼容',
  'settings.custom.kind.anthropic': 'Anthropic SDK 兼容',

  // ── Models tab ─────────────────────────────────
  'settings.models.default': '默认模型',
  'settings.models.local': '本地',
  'settings.models.list': '模型列表',
  'settings.models.noModels': '该 provider 暂无可拉取的模型。',
  'settings.models.emptyFallback':
    '（暂无回退；主模型失败时该角色会直接报错）',
  'settings.models.newModelId': 'model-id',
  'settings.models.newModelName': '显示名(可选)',

  // ── CompatHints (per-provider 兼容接口 hint) ─────────
  'settings.compat.title': '兼容接口（任选其一）',
  'settings.compat.restartHint': '把以上命令放进系统环境变量（或者在下方添加 <MINIMAX_API_KEY> 后由 runtime 自动注入 os.environ），然后重启 runtime。',

  // ── Model manager dialog ─────────────────────
  'settings.models.fallbackChain': '回退链（{{count}}）',
  'settings.models.pullError': '拉取失败',
  'settings.models.callingApi': '正在调用 {{provider}} API...',
  'settings.models.foundCount': '共 {{count}} 个模型。勾选要加入的，已添加的会标记 ✓。',
  'settings.models.emptyOption': '(无可用模型 — 先在上方「添加 AI 供应商」里填 key)',
  'settings.models.emptyCustomModels': '暂无自选模型。点击「拉取最新模型」从 {{provider}} 官方 API 拉取可用列表，勾选要加入的即可。',
  'settings.models.clearAll': '清除全部自选模型',
  'settings.roles.chief': '主理',
  'settings.roles.worker': '实施',
  'settings.roles.reporter': '汇报',
  'settings.quickAdd.errorSaveFailed': '保存失败',
  'settings.quickAdd.errorNoModels': '请至少添加一个模型',
  'settings.providers.siliconflow.desc': 'SiliconFlow (硅基流动)',
  'settings.about.title': '关于 Flowntier',
  'settings.about.version': '版本：v{{version}}{{build}}',
  'settings.about.clearedNotice': '已清除本地数据。下次启动会回到首次运行向导。',
  'settings.about.clearData': '清除本地数据',
  'settings.about.clearDataConfirmTitle': '清除所有本地数据？',
  'settings.about.clearDataConfirmBody': '此操作会删除 %APPDATA%\flowntier\ 下的所有文件：API 密钥、自定义供应商、运行日志、错误日志。无法撤销。',
  'settings.about.clearDataConfirmYes': '✓ 是，清除所有数据',
  'settings.about.clearDataConfirmNo': '✗ 否',
  'settings.about.clearDataError': '清除失败：{{error}}',
  // ── Error fallbacks (Polish 12) ──────────────────
  'settings.error.saveFailed': '保存失败',
  'settings.error.alreadyExists': '模型 {{id}} 已存在',
  'settings.error.invalidId': 'ID 只能包含小写字母、数字和下划线',
  'settings.error.missingApiKey': '请填写 API Key',
  'settings.error.invalidBaseUrl': 'Base URL 必须以 http:// 或 https:// 开头',
  'settings.error.deleteCustomFailed': '删除中转站失败',
  'settings.error.deleteRole': '删除角色失败',
  'settings.error.tErr': '错误：{{error}}',
  // ── Polish 13: remaining strings ──────────────────
  'settings.providers.addAI': '添加 AI 供应商',
  'settings.roles.criticA': '审核员 A',
  'settings.roles.criticB': '审核员 B',
  'settings.field.keyConfigured': 'Key 已配置',
  'settings.models.customModels': '自选模型（{{count}}）',
  'settings.models.addSelected': '添加 {{count}} 个',
  'settings.models.alreadyAdded': '✓已添加',
  'settings.models.all': '全选',
  'settings.models.none': '清空选择',
  'settings.models.selectedCount': '已选 {{count}} 个',
  'settings.models.pullTitle': '拉取 {{provider}} 模型',
  'settings.models.modelExists': '模型 {{id}} 已存在',
  'settings.error.customAdd': '添加自定义中转站',
  'settings.error.alreadyAdded': '已添加',
  'workdir.title': '设置工作目录',
  'workdir.subtitleFirst': '在 AI 开始工作前，请选一个本地目录作为项目根目录。每个新项目会作为子目录自动创建。',
  'workdir.subtitleSettings': '更换工作目录。新建的项目会作为子目录自动创建在这里。',
  'workdir.placeholder': '/Users/me/projects',
  'workdir.browse': '浏览',
  'workdir.hint': '提示：可以是空目录。子目录会按任务自动创建。',
  'workdir.skip': '稍后设置',
  'workdir.confirmFirst': '确认并开始',
  'workdir.confirmSettings': '保存',
  'workdir.errorEmpty': '请输入路径或点击浏览',
  'workdir.errorPick': '选择目录失败：{{error}}',
  'workdir.pickTitle': '选择工作目录',
  // ── Drift banner (Phase 5) ───────────────────────────
  'drift.message':
    '⚠ Sidecar 运行时版本 (v{{sidecar}}) 低于 shell 期望 (v{{expected}}). 某些功能可能不可用。请重新构建 sidecar。',
  'drift.dismiss': '关闭',
};

export type Translations = typeof zhCN;
export default zhCN;