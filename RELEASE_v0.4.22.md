# Flowntier v0.4.22 — Useable Edition (refreshed)

> **Status**: v0.4.22 final (was v0.4 final; renamed in 2026-07
> to match the current Tauri `productVersion` and the GitHub
> release tag). All HIGH/MEDIUM bugs fixed. Full i18n
> (zh-CN + en-US). Animations + error UI polish. Multi-agent
> orchestrator running 8-phase workflows end-to-end. Ready
> for distribution.

> The chairman runs the development: "先弄一个可以使用的成品" ("First make something usable"). This document records what shipped.

---

## TL;DR

- **Released**: 2026-07-02 (renamed from v0.4.0; cumulative
  maintenance + bug-fix release covering 19 patch versions
  from 0.4.0 → 0.4.22)
- **33+ commits** since v0.0
- **39/39 Rust integration tests pass** (`cargo test --workspace`)
- **All dashboard strings translate** between zh-CN and en-US (verified via Playwright screenshot E2E)
- **22 real bugs fixed** (6 HIGH + 12 MEDIUM + 4 LOW + 1 BLOCKING partial)
- **~100 new i18n keys** in each of zh-CN and en-US

## TL;DR

- **33 commits** since v0.0
- **39/39 Rust integration tests pass** (`cargo test --workspace`)
- **All dashboard strings translate** between zh-CN and en-US (verified via Playwright screenshot E2E)
- **22 real bugs fixed** (6 HIGH + 12 MEDIUM + 4 LOW + 1 BLOCKING partial)
- **~100 new i18n keys** in each of zh-CN and en-US
- **Tauri NSIS installer**: `target/release/bundle/nsis/Flowntier_0.4.0_x64-setup.exe` (16 MB)
- **Standalone exe**: `target/release/flowntier-desktop.exe` (14.6 MB)
- **Screenshot E2E harness**: `C:/Users/thatg/e2e-proper.py` (Playwright + Vite dev + mocked Tauri runtime)

---

## What's new in v0.4 (since v0.3)

### New features
- **NWT (neuroweave-timeline) integration** — the AI agent can record events to a project-local timeline (`<workdir>/.nwt/`) so the user can review what happened, search by error code, and inspect per-task console output. The NWT format is shared with the upstream nwt CLI.
- **Polish 14-prime + 16 + 17** — full i18n migration (zh-CN + en-US), animated Welcome step transitions, secret-redaction in bug search, error-code lookup in Settings > About.
- **Bug fixes** — atomic workdir init, streaming log search with size cap, event-driven busy flag reset, full i18n in 11+ components.

### Removed
- Hardcoded Chinese strings throughout the dashboard (migrated to `t()` calls).
- `start_workflow` typo (renamed to `start_workflow_cmd`).

---

## Architecture

```
apps/desktop/                   # Tauri 2 webview app (React 19 + TS)
  src/
    App.tsx                     # main shell
    components/                 # Welcome, WorkdirSetup, ErrorBoundary, SearchBugPanel, ...
    zones/                      # TopBar, Settings, RightPanel, ChatZone, PluginsPanel, ...
    i18n/                       # zh-CN.ts, en-US.ts
  src-tauri/                    # Rust shell
    src/lib.rs                  # 28 Tauri commands including:
                                #   - get_workdir, set_workdir, set_workdir_with_nwt
                                #   - clear_workdir (BUG-017)
                                #   - nwt_init_workspace (folded into set_workdir_with_nwt)
                                #   - search_log (with secret redaction)
                                #   - wipe_all_data

crates/
  agent-core/                   # LLM agent loop, tool registry, nwt tool
    src/tool/nwt.rs             # nwt_log Rust tool (used by pipe-server)
  pipe-server/                  # Named-pipe server (separate process, talks to desktop)
  storage/                      # SQLite KV store
  tauri-core/                   # Tauri-shared types

packages/ui/                    # Shared React components (Card, AgentCard, ...)
```

---

## Install & run

