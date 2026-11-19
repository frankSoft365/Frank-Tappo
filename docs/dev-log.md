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

### 权限归属问题与全局输入验证（同日，第二轮）

- 在「输入监控」勾选 `frank-tappo` 后 `CGPreflightListenEventAccess()` 仍为 false；用 Swift 探针从终端运行同样返回 false。
- 定位：macOS 按**父进程归属**计算输入监控权限。通过 `npm run tauri dev` 启动时，实际被校验的是启动链（Qoder / 终端），勾选子进程无效。
- 解决：新增 `scripts/dev-macos.sh`，Vite 单独运行，二进制由 `launchctl submit` 拉起。启动日志变为：
  ```
  [permissions] accessibility=false input_monitoring=true
  [input] starting global listener
  ```
- 全局键盘/鼠标输入在真实应用中生效；端到端延迟 175 个样本：min 0ms / p50 1ms / p95 ≤ 9ms / max 19ms，优于 <100ms 预算。
- 窗口由 CGWindowList 确认：300×360，layer 5（floating），位置 (1146, 572)，托盘状态项存在。

### 输入后端替换、开发签名与需求调整（同日）

- **rdev macOS 崩溃**：两次崩溃报告显示 `EXC_BREAKPOINT`，栈在 rdev `string_from_code → TSMGetInputSourceProperty`，原因是它在事件 tap 线程调用输入法/TSM API，触发主队列断言。该转换同时违反隐私约定（会读取输入源信息）。
- 替换为自建 listen-only `CGEventTap`：只读取虚拟键码做左右分区，不做任何字符转换；鼠标按键与滚轮直接读按钮号。崩溃根因消除，隐私边界更严格。Windows 继续使用 rdev。
- **TCC 授权反复失效**：确认 ad-hoc 签名每次重编译都会换 cdhash。开发脚本改为用现有 Apple Development 证书签名（`com.franktappo.app`），签名身份固定，后续编译不再丢授权。
- **需求变更：取消睡眠机制**。兔宝永远待机，只展示拍击反应，不自动休眠、不要求交互；移除 Sleep / Wake 状态、Zzz、伸懒腰相关代码与 PRD 条目。
- **授权生效条件澄清**：macOS 15 在进程内缓存输入监控判定，用户在系统设置里勾选后必须重启应用才生效（之前误判为 3 秒轮询即可生效）。重启后日志为 `input_monitoring=true`，CGEventTap 正常监听。
- **签名稳定性已验证**：touch 触发重编译 + 同一证书重签 + 重启后，授权依然有效（`input_monitoring=true`），M1 迭代不再需要反复授权。
- 新 CGEventTap 后端延迟复测：75 个样本 min 0ms / p50 1ms / p95 ≤ 2ms / max 2ms，无崩溃。
- **release 资源复测**（`tauri build --no-bundle`，空闲 45s 后采样）：CPU ≈ 4.2%（单核），RSS ≈ 83MB。内存达标，CPU 超 0.3% 预算；空闲 CSS 动画的持续合成是主因，M1 优化。
- **人工确认**：兔宝长时间放置不再睡觉（无 Zzz/闭眼）；托盘「点击穿透」勾选后可点到后方窗口，取消后恢复。

### 本地化与设置页（同日）

- 明确「双语 = 本地化」：界面不再中英混排，按语言只显示一种文案。
- 新增设置页（托盘「设置…」打开独立窗口）：语言三选项「跟随系统 / 中文 / English」，默认跟随系统，切换即时生效并持久化。
- 后端 `settings.rs` 负责读取/保存 `settings.json`（应用配置目录），使用 `sys-locale` 解析系统语言（macOS `en_CN` → 英文）；`set_language` 广播 `settings:changed`，同时重建托盘菜单并更新设置窗口标题。
- 前端 `i18n.ts` 提供文案字典；兔宝窗口的提示、权限横幅、点击穿透徽标与设置页共用同一套文案，切换无需重启。
- 验证：浏览器预览（系统语言解析、中文切换、单语言提示）通过；真机托盘菜单与设置窗口语言切换通过；`settings.json` 正确落盘。

### 默认角色更换为墨线小猫（同日）

- 需求变更：默认角色按新参考图重画——白色填充、粗墨线描边的小猫，长尾在身后盘成带半调圆点的环形，头顶蓝心星花、右爪抱一枚蓝色小胶囊；移除桌面与键盘场景，角色完整站在桌面上。
- 左右前爪仍为独立元素：键位分左右拍击，鼠标点击双爪联动、滚轮轻弹；保留呼吸与眨眼。
- 文案中的「兔宝 / Bunny」统一改为「Tappo」，避免名称与形象不符，中英文提示同步更新。
- 预览截图：`docs/assets/tappo-idle.png`（300×360 @2x）。

### 默认尺寸与缩放设置（同日）

- 默认窗口缩到 75%（225×270），作为「小一圈」的默认值；窗口改为右下角锚定，缩放时右下角保持不动。
- 设置页新增「大小」滑块（当前范围 50%–150%，后续可调），拖动即时预览、松手保存；缩放写入 `settings.json` 的 `scale`，旧配置缺省为 0.75。
- 缩放链路统一走 Rust `apply_pet_scale`：`preview_scale` 只调整窗口不落盘，`set_scale` 落盘并广播；起点与位置基于显示器 `work_area` 计算，顺带修掉右下边距偏大的问题。
- 小尺寸适配：窗口 ≤200pt 时提示与权限横幅缩小字号并允许换行；设置窗口高度增至 470。
- 验证：1.5×/0.5×/0.75× 均以同一右下角锚定；设置页拖动滑块时窗口连续变化，松手后 `settings.json` 记录 0.5。

### 待办

- 空闲 CPU 优化（M1 首要项，当前 release ≈ 4.2%）。
- 单图导入、音效、内置形象精修。
- Windows 真机验证（同一份构建流程）。
