# Flowntier 主理 / ChiefApp (v0.1.0)

Android 客户端,跟 `IChingOracle` 同款墨纸配色 + Compose + Material3。

## 它干什么

`ChiefApp` 是 **Flowntier 桌面的远程控制器**。手机连上桌面 runtime,输入任务,看到 8 阶段工作流的实时进度 + chief 最终交付总结。

```
┌────────────────────────────────┐
│  主理                          │  ← app 标题(同 iching-oracle 字体)
│  Flowntier 任务调度员         │
├────────────────────────────────┤
│  host:port [192.168.1.10:8765] │  ← 桌面 runtime 的 LAN 地址
│  任务 [..................]      │  ← 输入
│  [ 派发 ]                     │
├────────────────────────────────┤
│  PhaseTimeline                 │
│  ● 1-需求                      │  ← 跑过的
│  ● 2-规划                      │
│  ◉ 3-计划审核   ← active       │  ← 当前
│  ○ 4-派发                      │  ← 待跑
│  ○ 5-开发                      │
│  ○ 6-终审                      │
│  ○ 7-修复                      │
│  ○ 8-交付                      │
├────────────────────────────────┤
│  📜 当前进度                   │
│  任务 3/4                      │
│  critic a/b 正在评审 plan...   │
└────────────────────────────────┘
```

## 怎么用

### 1. 桌面端启动 runtime(并允许 LAN 访问)

```bash
# 桌面 cmd:
set FLOWNTIER_HTTP_BRIDGE=0.0.0.0:8765
O:\Flowntier\flowntier_runtime.exe
```

确认 runtime 起来后,在同局域网的手机上能访问 `http://<桌面 IP>:8765/health` 返回 `{"ok":true}`。

### 2. 手机装 APK

`ChiefApp-v0.1.0-debug.apk` 走 Android 8.0+(API 26)+。

首次装需要:
- 在手机的"设置 → 应用 → 特殊权限 → 安装未知应用"里允许本次安装源
- 装好后桌面出现 "Flowntier 主理" 图标

### 3. 启动 app

打开 app,在 `host:port` 字段里填入桌面 runtime 的 LAN 地址(例:`192.168.1.10:8765`)。在 `任务` 字段里写需求,点 **派发**。

app 会:
1. `POST /api/run_workflow` — 0.155s 拿到 `wf_id`
2. 每 2s 轮询 `GET /api/workflow/{wf_id}/status`
3. PhaseTimeline 8 个点逐个亮起
4. chief 写完最终交付后,`summary` 字段显示在底栏

## 跟 iching-oracle 的关系

| | iching-oracle | chief-app |
|---|---|---|
| 平台 | Android | Android |
| 技术栈 | Kotlin + Compose + Material3 | Kotlin + Compose + Material3 |
| 主题 | 墨纸 teal | 墨纸 teal(同一套) |
| launcher 图标 | 渐变方块 | 同心圆(象征 8 阶段) |
| 包名 | `com.thatgfsj.iching` | `com.thatgfsj.chief` |
| 模式 | 单屏 + 抽卦 | 单屏 + 派任务 |

风格一致,Flowntier 全家桶的视觉统一。

## 状态(2026-07-03)

- ✅ Gradle build 通过(`./gradlew :app:assembleDebug`)
- ✅ APK 13.7MB,valid Android package
- ❌ **本机没装 Android emulator,无法现场启动验证**
- ⏳ 等主席装到真机 / 模拟器后告诉我,有问题再修
