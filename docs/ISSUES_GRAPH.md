# ACO 问题图谱 (Issues Knowledge Graph)

> 使用 Graphiti 的思路组织：实体(Entity) + 关系(Relation) + 时间戳(Temporal)

## 已解决的问题 (Resolved)

### E1: WebView2Loader.dll 缺失
- **状态**: ✅ 已解决
- **时间**: 2026-06-20
- **根因**: Tauri resources 路径配置错误
- **修复**: 将 DLL 复制到 src-tauri 目录，使用相对路径
- **关系**: `caused_by` → Tauri 打包机制, `fixed_by` → tauri.conf.json resources 配置

### E2: REPAIRING 状态转换缺失
- **状态**: ✅ 已解决
- **时间**: 2026-06-20
- **根因**: state_machine.py 缺少从 REPAIRING 到 final_review_* 的转换
- **修复**: 添加 3 个转换 (pass/repair/reject)
- **关系**: `caused_by` → 状态机设计不完整, `fixed_by` → state_machine.py

### E3: TypeScript 严格模式错误 (24个)
- **状态**: ✅ 已解决
- **时间**: 2026-06-20
- **根因**: exactOptionalPropertyTypes + 缺失类型导出
- **修复**: 添加类型导出、修复可选属性类型
- **关系**: `caused_by` → TypeScript 严格配置, `fixed_by` → 多文件修改

### E4: Rust Clippy 警告
- **状态**: ✅ 已解决
- **时间**: 2026-06-20
- **根因**: result_large_err, non_snake_case, map_identity
- **修复**: Box 错误类型、重命名、移除冗余
- **关系**: `caused_by` → Clippy 规则, `fixed_by` → 3 个 crate 修改

### E5: Event bus 测试阻塞
- **状态**: ✅ 已解决
- **时间**: 2026-06-20
- **根因**: 在异步运行时中调用阻塞 recv()
- **修复**: 使用 spawn_blocking
- **关系**: `caused_by` → Tokio 运行时限制, `fixed_by` → event-bus 测试

### E6: PyInstaller 缺少 aco_runtime 模块
- **状态**: ✅ 已解决
- **时间**: 2026-06-20
- **根因**: pathex 配置错误，aco_runtime 包未打包
- **修复**: 修正 spec 文件路径
- **关系**: `caused_by` → PyInstaller 配置, `fixed_by` → aco_runtime.spec

### E7: PyInstaller 缺少 loguru 模块
- **状态**: ✅ 已解决
- **时间**: 2026-06-20
- **根因**: pip 缓存清理后依赖丢失
- **修复**: 重新安装 loguru
- **关系**: `caused_by` → 依赖管理, `fixed_by` → pip install

### E8: Worker 不真正写入文件
- **状态**: ✅ 已解决
- **时间**: 2026-06-21
- **根因**: Worker 只返回 LLM 响应，不执行操作
- **修复**: 创建 WorkerAgentV2 + FileOpsPlugin
- **关系**: `caused_by` → 架构设计(Phase 1 限制), `fixed_by` → worker_v2.py + file_ops.py

### E9: 事件循环冲突
- **状态**: ✅ 已解决
- **时间**: 2026-06-21
- **根因**: asyncio.get_event_loop().run_until_complete() 在已运行的循环中调用
- **修复**: 将 _execute_response 改为 async
- **关系**: `caused_by` → Python 异步机制, `fixed_by` → worker_v2.py

### E10: Sidecar 不自动启动
- **状态**: ✅ 已解决
- **时间**: 2026-06-21
- **根因**: BaseDirectory::Resource 不正确解析 sidecar 路径
- **修复**: 使用 app.shell().sidecar() 替代 Command::new()
- **关系**: `caused_by` → Tauri v2 API 变更, `fixed_by` → lib.rs

### E11: 端口冲突 (旧进程未清理)
- **状态**: ✅ 已解决
- **时间**: 2026-06-21
- **根因**: 旧 aco_runtime.exe 未退出，占用 7317 端口
- **修复**: 启动前 taskkill 清理旧进程
- **关系**: `caused_by` → 进程管理, `fixed_by` → lib.rs

