# 皮肤包 manifest 草案 v0

目的：批量产出不同角色，但眨眼与拍手的动作原理完全一致。像素风是硬约束（2026-11-25 定稿）。
manifest 是一个角色包的唯一事实源——加载器按它建帧，校验器按它判 fail，美术或 AI 按它交稿。

本文件仍是草案：第 9 节三条没拍板之前不接进运行时。第 6 节的校验器已按这份契约落地在 `src-tauri/tests/skin_manifest.rs`（`cargo test` 的门，不进发布二进制），涉及色表档位与派生重算的两条留了显式 fail 分支，等拍板再补。

## 1. 两条不许按角色改的不变量

**状态机与时序**（`src/main.ts:44-45,50,72-74`）

- 状态 = `hand ∈ {up, left, right}` × `eyes ∈ {open, closed}`。
- 落下 200 ms / 抬起 70 ms；闭眼 280 ms、间隔随机 2600–6200 ms；每次启动左手先、打完一次换另一只手。
- 角色包只能声明像素，**不能声明时间参数**。想加"尾巴摇一下"必须先回答它属于哪个区域，否则不进这套状态机。

**几何链路**（`src/styles.css:34,56-62`、`src-tauri/src/lib.rs:20-21`）

- 固定网页画布 392×392，`scale` 0.24–0.6（默认 0.4）由原生容器 bounds 整体缩放。
- 每帧都是同一张全画布 PNG，`object-fit: contain` + `object-position: center bottom` + 同一个 `--art-drop` 位移。
- 对齐由"同画布"这个构造保证。任何角色包不许改用局部覆盖层定位——那会重新踩缩放接缝那类坑。

## 2. 三区域：角色包必须声明的东西

一个角色包 = 三个矩形 + 一份实测证据。矩形不许拍脑袋写，必须由帧间 diff 算出来后回填。

当前角色实测（`default.png` 197×164，tol=12）：

| 区域 | rows | cols | 变化像素 | 触发源 |
| --- | --- | --- | --- | --- |
| `eyes` | 88..116 | 51..128 | 1493 | 眨眼定时器 |
| `hand_left` | 118..163 | 83..118 | 930 | `input:activity`，与右手交替 |
| `hand_right` | 130..159 | 57..82 | 707 | `input:activity`，与左手交替 |

约束（校验器逐条执行，见第 6 节）：

1. **两两交集 = 0**，按 diff 的坐标集合算，不是按 bbox 算。bbox 相交但像素集合不相交是允许的。
2. **区域外 0 变化**：任一帧与 base 的 diff 必须完全落在它声明的区域里。
3. 当前角色还欠一步：左手帧有 26 px 落在右手段内（25 px 全在 r=130 单行，最大通道差 31；1 px 在 (151,82)，base 全透明）。这 26 px 归位到 base 之后，第 2 条才对现有素材成立。
4. 上面两条是同一次污染的两种口径，别混着说。校验器实测：**按声明框限制后**三个区域两两交集 = 0；**按第 1 条的原始集合口径**，`hand_left ∩ hand_right = 24 px`（r=130 的 cols 59..82 那批 + (151,82)），这 24 px 是第 3 条那 26 px 的子集。归位那 26 px 同时消掉第 1 条的交集 fail。

## 3. manifest.json 字段