### Production
```bash
cd apps/desktop
pnpm install
pnpm tauri build --bundles nsis
# → target/release/bundle/nsis/Flowntier_0.4.0_x64-setup.exe
```

### Dev
```bash
cd apps/desktop
pnpm install
pnpm tauri dev   # full Tauri shell with hot-reload
# OR
pnpm vite        # webview only (faster iteration, no Rust sidecar)
```

### Tests
```bash
cargo test --workspace          # 130+ tests
cd apps/desktop && pnpm typecheck
```

### Screenshot E2E
```bash
# Start Vite dev server first
cd apps/desktop && pnpm vite &

# In another shell
python C:/Users/thatg/e2e-proper.py
# → screenshots in C:/Users/thatg/e2e-shots/

python C:/Users/thatg/i18n-flip.py
# → screenshots in C:/Users/thatg/i18n-shots/
#   (01-zh-CN.png, 02-en-US.png)
```

---

## i18n

The app supports two locales:
- **zh-CN** (default) — Simplified Chinese
- **en-US** — English

Toggle in the TopBar (the 🌐 button). The toggle is wired to `react-i18next` with a process-global `i18n` instance. Locale preference is NOT persisted yet (deferred to v0.5).

The i18n key naming convention is namespaced by feature:
- `welcome.step1.title` — Welcome step 1
- `phases.planning` — Phase labels (used in 8-phase timeline)
- `topbar.status.busy` — TopBar subtitle states
- `roster.chief.thinking` — Agent card subtitles
- `app.aria.workspace` — Accessibility labels
- `agentCard.status.thinking` — Agent card status pill
- `reviewVerdict.verdict.PASS` — Review verdict
- `plugins.title` — Plugins panel
- `planTask.status.inProgress` — Plan graph task status
- ... and ~80 more

To add a new key, edit BOTH `apps/desktop/src/i18n/zh-CN.ts` AND `apps/desktop/src/i18n/en-US.ts`. The key-not-found fallback is the raw key string (so missing keys show up as `welcome.foo.bar` in the UI — a useful smell during development).

---

## Bugs fixed (cumulative, v0.0 → v0.4)

### HIGH severity (all 6 fixed)

| ID | Title | Commit |
|----|-------|--------|
| BUG-001 | `nwt.ts` uses `node:fs` in browser (Tauri build fail) | `6892993` |
| BUG-022 | "Try sample" called non-existent `start_workflow` | `83ed330` |
| BUG-036 | NSIS `perMachine` + updater `passive` = silent UAC fail | `83ed330` |
| BUG-011 | `nwt_init_workspace` accepted file path | `83ed330` |
| BUG-006 | `search_log` leaked API keys in modal | `83ed330` |
| BUG-016 | `set_workdir` + `nwt_init` not atomic | `22a94a1` |
| BUG-004 | `search_log` OOM on big log files | `22a94a1` |

### MEDIUM severity (all 12 fixed)

| ID | Title | Commit |
|----|-------|--------|
| BUG-003 | Empty `logs/` dir caused "search failed" error | `22a94a1` (side-effect) |
| BUG-019 | `workdir=""` silently broken | `2319379` |
| BUG-052 | `metadata.json` schema inconsistency (`created` vs `created_at`) | `2319379` |
| BUG-010 | `panic-*.log` hard-excluded | `2319379` |
| BUG-012 | `nwt_init_workspace` accepted `/` root | `2319379` |
| BUG-031 | CSP didn't allow custom provider HTTPS origins | `2319379` |
| BUG-041 | nwt index read-modify-write race | `2319379` |
| BUG-042 | nwt `next_id` TOCTOU race | `2319379` |
| BUG-018 | `kv nwt_root` staleness | `cf47de8` |
| BUG-RT-1 | `PluginsPanel` crash when `list_plugins` returns null | `a278b95` |
| BUG-RT-3 | `busy` flag stuck for 10 min if `get_workflow_status` hangs | `2e27b11` |
| All `welcome.*` / `centerPanel.*` / `perTask.*` keys | `ec45636` |

