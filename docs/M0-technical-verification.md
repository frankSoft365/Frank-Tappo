# M0 技术验证报告

- 日期：2026-11-19
- 阶段：M0（11/19 – 11/26）
- 验证者：Frank（单人）
- 工程提交：见 git 历史（M0 首批提交）
- 验证环境：macOS 15.8.1 (24H32)，Apple M2，2560×1664 Retina（缩放后 1512×982），Rust 1.99.1，Node v22.19.0，Tauri 2.12.7，rdev 0.5.3

## 目标

验证 PRD 中锁定的 5 项技术风险：

1. 双平台全局输入监听是否可用、macOS 辅助功能权限流程是否顺畅。
2. 透明 + 无边框 + 置顶窗口在两个平台的表现（macOS 是否出现黑底、无法穿透）。
3. 点击穿透（ignore cursor events）是否可靠、能否与托盘开关联动。
4. 输入事件到动画首帧的延迟。
5. 常驻资源占用是否满足性能预算。

> 分发渠道已定为官网安装包直装，App Store 沙盒限制不再构成风险，本项关闭。

## 本轮结果

![兔宝待机](./assets/bunny-idle.png)

| # | 验证项 | 结果 | 证据 / 备注 |
| --- | --- | --- | --- |
| 1 | 透明窗口渲染 | ✅ 通过 | 300×360 无黑底、无边框，桌面透出；CGWindowList 读取 bounds (1146, 572)、layer 5（悬浮层）、alpha 1.0 |
| 2 | 始终置顶 | ✅ 通过 | 窗口位于 layer 5（floating），切换应用不遮挡；托盘状态项 34×36 @ (881, 0) |
| 3 | 无 Dock 图标 / 托盘 | ✅ 通过 | 进程以 `Accessory` 策略启动；菜单栏状态项创建成功 |
| 4 | 点击穿透 | ⚠️ 待托盘交互确认 | `set_ignore_cursor_events` + 托盘勾选项已实现 |
| 5 | 全局键盘监听 | ✅ 通过 | `[permissions] input_monitoring=true` → `[input] starting global listener`；真实全局输入触发前端动画 |
| 6 | 全局鼠标监听 | ✅ 通过 | 同一监听线程；鼠标点击/滚轮事件正常进入前端（点击反应沿用测试） |
| 7 | 权限引导 | ✅ 通过 | 同时检测辅助功能 / 输入监控；双语横幅、授权弹窗、深链跳转；授权后 3s 内自动启动监听，无需重启 |
| 8 | 输入 → 动画延迟 | ✅ 通过 | 125 个真实按键样本：min 0ms / p50 1ms / p95 ≤ 9ms / max 15ms（预算 <100ms） |
| 9 | 空闲 CPU / 内存 | ❌ 未达标（debug） | 空闲 CPU ≈ 2.7%（单核），RSS ≈ 100MB；需 release 复测并优化空闲动画 |
| 10 | 打包产物（.dmg） | ⏳ 未开始 | 计划 M2 处理签名与公证 |

前端动画与交互已在浏览器预览中验证：左右半区分别触发左右手拍击、鼠标点击触发双手拍击、hint 淡化、眨眼与呼吸动画均正常（tap 状态截图见 `assets/bunny-tap.png`）。

## 已知问题

1. **macOS 权限按父进程归属**：从终端启动的开发版，输入监控授权会被算到启动它的父进程（Qoder / 终端）名下，导致「输入监控」里勾选 `frank-tappo` 后 `CGPreflightListenEventAccess()` 仍返回 false。改用 `launchctl submit` 让系统直接拉起二进制后授权立即生效。正式发布为 `.app` 由 Finder 启动，不存在此问题。
2. **重编译会使授权失效**（已缓解）：ad-hoc 签名每次链接都会改变 cdhash。开发脚本现在用现有 Apple Development 证书签名（`com.franktappo.app`），签名身份稳定，M1 反复编译无需重新授权。
3. **空闲 CPU 超出预算**：常驻 CSS 动画（呼吸/耳朵/眨眼）持续 60fps 合成。M1 优化方向：窗口隐藏/睡眠时暂停动画、降低空闲动画帧率、减少合成层。
4. **窗口右下边距偏大**（约 48pt），定位逻辑需按 `visibleFrame` 精修（M1）。
5. rdev 0.5.3 引入的传递依赖 `block v0.1.6` 有 future-incompat 警告（不影响运行）。
6. macOS 屏幕录制权限限制导致无法自动截取原生窗口，原生视觉项依赖人工确认。

## 人工验证清单（需授权输入监控）

1. 打开「系统设置 → 隐私与安全性 → 输入监控」，勾选 `frank-tappo`：
   `/Users/frank/myCodes/Frank Tappo/src-tauri/target/debug/frank-tappo`
   （若列表中没有，点击应用内权限横幅按钮触发系统授权弹窗）
2. 回归应用：兔宝窗口应透明、始终置顶、位于屏幕右下。
3. 在任意应用敲击键盘：左手键位 → 左手拍击，右手键位 → 右手拍击；连续敲击应连贯跟手。
4. 终端（或 `tail -f /tmp/frank-err.log`）应周期性输出：
   `[latency] samples=25 min=…ms p50=…ms p95=…ms max=…ms`
5. 托盘菜单：勾选「点击穿透」后鼠标可穿透兔宝；显示/隐藏、退出正常。

## 结论与下一步

- Tauri 2 + rdev 在 macOS 15 / Apple M2 上可编译、可启动，窗口、托盘与全局输入链路成立，未发现硬阻塞。
- 延迟显著优于预算（p50 1ms / p95 ≤ 9ms / max 15ms），可满足「输入即反馈」体验。
- 权限链路的坑已定位并写入开发流程：macOS 下用 `scripts/dev-macos.sh` 启动（Vite + `launchctl submit`），避免父进程归属问题。
- 下一步：托盘点击穿透人工确认 → release 构建测 CPU/内存 → 安排 Windows 真机验证（M0 出口标准）。

## 复现步骤

macOS（关键：用 launchctl 独立启动，避免 TCC 把权限算到终端/Qoder 名下）：

```bash
cd "/Users/frank/myCodes/Frank Tappo"
npm install
./scripts/dev-macos.sh
tail -f /tmp/frank-err.log
```

Windows 及常规 Tauri 流程：

```bash
npm run tauri dev
```