```jsonc
{
  "format": 0,
  "id": "xigerika",
  // 显示名与素材授权都要人工填：id 取自原画目录名，素材来源是 zip/demo 现行版
  "name": { "zh": "TODO", "en": "TODO" },
  "license": { "source": "TODO 原画来源与再分发权限待确认", "redistribution_ok": false },

  "canvas": { "width": 197, "height": 164 },

  // 区域矩形，rows/cols 闭区间；数值必须是 diff 实测回填，不许手填
  "regions": {
    "eyes":       { "rows": [88, 116],  "cols": [51, 128],  "changed_px": 1493 },
    "hand_left":  { "rows": [118, 163], "cols": [83, 118],  "changed_px": 930 },
    "hand_right": { "rows": [130, 159], "cols": [57, 82],   "changed_px": 707 }
  },

  // 色表模式，见第 4 节，待拍板
  "palette": { "mode": "free", "colors": null, "tol": 12 },

  // 帧来源，见第 5 节
  "provenance": "source",

  // 六张全画布整帧；图层化之后这里换成 layers 段。
  // 每条除 md5 外还有 "sha256"，那才是校验器比对的字段，实例见 src/assets/manifest.json。
  "frames": [
    { "state": { "hand": "up",    "eyes": "open"   }, "file": "default.png",              "md5": "8c72e5ba463b009a699a3b43695f6d0b" },
    { "state": { "hand": "left",  "eyes": "open"   }, "file": "left-hand.png",            "md5": "d1fbc29992a16adf6ac36dac4cf6c638" },
    { "state": { "hand": "right", "eyes": "open"   }, "file": "right-hand.png",           "md5": "6d9a7f1d8edc72a570d6d2a45fd95f20" },
    { "state": { "hand": "up",    "eyes": "closed" }, "file": "default-eyes-closed.png",  "md5": "dae286ac4f5e487f99d95317db0bf135" },
    { "state": { "hand": "left",  "eyes": "closed" }, "file": "left-hand-eyes-closed.png","md5": "13c5fe8822b401a4ab65f750811a457a" },
    { "state": { "hand": "right", "eyes": "closed" }, "file": "right-hand-eyes-closed.png","md5": "97bca7bec2ee20c7522cc347cc7e2d55" }
  ]
}
```

`license` 字段是硬要求：正式分发前每个角色的素材授权状态必须写在包里。

落地状态：`src/assets/manifest.json` 是当前角色的实例，三区域的 rows/cols/changed_px 全部由校验器实测回填（不是照抄笔记）。摘要比对用 `sha256`——`sha2` 已在 `Cargo.lock` 里，`md-5` 不在，加它要联网取新包；`md5` 保留成信息字段，与 `docs/v2-minor-updates.md` 第 1 节的验收哈希逐一对得上。第 6 节第 7 条输出的 `src/assets/lock.json` 两种摘要都写。

## 4. 色表模式：允许出现哪些颜色

先定义清楚：色表 = 这张图**允许出现哪些颜色**。模式回答的是"这个允许由谁定、卡到多严"。

| `palette.mode` | 规则 | 能判什么 | 代价 |
| --- | --- | --- | --- |
| `indexed` | 每个像素必须精确等于色卡内某一色 | `tol` 可设 0；AI 出图必须量化后才能入库；不透明遮挡可被颜色判出 | 全图重着色，观感变硬 |
| `core_plus_antialias` | 核心色必须来自色卡，允许核心色之间的抗锯齿过渡 | 区域边界必须取核心色；过渡色必须能由两核心色插值复现 | 校验要多一条"最近核心色连线"判定 |
| `free` | 不卡颜色，只卡坐标 | 只能判"该像素变没变"，`tol` 必须留 12 | 遮挡式修法查不出；AI 结果无法用颜色校验 |

### 实测（`default.png`，24188 个不透明像素）

- 6427 种颜色，其中 **4582 种只出现 1 次**（占色板 71.3%）。
- 出现 ≥32 次的核心色只有 **60 种，覆盖 43.05% 像素**；≥64 次的 29 种，覆盖 37.51%。
- 同一个橙色存在 `255,141,73 / 255,141,72 / 255,142,73` 三个近亲变体。
- 那 4582 个一次性颜色**全部落在边缘像素上，内部一次性颜色 = 0** → 它们是抗锯齿产物，不是渐变带。
- 但 **94.0% 的不透明像素都是边缘像素**（四邻里有不同色），四周同色的内部像素只有 6.0%。
- 第 2 节那 25 个左手帧混色像素，25 个全部是 base 色表外的颜色。

### 结论

1. 现在这张"像素画"是**带抗锯齿的全彩栅格**，不是索引色像素画；像素观感来自 197×164 的低分辨率块状造型，而不是有限色板。
2. 量化到 60 色会改变约 57% 像素的颜色，且集中在发丝与轮廓过渡上——直接顶到已定的两条标准：「按原稿，浅就浅」和「发丝只可被遮挡不可改色」。所以 `indexed` 不是免费的。
3. **色表不是拦那 26 px 的闸。** 那 26 px 的性质是"区域外却变了"，属坐标约束，`free` 模式照样能判。色表的价值在另外三处：AI 生成结果能被标准化收编、`tol` 能从 12 降到 0、以及"贴个不透明块挡一下"这种修法能被机器拒绝。