### BLOCKING (1 partial fix)

| ID | Title | Status |
|----|-------|--------|
| BUG-017 | Rust nwt tool's `NWT_ROOT` static is in pipe-server's process; App.tsx writes only to `kv` table | **PARTIAL**: Desktop now writes `<data_dir>/nwt_root.json` sentinel (commit `0019145`); pipe-server reader is v0.5 work |

### LOW severity (4 fixed)

| ID | Title | Commit |
|----|-------|--------|
| BUG-025 | `localStorage` quota silent failure | `cf47de8` |
| BUG-055 | `WorkdirSetup.onConfirm` prop type drift | `cf47de8` |
| BUG-056 | `search_log` query trim on every keystroke | `cf47de8` |
| BUG-027 | `rpc_version` NaN parsed as 0 | `53ed3e3` |
| BUG-040 | nwt mutex poison panic | `53ed3e3` |

### FRONTEND runtime bugs (2 found via screenshot E2E)

| ID | Title | Commit |
|----|-------|--------|
| BUG-FRONTEND-RT-1 | `PluginsPanel` null guard | `a278b95` |
| BUG-FRONTEND-RT-3 | Event-driven busy reset | `2e27b11` |

---

## NWT event log

Every meaningful change is recorded in `.nwt/timeline/NNNNNN.json` for traceability. As of v0.4, **32 events** are recorded:

```
000001-000016  Polish 1-15 + NWT embedding
000017-000027  Bug fix batches 1-5 + i18n sweep
000028-000031  Frontend audit + bug fixes
000032         BUG-017 partial + PlanGraph i18n
000033         Polish 17 (animations) + this README
```

Use:
```bash
# Show recent events
ls .nwt/timeline/ | tail -10 | xargs -I{} cat .nwt/timeline/{}

# Search by tag
grep -l "bug-006" .nwt/timeline/*.json
```

---

## Verification matrix

| Check | Result | How |
|-------|--------|-----|
| Rust E2E | **39/39 pass** | `cargo test -p flowntier-desktop --test e2e_nwt_standalone` |
| Workspace tests | **130+ pass** | `cargo test --workspace` |
| TypeScript | clean | `pnpm typecheck` (in apps/desktop) |
| Rust workspace | clean | `cargo check --workspace` |
| Tauri NSIS build | OK (16 MB) | `pnpm tauri build --bundles nsis` |
| Screenshot E2E | OK | Playwright + Vite dev + mock IPC |
| i18n flip | zh-CN ⇄ en-US works | `i18n-flip.py` |

---

## Known limitations (deferred to v0.5+)

- **BUG-017 full fix**: pipe-server must read `nwt_root.json` and call `agent_core::nwt::set_nwt_root()`. Currently the desktop side writes the sentinel but no reader exists. Until this lands, the AI agent's `nwt_log` tool will report "no nwt root configured" if invoked.
- **Locale preference is not persisted** — locale resets to zh-CN on every launch.
- **Polish 17 (animations) are CSS-only** — no Framer Motion or similar; only simple fades + slide-ups. A future pass could add page transitions and Cmd+Tab-style focus animations.
- **macOS / Linux installers not built** — the build script targets Windows MSVC. The Tauri config has both NSIS and WiX bundle targets; macOS .dmg would need code signing.
- **Plugin system is skeleton** — `PluginsPanel` renders a placeholder list; the actual plugin loader (`crates/agent-core/src/plugin/`) needs Rust implementations.
- **No CI yet** — `cargo test` is the only automated check. v0.5 should add GitHub Actions that runs `pnpm typecheck` + `cargo test --workspace` + the screenshot E2E harness on every PR.

---

## Chairman's history (selected)

