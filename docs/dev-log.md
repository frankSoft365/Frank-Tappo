# 开发日志

## 2026-11-19

### 产品决策定稿（PRD v0.2）

- 商业模式：完全免费，无付费内容与付费商店。
- 默认角色：原创可爱兔宝（Bunny）。
- 首发语言：中英双语。
- 分发渠道：官网安装包链接下载，不做应用商店首发。
- MVP 定制范围：单图导入先行，皮肤包与编辑器放 v1.1。
- 团队与节奏：1 人，2026 年 12 月上线。

### M0 工程启动

- 使用 `create-tauri-app` 生成 Tauri 2 + vanilla-ts 骨架，项目目录 `/Users/frank/myCodes/Frank Tappo`，分支 `main`。
- 初始化 git，基线提交 `chore: scaffold Tauri 2 app and import PRD`。
- 移除模板示例（greet、Vite/TS logo、opener 插件），保持依赖最小。

### M0 实现要点

**窗口**
- 透明、无边框、始终置顶、不可缩放，隐藏 Dock 图标（macOS `Accessory` 策略）。
- 启动时定位到主屏右下角（24px 边距，按 scale factor 换算）。
- 托盘菜单：点击穿透开关、显示/隐藏、退出。

**全局输入**
- `rdev` 在独立线程监听，只向 WebView 发送两类事件：
  - `input:key`：`{ side: "left" | "right", ts }`
  - `input:mouse`：`{ button, ts }`
- 左右手按 ANSI 键位分区近似判断；键值本身在 Rust 侧即被丢弃（隐私不变量）。
- macOS 通过 `AXIsProcessTrusted` 检测辅助功能权限；未授权时界面显示双语引导，可跳转系统设置。

**动画与状态机**
- 原创 SVG 兔宝造型（避免与 Bongo Cat 相似）：只露头和两只圆手，头从桌面后探出，双手放在小键盘上拍击，不绘制身体。
- 耳朵摆动、眨眼、呼吸；左右手按键盘左右半区分别拍击；鼠标点击双手拍击。
- 5 分钟无输入进入睡眠（耳朵下垂、闭眼、Zzz、头部下沉），任意输入唤醒并伸懒腰。
- 浏览器预览模式：无 Tauri 时用本地 keydown 近似左右分区，方便调试 UI。

**延迟采样**
- Rust 在事件发生时打毫秒时间戳，前端计算端到端差值并回报 `report_latency`（仅开发模式启用），每 25 个样本输出一次 p50/p95。

### 权限问题定位与修正（同日）

- 首轮授权「辅助功能」后启动日志仍为 `trusted: false`。定位原因有两点：
  1. rdev 在 macOS 使用 listen-only 事件 tap，真正需要的是**「输入监控」**权限（`CGPreflightListenEventAccess`）；
  2. dev 二进制每次重编译都会改变 ad-hoc 签名，已授予的 TCC 权限可能失效。
- 修正：同时检测辅助功能与输入监控；首次启动主动调用 `CGRequestListenEventAccess` 弹出系统授权框；前端每 3 秒轮询，权限就绪后自动启动监听线程，无需重启应用。
- 授权完成前冻结 Rust 侧改动，避免再次重建导致授权失效。

### M0 验证（macOS，macOS 15.8.1 / Apple M2）

- 应用可编译、启动；窗口 300×360 位于主屏右下，菜单栏托盘状态项创建成功。
- 启动日志：`[permissions] accessibility trusted: false`，全局输入监听暂不可用，需手动将开发二进制加入「辅助功能」。
- debug 构建空闲资源：CPU ≈ 2.7%（单核）、RSS ≈ 100MB，空闲动画占用偏高，列入 M1 优化。
- 原生窗口像素级截图被 macOS 屏幕录制权限拦截；动画与交互已通过浏览器预览 + DOM/动画状态断言验证。
- 详见 [M0 验证报告](./M0-technical-verification.md)。

### 待办

- 辅助功能授权后复测全局输入与端到端延迟。
- release 构建复测 CPU / 内存，优化空闲动画。
- Windows 真机验证（同一份构建流程）。
- M1：首次启动自动弹辅助功能授权、单图导入、音效。