### 建议（相对初稿改过一次）

- 当前角色：`free`，不动它已验收的发丝过渡。
- 新角色：`core_plus_antialias`——保住柔和过渡，同时边界混色与遮挡式修法仍可判。
- 只有当"AI 出图后一步量化、完全不保留抗锯齿"成为目标时才选 `indexed`；那是拿观感换校验强度，要单独拍板。

## 5. 帧从哪来：`provenance`

- `"source"`：素材自带多状态（当前角色就是这种，帧来自原始画稿）。这种包**不满足派生规则**，只能整包人工交付 + 校验。
- `"derived"`：只交 3 张稿——base 全画布、闭眼形状、手臂形状（含旋转中心与角度），其余帧由程序生成。这是批量的目标形态。

派生规则要写成可复算的参数，不是描述：

- 闭眼 = 在 `regions.eyes` 内替换，区域外逐像素等于 base。
- 手臂 = 同一形状绕声明的锚点做**刚体旋转**，不许重绘。旧版叠加手实测用 28°（`docs/dev-log.md` 2026-11-20 单手拍击那节），可作为新角色的起点参数，但它不是当前帧的依据。
- 校验器会用 manifest 里的参数重算一遍 down 帧，与提交的帧 diff，不一致就 fail——这条只有 `derived` 包能过。

AI 的位置：只出 base 一帧和单件形状。让模型一次出六张严格对齐的帧不可行，扩散模型逐帧独立采样，区域外必然漂移，第 6 节第 3 条会大面积 fail。

## 6. 校验器 fail 条件

按顺序执行，任一条 fail 就退回，**报错必须带行列坐标和颜色值**，不接受"整体不像"这种结论。

实现位置：`src-tauri/tests/skin_manifest.rs`，跑 `cargo test --test skin_manifest -- --nocapture`（默认校验 `src/assets`，`TAPPO_SKIN_DIR` 指别的包）。png 与 sha2 只声明在 `[dev-dependencies]`，所以校验器是测试门，不进发布二进制。diff 判定与 `/tmp/tappo-pipeline` 那套 Swift 脚本同一条规则：alpha 阈值 128、tol=12、两侧都透明的像素不计入。

1. `format` 与字段自洽（`name` / `license` 不许留空或 TODO、`palette.mode` 与 `tol` 的组合不许矛盾、`provenance` 合法）、每帧尺寸 == `canvas`、六格全积不许缺角、文件存在且 `sha256` 对得上。
2. 透明区必须是真 `alpha = 0`；不许用不透明色块盖住不要的部分（这是验收标准，不是风格偏好）。实现成第 3 条的一个子情形：base 透明而该帧不透明的像素，只要落在允许区域外就单独报出"属不透明涂色"。
3. 区域外 0 变化：每帧与 base 的 diff 坐标集合 ⊆ 该帧状态用到的区域（`hand≠up` → 对应手，`eyes=closed` → eyes）。tol 由 `palette.mode` 决定。
4. 三区域两两像素交集 = 0（按原始 diff 坐标集合，见第 2 节第 4 条的两种口径）。
5. 色表合规，按模式分档：`indexed` 要求色表外颜色数 = 0，已实现；`core_plus_antialias` 要求区域边界一圈像素必须取核心色，其余非色表色必须能由两个核心色在容差内插值复现，**这条直接 fail 并说明阈值未定**（第 9 节第 1 条没拍板，语义没定就不猜）；`free` 跳过这条，只靠第 3 条的坐标判定。
6. `derived` 包：按 manifest 参数重算的帧与提交帧 diff = 0。**尚未实现**——派生参数字段还没进契约；现在声明 `provenance: "derived"` 会得到一条明确 fail，而不是被静默当成 `source` 放过。
7. 输出每个文件的摘要清单，写进包内 `lock.json`（`sha256` + `md5`）。

