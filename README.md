# Frank Tappo

跨平台的交互式桌宠：你在电脑上敲键盘、点鼠标或按手柄，桌面上的像素角色就跟着拍击回应，并累计总次数。当前内置两个角色皮肤包——默认「西格莉卡」，可选「莫宁」；角色像素、火花颜色与锚点都来自皮肤包本身，切换立即生效并自动保存。

角色按三块互不影响的变化区域呈现：眨眼、左手拍击、右手拍击。拍击反馈参考《Bongo Cat》（Irox Games）：鼠标移动、拖动过程、滚轮和摇杆轴不触发；长按同一个键或鼠标按钮只计一次；每次输入直接拍一下（落下 200ms / 抬起 70ms），左右手交替，快速输入不排队，只在抬手后补一拍。空闲时每 2.6–6.2 秒随机眨一次眼（闭合 280ms），与拍击互不等待。计数由原生侧累计，显示在图片下方的奶油色控制栏里；右侧汉堡按钮打开「更多」窗口（角色 / 设置）。桌宠透明无边框，可拖动、可等比缩放、可点击穿透，隐藏后仍继续计数。

| 默认角色：西格莉卡 | 可选角色：莫宁 |
| --- | --- |
| ![西格莉卡](./src/assets/skins/xigerika/default.png) | ![莫宁](./src/assets/skins/moning/default.png) |