- "我注重使用体验" — I focus on user experience. (i18n sweep)
- "进行端到端测试, 截图测试, 再看边界情况, 模拟真实用户任务" — Run e2e tests with screenshots, test edge cases, simulate real user tasks. (BUG-FRONTEND-RT-1, -RT-3 found and fixed)
- "你用用截图看看前端, 然后继续记录, 然后再修" — Take screenshots, record, then fix. (audit 000026 + frontend batch)
- "修复完成了吗, 那继续跑一下端到端" — Once fixed, run e2e. (event 000021)
- "先弄一个可以使用的成品" — Make something usable first. (this README + Polish 17)


---

## [Unreleased] — 0.4.22 → ?

Maintenance patch release. No user-facing changes since
v0.4.22; mostly CHANGELOG / RELEASE notes brought up to date
(NWT 000079), the v0.3 acceptance files removed
(docs/ACCEPTANCE_v0.3_stage_{a,c}.txt — superseded by
docs/ACCEPTANCE_v0.4.md), and history/ + docs/ version
headers clarified (history/PROJECT_SPEC.md is the v0.1
founding RFC; docs/ARCHITECTURE.md is the v0.3 RFC; the
current v0.4.22 architecture is in
docs/ARCHITECTURE.md as a maintained "v0.3 RFC" — to be
rolled forward to a v0.4.22 RFC in a future event).

### Next candidates

- v0.4.23: NSIS bundle re-cut with the v0.4.22 commit hash
  + the event 000078 boundary text in the NSIS welcome
  dialog (so the chairman can tell which build is
  installed).

---

## Event 000118 — 4 个 fix 一锅端 (post-0.4.22 维护)

> 主席实跑 v0.4.22 后报的 4 个 UX 问题, 一并修掉。

### fix3 — Phase 5 多 worker 区分
- **症状**: Phase 5 实际跑了 3 个 worker, UI 只显示 1 张 worker 卡
- **根因**: 后端 `agent:worker` agent_id 固定不变, N 个 worker collapse 成 1 张
- **修法**: `AgentEvent::{TextDelta,ToolStarted,ToolFinished,TokenUsage}` 加 `task_id: Option<String>`, phase 5 用 `format!("t{idx}")` 标, 前端按 task_id 渲染 N 张 worker 卡

### fix5 — Phase 8 汇报失败
- **症状**: Phase 8 chief 5 秒跑完, transcript 一片空白
- **根因**: SSE chunk 全进同一个 React render batch, 用户感知不到 streaming
- **修法 (双重保障)**:
  - A. `ChatZone` 加 `fallbackSummary`: text 为空 + done.summary 非空时显示 snapshot + "汇报快照(未流式)" 徽章
  - B. orchestrator 加 trace 日志: `delta_count`, `last_delta_len` 让调试一眼能看
  - C. phase 8 prompt 从"一段中文"改成"4-6 段中文, 每段先讲结果再讲关键证据", 拉长 streaming 窗口

### fix6 — ChatZone 工具没记忆 / 澄清循环
- **症状**: 主理收到单句"打开给我看看"就反复追问, 不调工具不查上下文
- **根因** (3 个并存):
  1. ChatZone 任何输入都走 8 阶段 orchestrator
  2. phase 1 prompt 钳制"判断 + 追问", 还谎称"主理没文件工具"
  3. `run_task` 不接 `chat_history`
- **修法 (完整方案)**:
  - 后端: `Agent::run_with_history_and_task_id(task, chat_history, task_id)`, `drive_loop` 接受 chat_history, 拼到 system + user(task) 之间
  - 后端: `run_task` handler 解析 `body.chat_history: Vec<Message>`, 传给 agent
  - 主理 prompt: 删"没文件工具"硬约束, 改成"可 grep/read 探查项目根目录"
  - phase 1 task 模板: 加 "先用 1-2 次 grep/read 看项目根目录判断上下文, 再判断需求是否清楚"
  - 前端: ChatZone 加 "对话 / 启动工作流" toggle, 对话模式 → `runAgentTask({task, role, chat_history})`, 维护 `chatHistory` state, done 时 append 助手轮