### E12: CSP 阻止 fetch
- **状态**: ✅ 已解决
- **时间**: 2026-06-21
- **根因**: Tauri v2 CSP 即使列了 http://127.0.0.1:7317 仍阻止 fetch
- **修复**: 暂时禁用 CSP (csp: null)
- **关系**: `caused_by` → Tauri v2 安全策略, `fixed_by` → tauri.conf.json

### E13: React hooks 顺序错误
- **状态**: ✅ 已解决
- **时间**: 2026-06-21
- **根因**: useEffect 在 if (!ready) return 之后
- **修复**: 拆分为 App (gate) + MainApp
- **关系**: `caused_by` → React 规则, `fixed_by` → App.tsx 重构

## 待解决的问题 (Open)

### P1: CSP 安全性
- **状态**: 🔴 未解决
- **优先级**: 中
- **描述**: 当前禁用 CSP，需要重新配置安全策略
- **关系**: `blocks` → 生产发布, `related_to` → E12

### P2: Worker prompt 不稳定
- **状态**: 🟡 部分解决
- **优先级**: 高
- **描述**: LLM 不总是返回正确的 tool call 格式
- **关系**: `related_to` → E8, `blocks` → 真正的代码生成

### P3: TypeScript 24 个错误 (0.2.3 清理)
- **状态**: ✅ 已解决
- **优先级**: 低
- **描述**: simulator.ts 和 UI 组件的严格模式错误

### P4: bus.publish monkey-patch 泄漏
- **状态**: 🔴 未解决
- **优先级**: 低
- **描述**: 事件处理器在 workflow 间累积
- **关系**: `tracked_in` → 0.3

### P5: /api/settings/secrets/{name}/reveal 无认证
- **状态**: 🔴 未解决
- **优先级**: 低
- **描述**: loopback-only，需要 Origin 检查
- **关系**: `tracked_in` → 0.3

### P6: Health monitoring + TPM throttling
- **状态**: 🟡 部分解决
- **优先级**: 中
- **描述**: plan_scheduler.py 有注释但未实现
- **关系**: `related_to` → Phase 2.12

### P7: 真正的文件写入验证
- **状态**: 🟡 部分解决
- **优先级**: 高
- **描述**: Worker 调用 write_file 但 LLM 不总是生成正确的 tool call
- **关系**: `related_to` → P2, E8

## 实体关系图

```
┌─────────────────────────────────────────────────────────────┐
│                    ACO 问题图谱                              │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  [E12: CSP 阻止 fetch] ──blocks──→ [P1: CSP 安全性]        │
│         │                                                   │
│         └──related_to──→ [E10: Sidecar 不自动启动]          │
│                              │                              │
│                              └──caused_by──→ Tauri v2 API   │
│                                                             │
│  [E8: Worker 不写文件] ──related_to──→ [P2: prompt 不稳定]  │
│         │                              │                    │
│         └──caused_by──→ 架构设计       └──blocks──→ [P7]    │
│                                                             │
│  [E6: PyInstaller 缺模块] ──related_to──→ [E7: 缺 loguru]  │
│         │                                                   │
│         └──caused_by──→ 依赖管理                            │
│                                                             │
│  [E11: 端口冲突] ──related_to──→ [E10: Sidecar 启动]       │
│                                                             │
│  [E13: hooks 顺序] ──caused_by──→ React 规则                │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

## 时间线

```
2026-06-20  E1, E2, E3, E4, E5, E6, E7  ── 基础设施修复
2026-06-21  E8, E9, E10, E11, E12, E13   ── 功能修复
            P1, P2, P4, P5, P6, P7       ── 待解决
```

## Graphiti 集成建议

如果要使用 Graphiti 构建知识图谱：

1. **安装**: `pip install graphiti-core` + Neo4j
2. **实体**: Issue, Component, Fix, Commit
3. **关系**: caused_by, fixed_by, blocks, related_to, tracked_in
4. **时间**: 每个 issue 有创建时间和解决时间
5. **搜索**: "哪些问题导致了 CSP 错误？" → 图遍历找到 E12, E10

但考虑到项目当前阶段，建议先用这个 Markdown 图谱，等项目稳定后再集成 Graphiti。