实测结果（当前包，2026-11-25）：**54 条 fail** = 1 条 license TODO + 1 条 `hand_left ∩ hand_right = 24 px` + `left-hand.png` 与 `left-hand-eyes-closed.png` 各 26 条越界像素。三区域实测框与 `changed_px`、六帧哈希、其余四帧的"区域外 0 变化"全部通过。第 3、4 条就是那 26 px 本该被拦住的地方，现在它拦住了；另外两条合成素材的单元测试专门验证越界和区域重叠确实会被报出，而不是靠调容差蒙过去。

## 7. 生产流程

1. AI 或美术交 base 一帧（+ `derived` 时交闭眼形状、手臂形状与旋转参数）。
2. 程序派生其余帧（`source` 包跳过这步）。
3. 跑校验器（`cargo test --test skin_manifest`，第 6 节）→ 不通过就按坐标打回，不许"贴张图挡一下"。
4. 过一遍缩放检查：0.24 / 0.4 / 0.6 三档，看区域边界有没有露边或断层。
5. 观感只能由他在浏览器预览或真机确认——桌宠窗口 `sharing = 0`，截不到，校验器只保证像素约束，不保证好看。

## 8. 实现侧要改什么（现状全是写死的）

- `index.html:14-19` 六个 `<img>` 硬编码 → 由 manifest 在启动时生成。
- `src/styles.css:77-118` 按 `#pet-art*` 的 id 写死切换规则 → 改成由状态属性驱动的统一选择器。
- `src/styles.css:34` 的 `--art-drop: calc(12 * 300 / 197 * var(--pet-unit))` 里 197 是当前 sprite 宽度 → 从 manifest 读。
- `src/assets/*.png` 平铺目录 → `src/skins/<id>/`，一个目录一个包。
- `settings.json` 增加 `skin` 字段；`input:activity`、计数、缩放链路都不动。
- 渲染侧目前没有 `image-rendering` 声明，走浏览器平滑缩小。像素风在 0.24–0.6 的小数缩放下必然被重采样，这是已验收的既有行为，本草案不改它；若要真·硬边像素观感，那是另一个决定。

## 9. 待拍板

1. **色表模式**（第 4 节）：新角色走 `core_plus_antialias`（我的建议）还是 `indexed`（观感换校验强度，会改约 57% 像素的颜色）。这条决定 `tol` 取值，也决定 AI 出图要不要强制量化。校验器里 `indexed` 分支已经能判；`core_plus_antialias` 需要先把"边界一圈取核心色"和"两核心色插值复现"的阈值写成数值，没拍板之前它走的是显式 fail，不是我猜一套判定然后假装通过。
2. **画布尺寸**：所有角色固定 197×164，还是允许每角色不同。建议统一——`--art-drop` 和 `object-fit` 这条链路只在这一套尺寸上验过。
3. **交付形态**：批量要求 `derived`（AI 只出 base，程序派生），还是允许 `source`（人工交六帧）长期共存。校验器现在只对 `source` 包出结论，`derived` 会得到一条明确 fail。
   拍板前有个实测约束要先接受：手臂帧是整形状绕腕旋转，锚点落在小数像素上，刚体旋转必然重采样，"重算 diff = 0"这条对刚体旋转永远不成立（第 4 节实测 94.0% 不透明像素是边缘像素，一个 1px 的位移就改色）。所以 `derived` 要么把手臂改成"交 3 张稿 + 只派生闭眼"（闭眼是区域内替换、不含几何变换，重算是确定性的逐像素插值，条件是把那圈 lid α 写成 manifest 参数，而不是现在这样只存在笔记里），要么把第 6 节第 6 条从 diff = 0 放宽成带容差的坐标判定。这两个都不是免费的，我不替你选。

## 10. 本草案不保证的事

- 不保证好看，只保证像素约束可机器判定；观感仍受第 7 节第 5 步的验证手段限制。
- 不覆盖双手合拍。它的前置条件是第 2 节那 26 px 归位（顺带消掉第 4 条那 24 px 原始集合交集），以及状态机要不要允许两只手同时 down——那是独立的一个决定。
- 不含图层化的字段设计。第 3 节的 `frames` 现在是整帧模式；切图层会换成 `layers` 段，等色表和交付形态拍板后再写。

