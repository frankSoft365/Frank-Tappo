# Frank Tappo

跨平台的交互式桌宠：你在电脑上敲键盘、点鼠标，兔宝（Bunny）就会跟着拍键盘。目标是让任何人都能把自己的形象放上桌面——不只是猫。

![Bunny preview](./docs/assets/bunny-idle.png)

- 状态：**M0 技术验证**（2026-11）
- 目标平台：Windows 10/11、macOS 12+（Apple Silicon / Intel）
- 产品文档：[PRD.md](./PRD.md) · [M0 验证报告](./docs/M0-technical-verification.md) · [开发日志](./docs/dev-log.md)

## 技术栈

| 层 | 选型 |
| --- | --- |
| 外壳 | Tauri 2（Rust） |
| 前端 | TypeScript + Vite（无框架） |
| 全局输入 | [rdev](https://crates.io/crates/rdev)（Windows `WH_KEYBOARD_LL` / macOS `CGEventTap`） |
| 动画 | 原生 SVG + CSS，状态机在 `src/main.ts` |

## 开发

```bash
npm install
npm run tauri dev   # 启动桌面应用
npm run dev         # 仅启动前端，在浏览器里预览兔宝动画
npm run build       # 前端类型检查 + 打包
```

macOS 开发注意事项：

- 全局键盘监听需要给**运行应用的进程**（开发时通常是终端）开启「系统设置 → 隐私与安全性 → 辅助功能」。
- 应用内会显示权限引导横幅；`check_accessibility` / `open_accessibility_settings` 命令可主动检测与跳转。
- 应用以 `Accessory` 模式运行（无 Dock 图标），通过菜单栏托盘图标操作：显示/隐藏、点击穿透、退出。

## 目录结构

```
src/                      前端：兔宝 SVG、动画状态机、权限引导
src-tauri/src/input.rs    全局输入监听（隐私过滤层）
src-tauri/src/permissions.rs  macOS 辅助功能权限检测
src-tauri/src/lib.rs      窗口、托盘、命令注册
docs/                     M0 验证报告与开发日志
PRD.md                    产品需求文档
```

## 隐私约定（不可妥协）

- 只向界面传递「键盘左半区 / 右半区」和「鼠标按键」事件。
- 原始按键值、字符、输入内容不离开 Rust 监听线程，不落盘、不上传、不打印。
- 无账号、无远端服务即可使用。
