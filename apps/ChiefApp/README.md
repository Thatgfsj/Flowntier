# ChiefApp — Flowntier Tarot (v0.1.0, event 000074)

Android 客户端 — 视觉用 iching-oracle 的语言(Kotlin + Compose + Material3 + 墨纸 teal),**功能是 Flwntier 的**:抽塔罗牌,数据走 Flwntier runtime。

## 它干什么

- 首页:78 张牌名飘动字云 + 居中"塔罗"标题 + 副标"Flowntier · 心诚则灵" + **"点击抽取"** 主按钮 + 次按钮 **"三卡阵 · 过去/现在/未来"**
- 抽卡:1.5s 翻牌动画(scale-in + Y 轴旋转 0°→360°),正位或逆位(50/50)
- 单卡:牌面 + 中文名 + 拼音 + 英文 + 正逆位 + 一句解读文案
- 三卡阵:三张牌横排,每张标"过去/现在/未来"
- 抽完可"再抽一张"或"三卡阵"或"返回首页"

**与 iching-oracle 区别**:iching-oracle 是 64 卦本地数据(assets/hexagrams.json),**完全离线**。ChiefApp 调 Flwntier runtime 的 `/api/tarot/draw` — runtime 端有 78 张牌的 deck(rust tarot module,内嵌 SVG 符号 + 中文解读文案)。

## 怎么用

### 1. 桌面端启动 runtime

```bash
# 桌面 cmd:
set FLOWNTIER_HTTP_BRIDGE=0.0.0.0:8765
O:\Flowntier\flowntier_runtime.exe
```

确认 `curl http://<桌面 IP>:8765/health` 返回 `{"ok":true}`。

### 2. 装 APK 到手机

`ChiefApp-tarot-v0.1.0-debug.apk` 装 Android 8.0+(API 26)+,首次装要允许"未知来源"。

桌面出现"Flowntier 塔罗"图标(墨纸 teal 底,白色塔罗牌轮廓 + 五角星)。

### 3. 启动 app

打开 app,顶部 `runtime` 输入框填桌面 LAN IP(例 `192.168.1.10:8765`),按 ping 验证连接。点 **点击抽取** 或 **三卡阵**。

## 数据路径

```
Android ChiefApp
    │
    │  OkHttp JSON-RPC 2.0
    │  POST /rpc  {"method":"GET","path":"/api/tarot/draw?spread=3"}
    ▼
Flwntier runtime (Rust)
    │
    │  tarot::draw_one() / draw_three_card_spread()
    │  Card { id, arcana, suit, rank, name_zh, name_pinyin, name_en,
    │         symbol_svg (inline 100x140 SVG), upright_meaning,
    │         reversed_meaning }
    ▼
78-card Rider-Waite deck
```

## 跟 iching-oracle 的关系

| | iching-oracle | chief-app |
|---|---|---|
| 平台 | Android | Android |
| 技术栈 | Kotlin + Compose + Material3 | 同 |
| 主题 | 墨纸 teal | 同 |
| 牌 | 64 卦(本地 assets/) | **78 张塔罗(runtime 端)** |
| 数据路径 | 离线 | 走 Flwntier runtime |
| 解读 | i_ching 静态 JSON | runtime 端 tarot::TarotCard 字段 |
| 抽卡动画 | 6 爻从下往上 | 整张牌 Y 轴翻转 |
| 包名 | `com.thatgfsj.iching` | `com.thatgfsj.chief` |

风格一致(同视觉语言),数据源不同(iching-oracle 本地 / chief-app 走 runtime)。

## 状态(2026-07-03)

- ✅ Gradle build:`./gradlew :app:assembleDebug` 14s 完成
- ✅ APK 13.7 MB,valid Android package
- ✅ Runtime `/api/tarot/draw?spread=3` 已验证,3 张牌 位置+正逆位+解读 都对
- ❌ **本机没装 Android emulator**,无法现场启动验证 UI
- ⏳ 等主席装真机 / 模拟器后告诉我

## 边界(NWT 000071/000073)

- ChiefApp **不**作为 Flwntier GitHub release 发布 — 是 Flwntier 代码面,不是发布产品
- 塔罗牌是 chairman 端的真实任务,不是 Flwntier 自己的产品
- iching-oracle 是独立产品(Thatgfsj/iching-oracle),本仓库不复刻它的代码