## 11. 插件化接口层（2026-11-27 新增，待他确认）

这一节是接口层落地后补的契约增量，其余各节原样未动。状态机、时序、几何链路（第 1 节）全部不变。

### 11.1 包目录（已落地）

```
src/assets/skins/<id>/
  manifest.json          六帧整帧 + regions + display + 摘要
  default.png …          六个 frame 文件（文件名由 manifest.frames[].file 决定）
  lock.json              校验器输出（sha256 + md5）
```

与第 8 节清单的出入：那里写的是 `src/skins/<id>/`，实际落在 `src/assets/skins/`——Vite 的 `import.meta.glob` 与 `?url` 以 `src/` 为根直接生效，不必再搬到 `public/`。校验器的默认扫描目录随之是 `src/assets/skins/*`（`TAPPO_SKIN_DIR` 仍可指向单个包）。

**加一个角色 = 加一个目录**：`src/skins.ts` 用 `import.meta.glob` 在构建期收集所有 `*/manifest.json` 与 `*/*.png`，代码零改动。

### 11.2 manifest 新增 `display` 块

```jsonc
"display": {
  "width": 300,          // 画布内显示宽度，392 设计空间的 px（原写死的 300）
  "drop": 12,            // 下移行数，按 sprite 画布行计（原 --art-drop 里的 12）
  "impact": {
    "color": "#e85431",  // 拍击闪光色（原 styles.css 里的 #e85431）
    "left":  { "x": 138, "y": 312 },   // #stage 坐标系锚点（原 tap-impact left）
    "right": { "x": 66,  "y": 301 }    // （原 tap-impact right）
  }
}
```

- 数值全部取自当前橙角色已验收的呈现参数，语义与旧写死值一一对应：`--art-drop: calc(var(--skin-drop) * var(--skin-width) / var(--skin-canvas-w) * var(--pet-unit))` 在 12/300/197 时与旧公式逐字等价。
- 校验器（第 6 节）新增：`display` 可选，`deny_unknown_fields`；只收字段类型（`drop` 为 usize 即 ≥0），不判数值好坏——好看的量留给观感验收。

### 11.3 运行时插槽

- **六个 `<img>` id 是插槽**：`pet-art` / `pet-art-left` / `pet-art-right` / `pet-art-eyes` / `pet-art-eyes-left` / `pet-art-eyes-right`，对应 manifest 六个 `state`。index.html 不再写死 `src`，CSS 可见矩阵的分工不变。
- `applySkin(id)`（`src/skins.ts`）：等该包全部帧 decode 完成后一次性换 `src`，再把 `display` 写进 `#pet` 上的 CSS 变量（`--skin-width` / `--skin-canvas-w` / `--skin-drop` / `--impact-left-x` / `--impact-left-y` / `--impact-right-x` / `--impact-right-y` / `--impact-color`），并设 `document.documentElement.dataset.skin`。请求的包不存在时回退默认包并 `console.warn`。
- CSS 里保留橙角色默认值，JS 未跑之前页面照常渲染。

### 11.4 设置槽

- `settings.json` 增加 `skin`（serde 默认 `"xigerika"`，旧文件可直接读）；`SettingsView.skin` 下发；新增 `set_skin` 命令保存并发 `settings:changed`；main.ts 启动与事件时应用。
- 开发期可用 `?skin=<id>` 覆盖当前设置。
- **设置窗口的角色选择 UI 不在这一刀**：`set_skin` 是已就绪的写口，界面等 manifest 字段拍板后再接。

### 11.5 插槽实测（2026-11-27）

临时探针包（260 宽 / drop 0 / 蓝 `#1a6dff` / 锚点 (100,260)(180,240)，六张纯色帧），不改任何代码：

- `?skin=zz-probe`：六态可见矩阵正确；CSS 变量 260 / 197 / 0；右锚点实测 181.633px = 180 × unit；拍击动画后回到 up。
- 删掉探针包：默认包回归 300 / 197 / 12 / `#e85431`，右锚点 119.878px = 66 × unit；干净加载首拍 = 左手（交替规则不变）。
- `npm run build` 绿；校验器 54 条 fail 与已知基线一致（无新增回归），两条合成越界/重叠用例通过。