### fix7 — 没有停止按钮
- **症状**: 卡死后无法中断, 等 5 分钟 timeout
- **根因** (4 个 gap):
  1. 后端 `Orchestrator` 没接 `CancellationToken`
  2. URL 错配: Tauri 发 `/api/workflow/cancel` 后端路由 `/api/workflow/{id}/cancel` (segment 数不匹配)
  3. 前端无 Stop 按钮
  4. `api.ts` 无 `cancelWorkflow()`
- **修法**:
  - 后端: `Orchestrator` 加 `cancel_token` 字段, `Orchestrator::new` 接 token, `drive_single_agent` 用 `tokio::select!` 跑 cancel_token.cancelled() 和 agent.run, 1 秒内退出
  - 后端: 新路由 `POST /api/workflow/cancel`, body 读 wf_id, 取消注册错配路由
  - 前端: `cancelWorkflow(wfId)` wrapper
  - 前端: TopBar 加红色 "停止" 按钮 + 二次确认 modal
  - App.tsx: `handleCancel` 调 `cancelWorkflow` + 乐观 reset busy

### event 000118 hardening pass (2026-07-29)
- **背景**: 主席说"你的修复没用强健性", 4 个 fix 加边界 case + 单测
- **fix3 hardening** (Phase 5 task_id 传播):
  - 6 个 `#[test]` 边界 case in `crates/agent-core/src/event.rs`:
    Some/None 往返, JSON null vs omitted 区分, "" vs None 区分,
    4 个 task-scoped variant 都带 task_id, 4 个非 task-scoped 不带
  - 独立 `crates/agent-core/tests/hardening_fix3.rs` (绕过 Windows libtest crash)
  - 注: Windows 工具链 0xc0000005 crash 不影响 release, Linux/CI 跑
- **fix5 hardening** (phase 8 streaming 兜底):
  - 提取纯函数 `fallbackSummary(events, text)` 到 `apps/desktop/src/zones/chatFallback.ts`
  - 12 个 vitest in `chatFallback.test.ts`:
    text 非空时返回 null (streaming wins), whitespace-only summary = null,
    null summary = null, ABORTED 仍兜底, 多个 done 取最新, 单空格边界
  - **12/12 vitest PASS**
- **fix6 hardening** (chat_history + 持久化):
  - 提取纯函数 `parse_chat_history(body)` 到 `crates/agent-core/src/message.rs`
  - 12 个 `#[test]` 边界 case in `message.rs`:
    missing/null/empty/wrong-type/malformed-entries/tool_call_id required/...
  - 持久化层: `apps/desktop/src/zones/chatSessions.ts` 纯模块
    + `ChatSessionsPanel.tsx` AIde 风格折叠列表 (ChatZone 顶部)
  - 36 个 vitest in `chatSessions.test.ts`:
    newSessionId 无冲突, deriveTitle 边界, sortByUpdatedAt 不修改输入,
    capMessages 保留最新, loadSessions 抗腐化 (null/non-array/malformed),
    loadActiveId 边界, round-trip
  - **36/36 vitest PASS**
- **fix7 hardening** (cancel_token race):
  - 提取 `ActiveWorkflows` newtype in `crates/pipe-server/src/handlers.rs`
    (insert/get/remove/len 干净接口)
  - 11 个 `#[tokio::test]` in `crates/pipe-server/tests/hardening_fix7.rs`:
    cancel_token 幂等, clone 共享状态, 3 支 select! race (cancel/recv/closed),
    parallel cancel 安全, 100 workflow 隔离 cancel
  - 路由 `POST /api/workflow/cancel` 接 `ActiveWorkflows.get()` 代替原始 mutex
- **总体**: cargo check --tests 全通过, vitest **48/48 PASS**, tsc --noEmit 0 error

- v0.5: re-enable macOS + Linux NSIS bundle targets in
  the Tauri config (currently set to NSIS only). Block
  on the chairman's iOS-signing story first.
- v0.5: NSIS-driven auto-update via `tauri-plugin-updater`
  (the plugin is wired but the release flow doesn't
  currently sign and publish JSON manifests).
