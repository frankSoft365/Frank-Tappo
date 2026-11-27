//! 皮肤包校验器：`docs/skin-manifest.md` 第 6 节的七条 fail 条件。
//!
//! 只在 `cargo test` 里跑——png / sha2 声明在 dev-dependencies，不进发布二进制。
//! 报错一律带 `(row, col)` 与两侧颜色，不接受"整体不像"这种结论。

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};

/// 与 `/tmp/tappo-pipeline` 的 Swift 量测脚本同一条规则：alpha 阈值 128 判透明/不透明，
/// 两侧都透明的像素不计入 diff（校验只约束可见像素，不约束底层遗留数据）。
const ALPHA_CUTOFF: u8 = 128;

/// 合成素材的画布边长。
const SIDE: usize = 4;

type Pixel = [u8; 4];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Hand {
    Up,
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Eyes {
    Open,
    Closed,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
    hand: Hand,
    eyes: Eyes,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Frame {
    state: State,
    file: String,
    /// 信息性字段：第 6 节第 1 条按 `sha256` 比对，md5 只是与既有验收记录对得上。
    #[serde(default)]
    md5: Option<String>,
    sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Region {
    rows: [usize; 2],
    cols: [usize; 2],
    changed_px: usize,
}

impl Region {
    fn contains(&self, row: usize, col: usize) -> bool {
        self.rows[0] <= row && row <= self.rows[1] && self.cols[0] <= col && col <= self.cols[1]
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Regions {
    eyes: Region,
    hand_left: Region,
    hand_right: Region,
}

impl Regions {
    fn by_name(&self, name: RegionName) -> &Region {
        match name {
            RegionName::Eyes => &self.eyes,
            RegionName::HandLeft => &self.hand_left,
            RegionName::HandRight => &self.hand_right,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum RegionName {
    Eyes,
    HandLeft,
    HandRight,
}

impl RegionName {
    fn label(self) -> &'static str {
        match self {
            RegionName::Eyes => "eyes",
            RegionName::HandLeft => "hand_left",
            RegionName::HandRight => "hand_right",
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Palette {
    mode: String,
    #[serde(default)]
    colors: Option<Vec<[u8; 3]>>,
    tol: u8,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct License {
    source: String,
    #[allow(dead_code)]
    redistribution_ok: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Canvas {
    width: usize,
    height: usize,
}

/// 运行时接口层的显示参数（2026-11-27）：前端装载器把它写进 CSS 变量。
/// 单位：`width` = 画布内显示宽度（392 设计空间的 px），`drop` = 画布行，
/// `impact.*` = #stage 坐标系的 392 px 锚点。校验器只收字段，不判数值。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct Display {
    width: usize,
    drop: usize,
    impact: Impact,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct Impact {
    color: String,
    left: Anchor,
    right: Anchor,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct Anchor {
    x: usize,
    y: usize,
}

/// 侧面配件挂点（2026-11-27）：画布坐标系，`x`/`y` 是配件锚角在画布里的位置，
/// `align` 决定框的哪个角对准锚点。挂点只定位、不规定配件大小。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Mounts {
    #[serde(default)]
    side: Option<SideMounts>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SideMounts {
    #[serde(default)]
    left: Option<Mount>,
    #[serde(default)]
    right: Option<Mount>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Mount {
    x: usize,
    y: usize,
    align: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    format: u32,
    id: String,
    name: HashMap<String, String>,
    license: License,
    canvas: Canvas,
    #[serde(default)]
    display: Option<Display>,
    #[serde(default)]
    mounts: Option<Mounts>,
    regions: Regions,
    palette: Palette,
    provenance: String,
    frames: Vec<Frame>,
}

#[derive(Debug, Clone)]
struct Image {
    w: usize,
    h: usize,
    px: Vec<Pixel>,
}

impl Image {
    fn rc(&self, idx: usize) -> (usize, usize) {
        (idx / self.w, idx % self.w)
    }

    /// 变更像素集合的实测外框与计数；空集合返回 None。
    fn envelope(&self, set: &HashSet<usize>) -> Option<(usize, usize, usize, usize, usize)> {
        let mut r0 = usize::MAX;
        let mut r1 = 0usize;
        let mut c0 = usize::MAX;
        let mut c1 = 0usize;
        for &idx in set {
            let (r, c) = self.rc(idx);
            r0 = r0.min(r);
            r1 = r1.max(r);
            c0 = c0.min(c);
            c1 = c1.max(c);
        }
        if set.is_empty() {
            None
        } else {
            Some((r0, r1, c0, c1, set.len()))
        }
    }
}

#[derive(Default)]
struct Report {
    failures: Vec<String>,
    notes: Vec<String>,
}

impl Report {
    fn fail(&mut self, msg: impl Into<String>) {
        self.failures.push(msg.into());
    }

    fn note(&mut self, msg: impl Into<String>) {
        self.notes.push(msg.into());
    }
}

fn color_text(px: &Pixel) -> String {
    format!("rgba({},{},{},{})", px[0], px[1], px[2], px[3])
}

fn sha256_hex(bytes: &[u8]) -> String {
    let sum = Sha256::digest(bytes);
    sum.iter().map(|b| format!("{b:02x}")).collect::<String>()
}

fn visible(px: &Pixel) -> bool {
    px[3] > ALPHA_CUTOFF
}

fn changed(a: &Pixel, b: &Pixel, tol: u8) -> bool {
    if visible(a) != visible(b) {
        return true;
    }
    if !visible(a) {
        return false;
    }
    (0..3).any(|c| a[c].abs_diff(b[c]) > tol)
}

fn max_channel_delta(a: &Pixel, b: &Pixel) -> u8 {
    (0..3).map(|c| a[c].abs_diff(b[c])).max().unwrap_or(0)
}

fn decode(path: &Path) -> Result<Image, String> {
    let file = fs::File::open(path).map_err(|e| format!("{} 打不开：{e}", path.display()))?;
    let decoder = png::Decoder::new(std::io::BufReader::new(file));
    let mut reader = decoder
        .read_info()
        .map_err(|e| format!("{} 读不出 PNG 头：{e}", path.display()))?;
    if reader.info().interlaced {
        return Err(format!(
            "{} 是隔行（Adam7）存储，校验器不支持，请交非隔行 PNG",
            path.display()
        ));
    }
    let mut buf = vec![0u8; reader.output_buffer_size().unwrap_or(0)];
    let info = reader
        .next_frame(&mut buf)
        .map_err(|e| format!("{} 解不出像素：{e}", path.display()))?;
    if info.bit_depth != png::BitDepth::Eight {
        return Err(format!(
            "{} 的位深是 {:?}，素材必须是 8 bit",
            path.display(),
            info.bit_depth
        ));
    }
    let bpp = match info.color_type {
        png::ColorType::Rgba => 4,
        png::ColorType::Rgb => 3,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Grayscale => 1,
        png::ColorType::Indexed => {
            return Err(format!(
                "{} 是索引色 PNG，请先展开成 RGBA 再入库",
                path.display()
            ));
        }
    };
    let w = info.width as usize;
    let h = info.height as usize;
    let mut px = vec![[0u8; 4]; w * h];
    for y in 0..h {
        for x in 0..w {
            let src = y * info.line_size + x * bpp;
            let some = |k: usize| buf.get(src + k).copied().unwrap_or(0);
            px[y * w + x] = match bpp {
                4 => [some(0), some(1), some(2), some(3)],
                3 => [some(0), some(1), some(2), 255],
                2 => [some(0), some(0), some(0), some(1)],
                _ => [some(0), some(0), some(0), 255],
            };
        }
    }
    Ok(Image { w, h, px })
}

fn diff_set(a: &Image, b: &Image, tol: u8) -> HashSet<usize> {
    (0..a.px.len())
        .filter(|&i| changed(&a.px[i], &b.px[i], tol))
        .collect()
}

fn hand_region(hand: Hand) -> Option<RegionName> {
    match hand {
        Hand::Up => None,
        Hand::Left => Some(RegionName::HandLeft),
        Hand::Right => Some(RegionName::HandRight),
    }
}

/// 第 2 节第 1 条：一帧只允许改它自己状态用到的区域。
fn allowed_regions<'a>(manifest: &'a Manifest, state: &State) -> Vec<(RegionName, &'a Region)> {
    let mut out = Vec::new();
    if let Some(name) = hand_region(state.hand) {
        out.push((name, manifest.regions.by_name(name)));
    }
    if state.eyes == Eyes::Closed {
        out.push((RegionName::Eyes, &manifest.regions.eyes));
    }
    out
}

fn frame_for(manifest: &Manifest, hand: Hand, eyes: Eyes) -> Option<&Frame> {
    manifest
        .frames
        .iter()
        .find(|f| f.state.hand == hand && f.state.eyes == eyes)
}

fn load_manifest(dir: &Path) -> Result<Manifest, String> {
    let path = dir.join("manifest.json");
    let text = fs::read_to_string(&path).map_err(|e| {
        format!(
            "{} 读不出来：{e}（manifest 是角色包的唯一事实源）",
            path.display()
        )
    })?;
    serde_json::from_str::<Manifest>(&text)
        .map_err(|e| format!("{} 的 manifest.json 不符合字段契约：{e}", path.display()))
}

fn coord_text(img: &Image, idx: usize) -> String {
    let (r, c) = img.rc(idx);
    format!("r={r} c={c}")
}

fn validate(dir: &Path) -> Report {
    let mut rep = Report::default();
    let manifest = match load_manifest(dir) {
        Ok(m) => m,
        Err(e) => {
            rep.fail(e);
            return rep;
        }
    };

    // ---- 第 6 节第 1 条：契约字段、画布、帧集合、文件与哈希 ----
    if manifest.format != 0 {
        rep.fail(format!(
            "format = {} 本校验器不认，当前只实现 format 0",
            manifest.format
        ));
    }
    if manifest
        .name
        .values()
        .all(|v| v.trim().is_empty() || v.contains("TODO"))
    {
        rep.fail("manifest.name 每种语言都是空的或 TODO，正式分发包必须有显示名");
    }
    if manifest.license.source.trim().is_empty() || manifest.license.source.contains("TODO") {
        rep.fail(format!(
            "manifest.license.source = {:?} 没写实：原画来源与再分发权限必须写在包里（redistribution_ok = {}）",
            manifest.license.source, manifest.license.redistribution_ok
        ));
    }
    if let Some(mounts) = &manifest.mounts {
        if let Some(side) = &mounts.side {
            for (label, mount) in [("left", &side.left), ("right", &side.right)] {
                let Some(m) = mount else { continue };
                if m.x > manifest.canvas.width || m.y > manifest.canvas.height {
                    rep.fail(format!(
                        "mounts.side.{label} 的挂点 ({}, {}) 超出画布 {}×{}：锚角必须落在画布坐标系内",
                        m.x, m.y, manifest.canvas.width, manifest.canvas.height
                    ));
                }
                if m.align != "bottom-left" && m.align != "bottom-right" {
                    rep.fail(format!(
                        "mounts.side.{label}.align = {:?} 只允许 bottom-left 或 bottom-right",
                        m.align
                    ));
                }
            }
        }
    }
    match manifest.palette.mode.as_str() {
        "free" | "indexed" | "core_plus_antialias" => {}
        other => rep.fail(format!(
            "palette.mode = {other:?} 不是三档之一（free / indexed / core_plus_antialias）"
        )),
    }
    if manifest.palette.mode == "free" && manifest.palette.tol == 0 {
        rep.fail("palette.mode = free 时 tol 不许是 0：全彩抗锯齿栅格必须留 12，见第 4 节");
    }
    if manifest.palette.mode == "indexed" && manifest.palette.tol > 0 {
        rep.fail("palette.mode = indexed 时 tol 必须是 0，否则卡色没有意义");
    }
    if manifest.palette.mode == "indexed" && manifest.palette.colors.is_none() {
        rep.fail("palette.mode = indexed 但没有 palette.colors 色卡");
    }
    match manifest.provenance.as_str() {
        "source" | "derived" => {}
        other => rep.fail(format!("provenance = {other:?} 只允许 source 或 derived")),
    }
    let states = [
        (Hand::Up, Eyes::Open),
        (Hand::Left, Eyes::Open),
        (Hand::Right, Eyes::Open),
        (Hand::Up, Eyes::Closed),
        (Hand::Left, Eyes::Closed),
        (Hand::Right, Eyes::Closed),
    ];
    for (hand, eyes) in states {
        if frame_for(&manifest, hand, eyes).is_none() {
            rep.fail(format!(
                "frames 缺状态 hand={hand:?} / eyes={eyes:?}：六张整帧是全积，不许缺角"
            ));
        }
    }
    for f in &manifest.frames {
        let path = dir.join(&f.file);
        let Ok(bytes) = fs::read(&path) else {
            rep.fail(format!("{} 声明了但文件不存在", path.display()));
            continue;
        };
        let actual = sha256_hex(&bytes);
        if actual != f.sha256 {
            rep.fail(format!(
                "{} 哈希对不上：manifest 写 {}，实测 {actual}",
                path.display(),
                f.sha256
            ));
        }
    }

    // ---- 解码 ----
    let mut imgs: HashMap<(Hand, Eyes), Image> = HashMap::new();
    for f in &manifest.frames {
        match decode(&dir.join(&f.file)) {
            Ok(img) => {
                if img.w != manifest.canvas.width || img.h != manifest.canvas.height {
                    rep.fail(format!(
                        "{} 是 {}x{}，manifest.canvas 声明 {}x{}：所有帧必须是同一张全画布",
                        f.file, img.w, img.h, manifest.canvas.width, manifest.canvas.height
                    ));
                }
                imgs.insert((f.state.hand, f.state.eyes), img);
            }
            Err(e) => rep.fail(e),
        }
    }
    let Some(base) = imgs.get(&(Hand::Up, Eyes::Open)) else {
        rep.fail("base（hand=up / eyes=open）解不出来，坐标判定全部跳过");
        return rep;
    };
    if base.px.iter().all(|p| !visible(p)) {
        rep.fail("base 整张全透明，这不是一张角色帧");
        return rep;
    }

    let tol = manifest.palette.tol;
    let region_sets: Vec<(RegionName, HashSet<usize>)> = [
        (RegionName::Eyes, Hand::Up, Eyes::Closed),
        (RegionName::HandLeft, Hand::Left, Eyes::Open),
        (RegionName::HandRight, Hand::Right, Eyes::Open),
    ]
    .into_iter()
    .map(|(name, hand, eyes)| {
        let set = imgs
            .get(&(hand, eyes))
            .map(|img| diff_set(img, base, tol))
            .unwrap_or_default();
        (name, set)
    })
    .collect();

    // ---- 第 2 节第 2 条 + 第 6 节第 4 条：区域实测值与两两交集 ----
    for (name, set) in &region_sets {
        let region = manifest.regions.by_name(*name);
        let restricted: HashSet<usize> = set
            .iter()
            .copied()
            .filter(|&idx| {
                let (r, c) = base.rc(idx);
                region.contains(r, c)
            })
            .collect();
        match base.envelope(&restricted) {
            None => rep.fail(format!(
                "{} 在声明框 rows {}..{} cols {}..{} 内一个变更像素都没有",
                name.label(),
                region.rows[0],
                region.rows[1],
                region.cols[0],
                region.cols[1]
            )),
            Some((r0, r1, c0, c1, n)) => {
                if (r0, r1, c0, c1)
                    != (region.rows[0], region.rows[1], region.cols[0], region.cols[1])
                {
                    rep.fail(format!(
                        "{} 声明 rows {}..{} cols {}..{}，实测框是 rows {r0}..{r1} cols {c0}..{c1}（{} px）",
                        name.label(),
                        region.rows[0],
                        region.rows[1],
                        region.cols[0],
                        region.cols[1],
                        n
                    ));
                }
                if n != region.changed_px {
                    rep.fail(format!(
                        "{} 声明 changed_px = {}，实测 {n}（tol = {tol}）",
                        name.label(),
                        region.changed_px
                    ));
                }
                rep.note(format!(
                    "{} 实测 rows {r0}..{r1} cols {c0}..{c1}，{n} px",
                    name.label()
                ));
            }
        }
    }
    for i in 0..region_sets.len() {
        for j in (i + 1)..region_sets.len() {
            let (a_name, a) = &region_sets[i];
            let (b_name, b) = &region_sets[j];
            let inter: Vec<usize> = a.intersection(b).copied().collect();
            if inter.is_empty() {
                rep.note(format!(
                    "{} ∩ {} = 0 px",
                    a_name.label(),
                    b_name.label()
                ));
                continue;
            }
            let coords: Vec<String> = inter
                .iter()
                .take(8)
                .map(|&idx| coord_text(base, idx))
                .collect();
            rep.fail(format!(
                "{} ∩ {} = {} px，三区域必须两两不相交（例：{}）",
                a_name.label(),
                b_name.label(),
                inter.len(),
                coords.join(", ")
            ));
        }
    }

    // ---- 第 6 节第 2、3 条：区域外 0 变化（含透明区被涂）----
    for f in &manifest.frames {
        let Some(img) = imgs.get(&(f.state.hand, f.state.eyes)) else {
            continue;
        };
        if img.px.len() != base.px.len() {
            continue;
        }
        let allowed = allowed_regions(&manifest, &f.state);
        let mut outside: Vec<usize> = diff_set(img, base, tol)
            .into_iter()
            .filter(|&idx| {
                let (r, c) = img.rc(idx);
                !allowed.iter().any(|(_, region)| region.contains(r, c))
            })
            .collect();
        outside.sort_unstable();
        if outside.is_empty() {
            rep.note(format!("{} 区域外 0 变化", f.file));
            continue;
        }
        let allowed_text = allowed
            .iter()
            .map(|(name, _)| name.label())
            .collect::<Vec<_>>()
            .join(" + ");
        for &idx in outside.iter().take(40) {
            rep.fail(format!(
                "{} {} 在状态 {:?}/{:?} 的允许区域 [{}] 之外：base {} → frame {}，最大通道差 {}",
                f.file,
                coord_text(img, idx),
                f.state.hand,
                f.state.eyes,
                allowed_text,
                color_text(&base.px[idx]),
                color_text(&img.px[idx]),
                max_channel_delta(&base.px[idx], &img.px[idx]),
            ));
        }
        if outside.len() > 40 {
            rep.fail(format!(
                "{} 区域外变化共 {} px（上面只列前 40）",
                f.file,
                outside.len()
            ));
        }
    }

    // ---- 第 6 节第 5 条：色表 ----
    match manifest.palette.mode.as_str() {
        "indexed" => {
            let colors = manifest
                .palette
                .colors
                .clone()
                .unwrap_or_default()
                .into_iter()
                .map(|c| (c[0], c[1], c[2]))
                .collect::<HashSet<(u8, u8, u8)>>();
            for f in &manifest.frames {
                let Some(img) = imgs.get(&(f.state.hand, f.state.eyes)) else {
                    continue;
                };
                let mut offenders: Vec<(usize, usize, Pixel)> = img
                    .px
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| visible(p))
                    .filter(|(_, p)| !colors.contains(&(p[0], p[1], p[2])))
                    .map(|(idx, p)| {
                        let (r, c) = img.rc(idx);
                        (r, c, *p)
                    })
                    .collect();
                if offenders.is_empty() {
                    rep.note(format!("{} 色卡外颜色 = 0", f.file));
                    continue;
                }
                offenders.sort();
                for (r, c, p) in offenders.iter().take(8) {
                    rep.fail(format!(
                        "{} r={r} c={c} {} 不在 indexed 色卡内",
                        f.file,
                        color_text(p)
                    ));
                }
                if offenders.len() > 8 {
                    rep.fail(format!(
                        "{} 色卡外颜色像素共 {} 个（上面只列前 8）",
                        f.file,
                        offenders.len()
                    ));
                }
            }
        }
        "core_plus_antialias" => rep.fail(
            "palette.mode = core_plus_antialias 的判定还没实现：第 6 节第 5 条里「区域边界一圈取核心色」\
             与「两核心色插值复现」的阈值还没定，等第 9 节第 1 条拍板后再写"
                .to_string(),
        ),
        _ => {
            rep.note("palette.mode = free：跳过色表判定，只靠坐标约束（第 4 节结论 3）");
        }
    }

    // ---- 第 6 节第 6 条：derived 包按 manifest 参数重算 ----
    if manifest.provenance == "derived" {
        rep.fail(
            "provenance = derived 的帧重算（第 6 节第 6 条）还没实现：派生参数字段尚未进 manifest 契约，\
             等第 9 节第 3 条拍板"
                .to_string(),
        );
    }

    // ---- 第 6 节第 7 条：lock.json ----
    let mut lock = String::from("{\n  \"id\": ");
    lock.push_str(&serde_json::to_string(&manifest.id).unwrap());
    lock.push_str(",\n  \"tol\": ");
    lock.push_str(&manifest.palette.tol.to_string());
    lock.push_str(",\n  \"files\": [\n");
    let entries: Vec<String> = manifest
        .frames
        .iter()
        .map(|f| {
            let hex = fs::read(dir.join(&f.file))
                .map(|bytes| sha256_hex(&bytes))
                .unwrap_or_default();
            format!(
                "    {{ \"file\": {}, \"sha256\": {}, \"md5\": {} }}",
                serde_json::to_string(&f.file).unwrap(),
                serde_json::to_string(&hex).unwrap(),
                serde_json::to_string(&f.md5).unwrap(),
            )
        })
        .collect();
    lock.push_str(&entries.join(",\n"));
    lock.push_str("\n  ]\n}\n");
    let lock_path = dir.join("lock.json");
    match fs::write(&lock_path, &lock) {
        Ok(()) => rep.note(format!("lock.json 已输出到 {}", lock_path.display())),
        Err(e) => rep.fail(format!("lock.json 写不出来（{}）：{e}", lock_path.display())),
    }

    rep
}

fn print_report(dir: &Path, rep: &Report) {
    println!("=== 皮肤包校验：{} ===", dir.display());
    for n in &rep.notes {
        println!("  · {n}");
    }
    for f in &rep.failures {
        println!("  FAIL {f}");
    }
    println!(
        "  合计：{} 条 fail，{} 条实测记录",
        rep.failures.len(),
        rep.notes.len()
    );
}

fn repo_skins_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri 的上一级是仓库根")
        .join("src/assets/skins")
}

/// 默认遍历仓库里的全部皮肤包（`src/assets/skins/<id>/`），一批一起判；
/// `TAPPO_SKIN_DIR` 指定单个目录时只判那一个。
#[test]
fn skin_packages_pass_the_manifest_contract() {
    let dirs: Vec<PathBuf> = match std::env::var_os("TAPPO_SKIN_DIR") {
        Some(d) if !d.to_string_lossy().trim().is_empty() => vec![PathBuf::from(d)],
        _ => {
            let mut dirs: Vec<PathBuf> = fs::read_dir(repo_skins_dir())
                .map(|entries| {
                    entries
                        .filter_map(|entry| entry.ok())
                        .map(|entry| entry.path())
                        .filter(|path| path.join("manifest.json").is_file())
                        .collect()
                })
                .unwrap_or_default();
            dirs.sort();
            dirs
        }
    };
    assert!(
        !dirs.is_empty(),
        "没找到任何皮肤包（每个包 = src/assets/skins/<id>/manifest.json），校验无从谈起"
    );
    let mut failed: Vec<String> = Vec::new();
    for dir in &dirs {
        let rep = validate(dir);
        print_report(dir, &rep);
        if !rep.failures.is_empty() {
            failed.push(format!("{}：{} 条 fail", dir.display(), rep.failures.len()));
        }
    }
    assert!(
        failed.is_empty(),
        "皮肤包没过校验器（{}）；逐条坐标与颜色见 `cargo test --test skin_manifest -- --nocapture`",
        failed.join("；")
    );
}

/// 合成包：左手帧改了一个只属于右手区域的像素，必须按坐标报出来。
#[test]
fn validator_fails_a_frame_that_changes_outside_its_region() {
    let dir = fixture_dir("outside");
    let _ = std::fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("建临时目录");

    let base = vec![[10u8, 20, 30, 255]; SIDE * SIDE];
    let frames = [
        ("up-open.png", Hand::Up, Eyes::Open, vec![]),
        (
            "up-closed.png",
            Hand::Up,
            Eyes::Closed,
            vec![((0, 1), [200u8, 10, 10, 255])],
        ),
        (
            "left-open.png",
            Hand::Left,
            Eyes::Open,
            vec![
                ((2, 2), [10u8, 200, 10, 255]),
                ((2, 1), [250u8, 250, 10, 255]),
            ],
        ),
        (
            "right-open.png",
            Hand::Right,
            Eyes::Open,
            vec![((3, 0), [10u8, 10, 200, 255])],
        ),
        (
            "left-closed.png",
            Hand::Left,
            Eyes::Closed,
            vec![
                ((2, 2), [10u8, 200, 10, 255]),
                ((0, 1), [200u8, 10, 10, 255]),
                ((2, 1), [250u8, 250, 10, 255]),
            ],
        ),
        (
            "right-closed.png",
            Hand::Right,
            Eyes::Closed,
            vec![((3, 0), [10u8, 10, 200, 255]), ((0, 1), [200u8, 10, 10, 255])],
        ),
    ];
    write_fixture(&dir, &base, &frames, fixture_regions());

    let rep = validate(&dir);
    let text = rep.failures.join("\n");
    println!("=== 合成越界包 ===\n{text}");
    assert!(
        text.contains("left-open.png r=2 c=1"),
        "左手帧越界到右手列段的那个像素必须按坐标报出来，实际报错：{text}"
    );
    assert!(
        text.contains("left-closed.png r=2 c=1"),
        "闭眼 + 左手的组合同样要报出越界像素：{text}"
    );
    assert!(
        !text.contains("up-closed.png r=0 c=1"),
        "eyes 区域内的合法改动不该被判越界：{text}"
    );
    assert!(
        !text.contains("right-open.png r=3 c=0"),
        "右手区域内的合法改动不该被判越界：{text}"
    );

    // 文件被换掉：哈希与解码都要报。
    fs::write(dir.join("left-open.png"), b"not a png").unwrap();
    let rep = validate(&dir);
    let text = rep.failures.join("\n");
    assert!(
        text.contains("left-open.png") && (text.contains("哈希对不上") || text.contains("解不出")),
        "帧文件被替换后必须报错，实际：{text}"
    );

    let _ = fs::remove_dir_all(&dir);
}

/// 合成包：眨眼与左手改到同一像素，第 4 条两两交集必须 fail。
#[test]
fn validator_fails_overlapping_regions() {
    let dir = fixture_dir("overlap");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("建临时目录");

    let base = vec![[10u8, 20, 30, 255]; SIDE * SIDE];
    let frames = [
        ("up-open.png", Hand::Up, Eyes::Open, vec![]),
        (
            "up-closed.png",
            Hand::Up,
            Eyes::Closed,
            vec![((2, 2), [200u8, 10, 10, 255])],
        ),
        (
            "left-open.png",
            Hand::Left,
            Eyes::Open,
            vec![((2, 2), [10u8, 200, 10, 255])],
        ),
        (
            "right-open.png",
            Hand::Right,
            Eyes::Open,
            vec![((3, 0), [10u8, 10, 200, 255])],
        ),
        (
            "left-closed.png",
            Hand::Left,
            Eyes::Closed,
            vec![((2, 2), [90u8, 90, 90, 255])],
        ),
        (
            "right-closed.png",
            Hand::Right,
            Eyes::Closed,
            vec![((3, 0), [10u8, 10, 200, 255])],
        ),
    ];
    // eyes 的声明框故意覆盖 (2,2)，让越界判定不干扰交集判定。
    write_fixture(
        &dir,
        &base,
        &frames,
        r#""eyes": { "rows": [0, 2], "cols": [0, 3], "changed_px": 1 },
    "hand_left": { "rows": [2, 3], "cols": [2, 3], "changed_px": 1 },
    "hand_right": { "rows": [2, 3], "cols": [0, 1], "changed_px": 1 }"#,
    );

    let rep = validate(&dir);
    let text = rep.failures.join("\n");
    println!("=== 合成重叠包 ===\n{text}");
    assert!(
        text.contains("eyes ∩ hand_left = 1 px"),
        "眨眼与左手改到同一像素必须被第 4 条拦住，实际：{text}"
    );
    let _ = fs::remove_dir_all(&dir);
}

fn fixture_dir(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "tappo-skin-fixture-{}-{tag}",
        std::process::id()
    ))
}

fn fixture_regions() -> &'static str {
    r#""eyes": { "rows": [0, 1], "cols": [0, 3], "changed_px": 1 },
    "hand_left": { "rows": [2, 3], "cols": [2, 3], "changed_px": 1 },
    "hand_right": { "rows": [2, 3], "cols": [0, 1], "changed_px": 1 }"#
}

fn write_fixture(
    dir: &Path,
    base: &[Pixel],
    frames: &[(&str, Hand, Eyes, Vec<((usize, usize), Pixel)>)],
    regions: &str,
) {
    let mut declared = String::new();
    for (name, hand, eyes, edits) in frames {
        let mut px = base.to_vec();
        for ((r, c), v) in edits {
            px[r * SIDE + c] = *v;
        }
        let path = dir.join(name);
        write_png(&path, SIDE as u32, SIDE as u32, &px);
        let digest = sha256_hex(&fs::read(&path).unwrap());
        declared.push_str(&format!(
            "{{\"state\":{{\"hand\":\"{}\",\"eyes\":\"{}\"}},\"file\":\"{}\",\"md5\":null,\"sha256\":\"{}\"}},",
            match hand {
                Hand::Up => "up",
                Hand::Left => "left",
                Hand::Right => "right",
            },
            match eyes {
                Eyes::Open => "open",
                Eyes::Closed => "closed",
            },
            name,
            digest
        ));
    }
    declared.pop();
    fs::write(
        dir.join("manifest.json"),
        format!(
            r#"{{
  "format": 0,
  "id": "fixture",
  "name": {{ "en": "Fixture" }},
  "license": {{ "source": "合成素材，仅测试用", "redistribution_ok": false }},
  "canvas": {{ "width": 4, "height": 4 }},
  "regions": {{
    {regions}
  }},
  "palette": {{ "mode": "free", "colors": null, "tol": 12 }},
  "provenance": "source",
  "frames": [{declared}]
}}"#
        ),
    )
    .unwrap();
}

fn write_png(path: &Path, w: u32, h: u32, px: &[Pixel]) {
    let mut out: Vec<u8> = Vec::new();
    {
        let mut encoder = png::Encoder::new(std::io::Cursor::new(&mut out), w, h);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().expect("写 PNG 头");
        let flat: Vec<u8> = px.iter().flat_map(|p| p.iter().copied()).collect();
        writer.write_image_data(&flat).expect("写 PNG 像素");
    }
    fs::write(path, &out).expect("写文件");
}