- 状态：**0.2.1**（2026-11-27）；macOS Apple Silicon 已打包，Windows 待本机构建与实机验收
- 目标平台：Windows 10/11、macOS 12+（Apple Silicon；Intel 未构建验证）
- 仓库：[github.com/frankSoft365/Frank-Tappo](https://github.com/frankSoft365/Frank-Tappo)
- 文档：[产品需求 PRD](./PRD.md) · [皮肤包契约](./docs/skin-manifest.md) · [皮肤帧流程](./docs/skin-frames-workflow.md) · [开发日志](./docs/dev-log.md) · [M0 技术验证（历史）](./docs/M0-technical-verification.md)

## 0.2.1 试用与打包

### Apple Silicon Mac

- 安装包：`src-tauri/target/release/bundle/dmg/Frank Tappo_0.2.1_aarch64.dmg`（4,699,425 字节，SHA-256 `8dbfd4194cd88020d4c851bcb9897ccffb37d533a4b5ba6277b2bf1b2941edd8`），仅适用于M系列芯片Mac。
- 打开DMG，将应用拖到“应用程序”后启动。首次使用在系统设置的“隐私与安全性 → 输入监控”中允许Frank Tappo，然后从托盘退出并重新打开。
- 当前使用Apple Development开发签名，未做Developer ID分发签名或Apple公证。本机实测：给解包后的应用加上 `com.apple.quarantine` 再评估，`spctl -a -t exec` 直接返回 `rejected`（普通打开和右键打开两种上下文都是），所以别的Mac上“仍要打开”流程不一定出现。确认安装包来自可信来源后，先拖进“应用程序”，再执行 `xattr -dr com.apple.quarantine "/Applications/Frank Tappo.app"` 去掉隔离属性即可正常启动；不要关闭系统安全检查，也不要长期跳过签名校验。
- 不要同时运行开发实例和安装版，两者共享同一设置与累计计数文件。退出应使用托盘“退出”，保证最后的计数保存。
- 托盘菜单：「点击穿透」「隐藏桌宠 / 显示桌宠」「设置…」「角色…」「退出」。后两项直接打开「更多」窗口的对应页面。

### Windows 10/11 x64 本机构建

当前不提供已验证的Windows安装包。把源码包解压到Windows电脑后，先安装Node.js 24 LTS、Rust稳定版MSVC工具链、Visual Studio 2022 Build Tools的“使用C++的桌面开发”工作负载和Windows SDK，再打开新的PowerShell窗口。

在包含package.json的项目目录执行：

```powershell
npm ci
npm run build
rustup target add x86_64-pc-windows-msvc
cargo test --manifest-path src-tauri/Cargo.toml --target x86_64-pc-windows-msvc
npm run tauri -- build --target x86_64-pc-windows-msvc --bundles nsis --ci
```

构建完成后，从`src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis/`取得安装程序；首次构建需要联网下载依赖与构建工具。安装程序在缺少WebView2时可能联网安装运行时；应用本身不需要账号或远端服务。

Windows包尚未配置分发签名，可能出现SmartScreen提示；不要关闭系统防护。发给其他同学前，在构建电脑上实际验证安装/启动、键鼠长按只计一次、松开重按、隐藏/显示菜单、设置缩放、切换角色、退出后计数恢复；手柄支持还需连接真实设备测试。构建成功不等于这些功能已通过验收。

#### Windows 原生拖动

Windows 拖动只从网页发送一次起始请求，随后用正确打包的原生屏幕坐标进入 Win32 原生移动循环，由系统接管鼠标捕获、持续跟随、松手和取消；不再逐帧发送拖动 IPC，也不使用网页指针捕获或动画帧队列驱动窗口。主窗口在 `WM_MOVING` 中用当前鼠标位置和按下时保存的抓取点重新计算目标位置，再按鼠标所在显示器工作区和 DPI 限制边界。抓取点在整次拖动中保持不变，窗口停在边界时反向拖动先消耗越界距离，回到允许范围后恢复原抓取点跟随，不累积偏移；不限制或移动鼠标指针。边距保持顶部/左右 24 逻辑像素、底部 0，macOS 仍走已验证的网页拖动路径。

验收时分别在角色中心、边缘和控制栏按下拖动，快速来回移动并急停，检查抓取点保持、原地单击不跳位、松手/Esc 取消后不继续移动，并且能到达右侧 24 逻辑像素边距和底部边界；再在四边四角拖到屏幕边缘后反向拖回，确认多次往返不积累偏移。Windows 目标的类型检查与浏览器 IPC 回归不能替代系统级拖动验收。

## 角色与皮肤包

- 每个皮肤包 = `src/assets/skins/<id>/manifest.json` + 六张同画布 PNG（197×164 画布，展示宽 300）：`手 ∈ {抬起, 左, 右} × 眼 ∈ {睁, 闭}`。
- 三个变化区域互相独立：眨眼只换眼睛，左右手拍击只换对应手臂。渲染是整帧可见性切换（`#stage[data-hand][data-eyes]`），不是叠加覆盖层，同画布让对齐与缩放由构造保证，不会切出接缝。
- manifest 记录画布、展示宽度、火花颜色与左右锚点、三块区域的 rows/cols/changed_px，以及六帧的 md5/sha256；运行时只读显示参数，帧位由 `src/skins.ts` 装配。
- 新增角色不需要改拍击或眨眼代码：放入六帧并写 manifest，即出现在「更多」窗口的角色页。契约细节见 [docs/skin-manifest.md](./docs/skin-manifest.md)，补帧流程见 [docs/skin-frames-workflow.md](./docs/skin-frames-workflow.md)。
- 校验器：`cd src-tauri && cargo test --test skin_manifest`，按契约核对帧哈希、区域交集与“区域外 0 变化”。**当前两个包仍报 fail**：license 都是 TODO（原画来源与再分发权限待确认）；xigerika 另有 54 条既有素材条目（左手帧 r=130 的 25px 边缘混色 + (151,82) 1px）。这些是已知待拍板项，不是校验器误报，细节见开发日志 2026-11-25 章节。

## 交互与界面

### 控制栏与「更多」窗口

- 控制栏显示累计总数与汉堡按钮，按钮无悬停变色；点汉堡开/关「更多」窗口，重开时保留上次所在页。
- 「更多」窗口为 880×640 透明圆角整窗（Tauri 标签仍是 `settings`）：左侧 176px 侧栏分「角色 / 设置」，顶栏可拖动。
- 角色页：当前角色预览（顶部缺口圆）、固定 5 列角色网格，点选立即切换并保存；「配件」暂无内容。
- 设置页：语言（跟随系统 / 中文 / English，即时生效并持久化）、大小滑杆（60%–150%，默认 100%，拖动只预览、松手保存）、查看日志（在 Finder 中定位 `~/Library/Logs/com.franktappo.app/frank-tappo.log`）、退出应用；页脚含作者邮箱、「开源组件声明」与「关于 Frank Tappo」弹窗。

### 拖动、缩放与边界

- 按住角色或控制栏拖动，位移超过 3 屏幕像素才开始拖动，原地单击不移动窗口；macOS 与 Windows 两条拖动路径见上文。
- 以默认实际尺寸为 100%；macOS 保持网页画布不变，通过原生容器整体缩放，避免每档重排。贴底时放大、缩小均固定底边；贴左/右/顶部时，放大固定所在边，缩小按当前窗口中心；角落按横纵轴组合，窗口始终完整可见。
- 拖动与缩放限制在显示器可用桌面内（避开菜单栏和 Dock/任务栏）：顶部、左右各留 24 逻辑像素，底部不留额外间距；启动默认在右下角。

### 计数与持久化

- 计数由 Rust 统一累计，前端只显示权威总数；隐藏桌宠仍计数，显示时重新同步。`counter.json` 仅保存总数，每 5 秒有变化才原子写入，正常托盘退出时保存最后状态；强制终止或断电可能丢失最近 5 秒。语言、角色与大小同样持久化到应用配置目录。
- 浏览器预览计数仅在当前页面有效，不写入原生累计值。

### 输入支持

- 手柄使用原生 gilrs 监听，不依赖网页焦点：macOS 为 IOHID，Windows 选择 XInput 而非需要窗口焦点的 WGI。Windows 仅覆盖最多 4 个 XInput 兼容设备，不含任意 DirectInput 设备，后台/独占游戏及设备兼容性仍需真机验证。浏览器预览只响应键鼠，手柄需运行原生应用。
- 手柄只接受 ButtonPressed 事件，包括映射为按钮的扳机越过库内按下阈值；连接后的前 500ms 用于初始状态稳定。键盘长按忽略自动重复，鼠标同一按钮在释放前只计一次，释放后再次按下重新累计；不同按键和按钮独立计数，macOS 修饰键释放不另计一次。手柄监听阻塞等待事件，未启用震动反馈。

## 技术栈

| 层 | 选型 |
| --- | --- |
| 外壳 | Tauri 2（Rust） |
| 前端 | TypeScript + Vite（无框架） |
| 全局输入 | 自建 CGEventTap（macOS）/ [rdev](https://crates.io/crates/rdev)（Windows）监听键鼠；gilrs 监听手柄（macOS IOHID / Windows XInput） |
| 角色显示 | 皮肤包六帧透明 PNG（197×164 画布），三块变化区域可见性切换、等比缩放 |
| 设置界面 | 独立 `settings.html`（880×640 透明 WebView，启动时预创建隐藏） |

## 开发

```bash
npm install
npm run tauri dev   # 启动桌面应用（Windows；macOS 见下方说明）
npm run dev         # 仅启动前端，在浏览器里预览拍击与「更多」窗口
npm run build       # 前端类型检查 + 打包
cd src-tauri && cargo test   # Rust 测试
```

macOS 开发注意事项：

- 全局键盘监听需要**「输入监控」**权限（系统设置 → 隐私与安全性 → 输入监控），并且权限会记在**启动应用的父进程**名下。用 `npm run tauri dev` 从终端启动时，勾选 `frank-tappo` 也不会生效。
- 请改用 `./scripts/dev-macos.sh`：它会启动 Vite、编译后用 `launchctl submit` 让系统直接拉起二进制（label `com.franktappo.dev`）。日志在 `/tmp/frank-err.log`。
- 授权后**必须重启一次应用**——macOS 15 会在进程内缓存授权判定。重新编译不再影响授权：脚本会用本机 Apple Development 证书对二进制签名，签名身份固定。
- 应用以 `Accessory` 模式运行（无 Dock 图标），通过菜单栏托盘图标操作。
- 发布包：`APPLE_SIGNING_IDENTITY="Apple Development: ……" npx tauri build`；不指定身份时 Tauri 只做 adhoc 签名。DMG 阶段偶发临时挂载卷占用导致 `hdiutil couldn't unmount`，卸载该构建卷后重跑即可。

## 目录结构

```
src/main.ts                拍击 / 眨眼 / 计数显示 / 拖动时序
src/skins.ts               皮肤包加载、切换与显示参数装配
src/settings.ts            「更多」窗口逻辑（角色页 / 设置页 / 弹窗）
src/i18n.ts                中英文案字典与本地化应用
src/styles.css             桌宠与控制栏样式
index.html                 桌宠窗口
settings.html              「更多」窗口
src/assets/skins/<id>/     manifest.json + 六帧 PNG（xigerika / moning）
src-tauri/src/input.rs     全局输入监听与计数持久化
src-tauri/src/windows_pet.rs  Windows 原生拖动
src-tauri/src/permissions.rs  macOS 权限检测
src-tauri/src/settings.rs  语言 / 角色 / 大小持久化与系统语言解析
src-tauri/src/lib.rs       窗口、托盘、命令注册、macOS 拖动
src-tauri/tests/skin_manifest.rs  皮肤包校验器
scripts/dev-macos.sh       macOS 开发启动（launchctl + 稳定签名）
docs/                      皮肤契约、补帧流程、开发日志、M0 报告
PRD.md                     产品需求文档
```

## 已知限制

- macOS 安装包为 Apple Development 开发签名、未公证；Intel Mac 未构建验证。
- Windows 尚无已验证安装包，原生拖动与手柄仍需真机验收；无分发签名。
- 皮肤包 license TODO 与校验器红灯（moning 1 条 license、xigerika 54 条既有素材条目），等素材归位与 manifest 三条口径拍板。
- 空闲 CPU 优化仍在待办（M1 首要项）。

## 隐私约定（不可妥协）

- 键盘、鼠标和手柄统一只向界面传递 `input:activity`，载荷只有时间戳与十进制字符串总数；不传递键位分区、鼠标按钮、设备标识或模拟轴值，不建立输入历史。
- macOS 监听器不读取虚拟键码；按键字符、输入内容不读取、不落盘、不上传、不打印。
- 无账号、无远端服务即可使用。