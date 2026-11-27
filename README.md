# Frank Tappo

跨平台的交互式桌宠。当前展示原图抠出的草帽男孩：保留人物、帽子、吸管和饮料杯的原始颜色与姿势，背景透明，主体图片不重绘。左下方叠加独立透明手部图层，以《Bongo Cat》（Irox Games）的按键/点击反馈为参考：键盘按下、鼠标按钮按下和受支持手柄的按钮按下，统一触发拍击并累计一次，不区分输入内容。鼠标移动、拖动过程、滚轮转动和摇杆轴变化不触发。每次输入直接拍下，160ms内抬起；快速输入重新触发当前动作，不自动循环、不排队补播，累计次数不受动画合并影响。控制栏显示累计总数，原生应用自动保存，重启继续累计。图片下方设有奶油色、完全不透明、无边框的胶囊形控制栏，并带柔和底部阴影；栏比人物宽20%，基准高度72，两侧外延及轻微覆盖避免图片底角露在圆角外侧。右侧 ghost 齿轮按钮可显示或隐藏「更多」窗口（角色 / 设置），无悬停或按下变色，图标和手部随桌宠等比缩放。

![Tappo preview](./src/assets/boy-with-drink.png)

- 状态：**0.2.1**（2026-11-27）；macOS Apple Silicon已打包，Windows待本机构建与实机验收
- 目标平台：Windows 10/11、macOS 12+（Apple Silicon / Intel）
- 产品文档：[PRD.md](./PRD.md) · [M0 验证报告](./docs/M0-technical-verification.md) · [开发日志](./docs/dev-log.md)

## 0.2.1 试用与打包

### Apple Silicon Mac

- 安装包：`src-tauri/target/release/bundle/dmg/Frank Tappo_0.2.1_aarch64.dmg`（4,699,425 字节，SHA-256 `8dbfd4194cd88020d4c851bcb9897ccffb37d533a4b5ba6277b2bf1b2941edd8`），仅适用于M系列芯片Mac。
- 打开DMG，将应用拖到“应用程序”后启动。首次使用在系统设置的“隐私与安全性 → 输入监控”中允许Frank Tappo，然后从托盘退出并重新打开。
- 当前使用Apple Development开发签名，未做Developer ID分发签名或Apple公证。本机实测：给解包后的应用加上 `com.apple.quarantine` 再评估，`spctl -a -t exec` 直接返回 `rejected`（普通打开和右键打开两种上下文都是），所以别的Mac上“仍要打开”流程不一定出现。确认安装包来自可信来源后，先拖进“应用程序”，再执行 `xattr -dr com.apple.quarantine "/Applications/Frank Tappo.app"` 去掉隔离属性即可正常启动；不要关闭系统安全检查，也不要长期跳过签名校验。
- 不要同时运行开发实例和安装版，两者共享同一设置与累计计数文件。退出应使用托盘“退出”，保证最后的计数保存。
- 桌宠显示时托盘提供“隐藏桌宠 / Hide pet”，隐藏后提供“显示桌宠 / Show pet”；长按同一个键或鼠标按钮只计一次。

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

Windows包尚未配置分发签名，可能出现SmartScreen提示；不要关闭系统防护。发给其他同学前，在构建电脑上实际验证安装/启动、键鼠长按只计一次、松开重按、隐藏/显示菜单、设置缩放、退出后计数恢复；手柄支持还需连接真实设备测试。构建成功不等于这些功能已通过验收。

#### Windows 原生拖动版

此前相对坐标修正版解决了按下跳位，但逐帧网页事件→IPC→窗口移动仍使快速拖动落后。当前Windows路径只从网页发送一次起始请求，用正确打包的原生屏幕坐标进入Win32移动循环，由系统接管鼠标捕获、持续跟随、松手和取消；不再逐帧发送drag_pet/end_pet_drag，也不使用网页指针捕获或动画帧队列驱动Windows窗口。

主窗口在WM_MOVING中用当前鼠标位置和按下时保存的抓取点重新计算目标位置，再按鼠标所在显示器工作区和DPI限制边界，不沿用系统在触边后可能重设原点的目标位置。抓取点在整次拖动中保持不变，窗口尺寸变化时保持相对比例；鼠标越过允许范围时窗口停在边界，反向拖动先消耗越界距离，回到允许范围后恢复原抓取点跟随，不保留24逻辑像素的额外偏移。不限制或移动鼠标指针；松手、取消或窗口销毁后清理本次抓取点。

移动循环期间不再由窗口Moved回调执行二次位置纠正。保留顶部/左右24逻辑像素、底部0的边距；macOS仍使用已验证的固定画布拖动路径。原生拖动遵循Windows自己的窗口拖动显示设置，程序不会修改系统设置。

先从托盘退出旧程序，将修正版源码包解压到新目录。在新目录执行`npm.cmd ci`，再执行`npm.cmd run tauri dev`即可先验证，不必先制作安装包。确认正常后退出开发实例，按上方命令重新生成并安装NSIS包；旧安装程序不会随源码更新。无需删除已有设置或计数文件。

验收时分别在人物中心、边缘和控制栏按下拖动，快速来回移动并急停，检查抓取点保持、原地单击不跳位、松手/Esc取消后不继续移动，再拖动仍正常，并能到达右侧24逻辑像素边距和底部边界。在四边和四角继续把鼠标拖到屏幕边缘，不松手反向拖回，确认鼠标返回可移动范围后仍抓住最初的位置，多次往返不积累额外偏移；再测试触边后松手或Esc取消，然后从不同位置重新拖动。测试当前系统DPI及桌宠60%、100%、150%大小。Rust测试、浏览器IPC路由回归及Windows目标Rust类型检查不等于系统级实际拖动验收，触边回拖、快速跟随、跨不同DPI显示器和原生取消仍需在Windows实测。

