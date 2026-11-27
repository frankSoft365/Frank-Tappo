//! 侧面配件包校验器（2026-11-27）。
//!
//! 一个配件包 = `src/assets/accessories/<id>/manifest.json` + 一张静态 PNG。
//! 这里只判包自身的完整性：字段契约、哈希、文件安全与透明通道；挂点归皮肤包校验器管
//! （`mounts.side`），配件和皮肤的组合兼容性由前端在运行时决定。

use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Name {
    #[serde(default)]
    zh: Option<String>,
    #[serde(default)]
    en: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct License {
    source: String,
    redistribution_ok: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    format: u32,
    id: String,
    name: Name,
    license: License,
    kind: String,
    side: String,
    file: String,
    width: u32,
    layer: String,
    sha256: String,
}

#[derive(Default)]
struct Report {
    failures: Vec<String>,
    notes: Vec<String>,
}

impl Report {
    fn fail(&mut self, message: String) {
        self.failures.push(message);
    }

    fn note(&mut self, message: String) {
        self.notes.push(message);
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

fn load_manifest(dir: &Path) -> Result<Manifest, String> {
    let path = dir.join("manifest.json");
    let text = fs::read_to_string(&path)
        .map_err(|e| format!("{} 读不出来：{e}", path.display()))?;
    serde_json::from_str::<Manifest>(&text)
        .map_err(|e| format!("{} 不符合字段契约：{e}", path.display()))
}

/// 静态配件必须是 8 bit 非隔行 RGBA，且真透明过：整张不透明通常意味着白底没抠。
fn check_png(path: &Path) -> Result<(u32, u32), String> {
    let file = fs::File::open(path).map_err(|e| format!("{} 打不开：{e}", path.display()))?;
    let decoder = png::Decoder::new(std::io::BufReader::new(file));
    let mut reader = decoder
        .read_info()
        .map_err(|e| format!("{} 读不出 PNG 头：{e}", path.display()))?;
    let header = reader.info();
    if header.interlaced {
        return Err(format!("{} 是隔行（Adam7）存储，请交非隔行 PNG", path.display()));
    }
    if header.bit_depth != png::BitDepth::Eight {
        return Err(format!(
            "{} 的位深是 {:?}，配件素材必须是 8 bit",
            path.display(),
            header.bit_depth
        ));
    }
    if header.color_type != png::ColorType::Rgba {
        return Err(format!(
            "{} 的颜色类型是 {:?}，配件素材必须是 RGBA（带透明通道）",
            path.display(),
            header.color_type
        ));
    }
    let mut buf = vec![0u8; reader.output_buffer_size().unwrap_or(0)];
    let info = reader
        .next_frame(&mut buf)
        .map_err(|e| format!("{} 解不出像素：{e}", path.display()))?;
    if info.width == 0 || info.height == 0 {
        return Err(format!("{} 的宽或高是 0", path.display()));
    }
    let pixels = &buf[..(info.width as usize * info.height as usize * 4).min(buf.len())];
    let mut transparent = false;
    let mut visible = false;
    for px in pixels.chunks_exact(4) {
        if px[3] < 255 {
            transparent = true;
        }
        if px[3] > 0 {
            visible = true;
        }
    }
    if !visible {
        return Err(format!("{} 整张全透明，没有可见像素", path.display()));
    }
    if !transparent {
        return Err(format!(
            "{} 整张不透明：配件要抠掉底色，只留配件本身",
            path.display()
        ));
    }
    Ok((info.width, info.height))
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

    let dir_name = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if manifest.format != 0 {
        rep.fail(format!(
            "format = {} 本校验器不认，当前只实现 format 0",
            manifest.format
        ));
    }
    if manifest.id != dir_name {
        rep.fail(format!(
            "manifest.id = {:?} 与目录名 {:?} 不一致",
            manifest.id, dir_name
        ));
    }
    let names = [manifest.name.zh.as_deref(), manifest.name.en.as_deref()];
    if names.iter().all(|n| {
        n.map(|v| v.trim().is_empty() || v.contains("TODO"))
            .unwrap_or(true)
    }) {
        rep.fail("manifest.name 每种语言都是空的或 TODO，正式配件必须有显示名".to_string());
    }
    if manifest.license.source.trim().is_empty() || manifest.license.source.contains("TODO") {
        rep.fail(format!(
            "manifest.license.source = {:?} 没写实：原画来源与再分发权限必须写在包里（redistribution_ok = {}）",
            manifest.license.source, manifest.license.redistribution_ok
        ));
    } else if !manifest.license.redistribution_ok {
        rep.note(format!(
            "license.redistribution_ok = false（{}）：随包分发前需要确认",
            manifest.license.source
        ));
    }
    if manifest.kind != "side" {
        rep.fail(format!(
            "kind = {:?} 目前只支持 \"side\"（戴在头一侧的静态配件）",
            manifest.kind
        ));
    }
    if manifest.side != "left" && manifest.side != "right" {
        rep.fail(format!(
            "side = {:?} 只能是 \"left\" 或 \"right\"",
            manifest.side
        ));
    }
    if manifest.layer != "front" && manifest.layer != "behind" {
        rep.fail(format!(
            "layer = {:?} 只能是 \"front\"（盖住角色）或 \"behind\"（被角色遮挡）",
            manifest.layer
        ));
    }
    if manifest.width == 0 {
        rep.fail(
            "width = 0：配件显示宽度必须大于 0（以 300 宽的角色为基准，运行时会随皮肤等比缩放）"
                .to_string(),
        );
    }
    if manifest.file.contains('/') || manifest.file.contains('\\') || manifest.file.starts_with('.') {
        rep.fail(format!(
            "file = {:?} 只允许包内的纯文件名",
            manifest.file
        ));
        return rep;
    }

    let path = dir.join(&manifest.file);
    if !path.is_file() {
        rep.fail(format!("{} 声明了但文件不存在", path.display()));
        return rep;
    }
    let bytes = match fs::read(&path) {
        Ok(b) => b,
        Err(e) => {
            rep.fail(format!("{} 读不出来：{e}", path.display()));
            return rep;
        }
    };
    let actual = sha256_hex(&bytes);
    if !actual.eq_ignore_ascii_case(&manifest.sha256) {
        rep.fail(format!(
            "{} 哈希对不上：manifest 写 {}，实测 {actual}",
            path.display(),
            manifest.sha256
        ));
    }
    match check_png(&path) {
        Ok((w, h)) => rep.note(format!(
            "{}：{w}×{h} RGBA，宽 {}（角色 300 宽基准，≈ {:.1} 原生像素/单位）",
            manifest.file,
            manifest.width,
            w as f64 / manifest.width as f64
        )),
        Err(e) => rep.fail(e),
    }

    rep
}

fn print_report(dir: &Path, rep: &Report) {
    println!("=== 配件包校验：{} ===", dir.display());
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

fn repo_accessories_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri 的上一级是仓库根")
        .join("src/assets/accessories")
}

/// 默认遍历 `src/assets/accessories/<id>/` 下的全部配件包；
/// `TAPPO_ACCESSORY_DIR` 指定单个目录时只判那一个。
#[test]
fn accessory_packages_pass_the_manifest_contract() {
    let dirs: Vec<PathBuf> = match std::env::var_os("TAPPO_ACCESSORY_DIR") {
        Some(d) if !d.to_string_lossy().trim().is_empty() => vec![PathBuf::from(d)],
        _ => {
            let mut dirs: Vec<PathBuf> = fs::read_dir(repo_accessories_dir())
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
        "没找到任何配件包（每个包 = src/assets/accessories/<id>/manifest.json），校验无从谈起"
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
        "配件包没过校验器（{}）；逐条见 `cargo test --test accessory_manifest -- --nocapture`",
        failed.join("；")
    );
}