## 技术栈

| 层 | 选型 |
| --- | --- |
| 外壳 | Tauri 2（Rust） |
| 前端 | TypeScript + Vite（无框架） |
| 全局输入 | 自建 CGEventTap（macOS）/ [rdev](https://crates.io/crates/rdev)（Windows）监听键鼠；gilrs 监听手柄（macOS IOHID / Windows XInput） |
| 角色显示 | 原图透明 PNG 与独立手部 PNG，本地合成、等比缩放；Web Animations API 单手拍击 |

## 开发

```bash
npm install
npm run tauri dev   # 启动桌面应用（Windows；macOS 见下方说明）
npm run dev         # 仅启动前端，在浏览器里预览点击拍击与设置
npm run build       # 前端类型检查 + 打包
```

macOS 开发注意事项：

- 全局键盘监听需要**「输入监控」**权限（系统设置 → 隐私与安全性 → 输入监控），并且权限会记在**启动应用的父进程**名下。用 `npm run tauri dev` 从终端启动时，勾选 `frank-tappo` 也不会生效。
- 请改用 `./scripts/dev-macos.sh`：它会启动 Vite、编译后用 `launchctl submit` 让系统直接拉起二进制。日志在 `/tmp/frank-err.log`（延迟采样 `[latency]` 也在这里）。
- 授权后**必须重启一次应用**（`./scripts/dev-macos.sh`）——macOS 15 会在进程内缓存授权判定。
- 重新编译不再影响授权：脚本会用本机 Apple Development 证书对二进制签名，签名身份固定。
- 应用以 `Accessory` 模式运行（无 Dock 图标），通过菜单栏托盘图标操作：显示/隐藏、点击穿透、设置、退出。
- 语言默认跟随系统，可在托盘「设置…」中切换中文 / English，切换即时生效并持久化到应用配置目录。
- 设置页提供「大小」滑块（60%–150%，默认 100%，步长 1%，仅显示和保存整数百分比；以默认实际尺寸为100%，旧值加载时按最近整数百分比取整并限制到新范围），拖动只处理最新预览值，松手保存；macOS 保持392×392的网页画布不变，通过原生容器整体缩放，避免每档重排网页；画布保留小数尺寸与位置，整数窗口只作为包围盒，不将取整偏差传给人物。通常围绕桌宠画布中心缩放。贴底时放大、缩小均固定底边；贴左/右/顶部时，放大固定所在边，缩小改用当前窗口中心；角落按横纵轴组合，始终保持窗口完整可见。
- 拖动与缩放均限制在显示器可用桌面内（避开菜单栏和 Dock/任务栏）：顶部、左右各留24逻辑像素，底部不留额外间距；启动默认放在右下角。

手柄使用原生gilrs监听，不依赖网页焦点：macOS为IOHID，Windows选择XInput而非需要窗口焦点的WGI。Windows仅覆盖最多4个XInput兼容设备，不含任意DirectInput设备，后台/独占游戏及设备兼容性仍需真机验证。浏览器预览只响应键鼠，手柄需运行原生应用。

手柄只接受gilrs的ButtonPressed事件，包括映射为按钮的扳机越过库内按下阈值，不重复统计ButtonChanged或AxisChanged；连接后的前500ms用于初始状态稳定。键盘长按忽略自动重复，鼠标同一按钮在释放前只计一次，释放后再次按下重新累计；不同按键和按钮独立计数，macOS修饰键释放不另计一次。macOS只读取键盘事件的自动重复标志，不读取键码或字符；Windows既有rdev监听仅在内存保留尚未释放的控制标识，释放即移除，不读取事件文本或保存输入历史；手柄监听阻塞等待事件，未启用震动反馈。

计数由Rust统一累计，前端只显示权威总数；隐藏桌宠仍计数，显示时重新同步。应用配置目录的counter.json仅保存总数，每5秒有变化才原子写入，正常托盘退出时保存最后状态；强制终止或断电可能丢失最近5秒，开发脚本重启前应先正常退出。浏览器预览计数仅在当前页面有效，不写入原生累计值。原作的精确采样、长按和手柄轴规则存在平台/版本差异，本实现不声称逐项完全一致。

## 目录结构

```
src/                      前端：透明角色图、设置控制栏、权限引导
src/i18n.ts               中英文案字典与本地化应用
src/settings.ts           设置页逻辑（语言与大小）
settings.html             设置页入口
src-tauri/src/input.rs    全局输入监听（隐私过滤层）
src-tauri/src/permissions.rs  macOS 权限检测
src-tauri/src/settings.rs 语言设置持久化与系统语言解析
src-tauri/src/lib.rs      窗口、托盘、命令注册
scripts/dev-macos.sh      macOS 开发启动（launchctl + 稳定签名）
docs/                     M0 验证报告与开发日志
PRD.md                    产品需求文档
```

## 隐私约定（不可妥协）

- 键盘、鼠标和手柄统一只向界面传递input:activity，载荷只有时间戳与十进制字符串总数；不传递键位分区、鼠标按钮、设备标识或模拟轴值，不建立输入历史。
- macOS 监听器不再读取虚拟键码；按键字符、输入内容不读取、不落盘、不上传、不打印。
- 无账号、无远端服务即可使用。
