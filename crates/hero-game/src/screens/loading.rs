//! Startup loading screen: the pack chain (`pack.toml` of the pack and of every pack it
//! extends, one after the other), fonts, every other text file of the chain (fetched in
//! parallel), `Pack::load` + `Pack::validate`, and a short media preload. Any failure leads to
//! the [`ErrorScreen`] with a retry option.
//!
//! The chain is read before the fonts because a layered pack may take its fonts (like any
//! media file) from a pack it extends: once the chain is known,
//! [`crate::platform::DataRoot::with_parent_packs`] makes the media store look in the top pack
//! first and then in each parent. Text files are read by their exact path relative to the top
//! pack (`../base/rules/game.toml` for a parent's), as `PackChain::text_files` lists them.
//!
//! In gallery mode only the chain (for the fonts), the fonts and UI sounds are loaded; a broken
//! chain is only logged there.
//!
//! **Original mode** (native, `crate::original`): when the settings ask for it and no pack was
//! chosen explicitly, the base pack is loaded as above, then the install is checked and
//! converted in memory on a worker thread (`Stage::Convert`), the result is mounted next to the
//! base pack and loading starts over with that layered pack. Every failure on this path ends on
//! [`ErrorScreen::original`], which also offers to pick another folder or to play the base pack.

use super::error::ErrorScreen;
use super::gallery::GalleryScreen;
use crate::app::{Ctx, Screen, Transition};
use crate::assets::{FileBatch, FileRequest, FirstOf, Media};
use crate::audio::{bgm, sfx};
use crate::flow::Flow;
use crate::gfx::{canvas_size, fill_rect, stroke_rect, Align, FontId, TextStyle};
use crate::platform::DataRoot;
use crate::saves;
use crate::ui::theme;
use hero_core::pack::{Pack, PackChain, PackError, Severity};
use macroquad::prelude::*;
use std::collections::BTreeMap;
use std::rc::Rc;

/// Longest time the loading screen waits for optional media before moving on.
const MEDIA_WAIT_SECONDS: f64 = 4.0;
/// Validation errors listed on the error screen.
const MAX_LISTED_ISSUES: usize = 12;
/// The manifest at the root of every pack.
const MANIFEST: &str = "pack.toml";

/// What to start once loading is done.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// The game: title screen.
    Game,
    /// The UI gallery dev screen.
    Gallery,
}

/// What follows the fonts.
enum Next {
    /// The UI gallery.
    Gallery,
    /// Read the text files of the chain: `files` (paths relative to the top pack), plus the
    /// `pack.toml` texts already read, by the same kind of path.
    Files {
        files: Vec<String>,
        manifests: BTreeMap<String, String>,
    },
    /// The pack cannot be loaded; the error screen needs the fonts to say why.
    Fail {
        title: &'static str,
        details: Vec<String>,
    },
}

enum Stage {
    Start,
    /// Reading the `pack.toml` at `path` (relative to the top pack): the top pack's first
    /// (`chain` is `None` then), then each parent's.
    Chain {
        /// Boxed: a chain is much bigger than the other stages.
        chain: Option<Box<PackChain>>,
        manifests: BTreeMap<String, String>,
        path: String,
        request: FileRequest,
    },
    Fonts {
        pending: Vec<(FontId, FirstOf)>,
        done: Vec<(FontId, Result<Vec<u8>, String>)>,
        next: Next,
    },
    Files {
        manifests: BTreeMap<String, String>,
        batch: FileBatch,
    },
    Parse(BTreeMap<String, String>),
    Media {
        since: f64,
    },
    /// Converting the original install; the result is mounted at the root.
    #[cfg(not(target_arch = "wasm32"))]
    Convert(Box<(crate::original::Conversion, crate::platform::DataRoot)>),
    Done,
}

/// Where the loading screen stands with the original mode.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Original {
    /// Not asked for (or not possible: web, gallery, explicit `--data`).
    Off,
    /// Convert the install in this folder once the base pack has loaded.
    Wanted(String),
    /// The converted pack is mounted; this pass loads it.
    Mounted,
}

pub struct LoadingScreen {
    target: Target,
    stage: Stage,
    original: Original,
    /// 0..=1 for the progress bar.
    progress: f32,
    status: String,
    fonts_ready: bool,
}

/// Parent pack directories of a (possibly incomplete) chain, nearest first.
fn parent_dirs(chain: Option<&PackChain>) -> Vec<String> {
    chain.map_or_else(Vec::new, |c| {
        c.layers().iter().skip(1).map(|l| l.dir.clone()).collect()
    })
}

/// The data root of a chain read to its end: the top pack of `current`, the packs the chain
/// extends and where the original mode's pack sits in it, for views that look past it
/// (D25 X2, [`DataRoot::public_media_paths`]).
fn chain_root(current: &DataRoot, chain: &PackChain) -> DataRoot {
    let original = chain
        .layers()
        .iter()
        .position(|l| l.manifest.id == crate::platform::ORIGINAL_PACK_ID);
    current
        .top_pack()
        .with_parent_packs(parent_dirs(Some(chain)))
        .with_original_layer(original)
}

/// A pack error for the error screen.
fn describe(e: &PackError) -> String {
    match e {
        PackError::Missing { file } => format!("{file}: 파일이 없습니다"),
        other => other.to_string(),
    }
}

impl LoadingScreen {
    pub fn new(target: Target) -> LoadingScreen {
        LoadingScreen {
            target,
            stage: Stage::Start,
            original: Original::Off,
            progress: 0.0,
            status: String::new(),
            fonts_ready: false,
        }
    }

    fn fail(&self, title: &str, details: Vec<String>, ctx: &Ctx) -> Transition {
        for d in &details {
            macroquad::logging::error!("{}: {}", title, d);
        }
        if self.original == Original::Mounted {
            // The converted pack itself does not load: the base pack is still playable.
            let mut lines = vec![
                "원작에서 변환한 팩을 불러오지 못했습니다. 기본 팩으로 계속하거나 다른 폴더를 고를 수 있습니다."
                    .to_string(),
                String::new(),
            ];
            lines.extend(details);
            return Transition::replace(ErrorScreen::original(title, lines));
        }
        let mut lines = details;
        lines.push(String::new());
        lines.push(format!("데이터 위치: {}", ctx.data_root.display()));
        if ctx.data_root.candidates().len() > 1 {
            lines.push(format!(
                "찾아본 위치: {}",
                ctx.data_root.candidates().join(", ")
            ));
        }
        if !crate::platform::is_web() {
            lines.push(format!(
                "--data <폴더> 옵션이나 {} 환경 변수로 데이터 팩 위치를 지정할 수 있습니다.",
                crate::platform::DATA_ENV
            ));
        }
        Transition::replace(ErrorScreen::fatal(title, lines, Some(self.target)))
    }

    /// Start reading the `pack.toml` at `path` (relative to the top pack).
    fn read_manifest(
        &mut self,
        ctx: &Ctx,
        chain: Option<PackChain>,
        manifests: BTreeMap<String, String>,
        path: String,
    ) {
        self.status = path.clone();
        let request = FileRequest::new(ctx.data_root.top_pack().path(&path));
        self.stage = Stage::Chain {
            chain: chain.map(Box::new),
            manifests,
            path,
            request,
        };
    }

    /// Load the fonts from the top pack and the given parents, then continue with `next`.
    fn load_fonts(&mut self, ctx: &Ctx, parents: Vec<String>, next: Next) {
        self.status = "글꼴".into();
        self.progress = 0.05;
        let root = ctx.data_root.top_pack().with_parent_packs(parents);
        let pending = [FontId::Main, FontId::Small]
            .into_iter()
            .map(|id| (id, FirstOf::new(root.media_paths(id.file()))))
            .collect();
        self.stage = Stage::Fonts {
            pending,
            done: Vec::new(),
            next,
        };
    }

    /// The chain could not be read. The game shows why (after loading the fonts it can find in
    /// the packs read so far, `parents`); the gallery does not need the pack and carries on.
    fn chain_failed(&mut self, ctx: &Ctx, parents: Vec<String>, title: &'static str, e: String) {
        let next = match self.target {
            Target::Gallery => {
                macroquad::logging::warn!("gallery: pack chain unavailable: {}", e);
                Next::Gallery
            }
            Target::Game => Next::Fail {
                title,
                details: vec![e],
            },
        };
        self.load_fonts(ctx, parents, next);
    }

    /// The whole chain has been read: look media up through it and plan the next files.
    fn chain_complete(
        &mut self,
        ctx: &mut Ctx,
        chain: PackChain,
        manifests: BTreeMap<String, String>,
    ) {
        let top = &chain.layers()[0].manifest;
        macroquad::logging::info!("pack {} {} ({})", top.id, top.version, top.name);
        for layer in &chain.layers()[1..] {
            let m = &layer.manifest;
            macroquad::logging::info!(
                "  extends {}: {} {} ({})",
                layer.dir,
                m.id,
                m.version,
                m.name
            );
        }
        let parents = parent_dirs(Some(&chain));
        let root = chain_root(&ctx.data_root, &chain);
        if root != ctx.data_root {
            // Nothing has been requested from the media store yet: start it over on the chain.
            ctx.media = Media::for_settings(root.clone(), &ctx.settings);
            ctx.data_root = root;
        }
        let next = match (self.target, chain.text_files()) {
            (Target::Gallery, _) => Next::Gallery,
            (Target::Game, Ok(files)) => {
                self.status = top.name.clone();
                Next::Files {
                    files: files.iter().map(|f| f.source_path()).collect(),
                    manifests,
                }
            }
            (Target::Game, Err(e)) => Next::Fail {
                title: "데이터 팩 오류",
                details: vec![describe(&e)],
            },
        };
        self.load_fonts(ctx, parents, next);
    }

    fn step(&mut self, ctx: &mut Ctx) -> Transition {
        match std::mem::replace(&mut self.stage, Stage::Done) {
            Stage::Start => {
                if self.original == Original::Off && self.target == Target::Game {
                    self.original = wanted_original(ctx);
                }
                self.read_manifest(ctx, None, BTreeMap::new(), MANIFEST.to_string());
            }
            Stage::Chain {
                chain,
                mut manifests,
                path,
                mut request,
            } => {
                let Some(result) = request.poll().cloned() else {
                    self.stage = Stage::Chain {
                        chain,
                        manifests,
                        path,
                        request,
                    };
                    return Transition::None;
                };
                let chain = chain.map(|c| *c);
                // Packs read so far, for the fonts of the error screen.
                let known = parent_dirs(chain.as_ref());
                // A read error already names the file (path or URL).
                let text = match result {
                    Ok(bytes) => String::from_utf8(bytes)
                        .map_err(|_| format!("{path}: not valid UTF-8 text")),
                    Err(e) => Err(e),
                };
                let text = match (text, &chain) {
                    (Ok(text), _) => text,
                    (Err(e), None) => {
                        self.chain_failed(ctx, known, "데이터 팩을 찾을 수 없습니다", e);
                        return Transition::None;
                    }
                    (Err(e), Some(c)) => {
                        let e = describe(&c.parent_unreadable(&e));
                        self.chain_failed(ctx, known, "데이터 팩 오류", e);
                        return Transition::None;
                    }
                };
                let pushed = match chain {
                    None => PackChain::new(&text),
                    Some(mut c) => c.push_parent(&text).map(|()| c),
                };
                let chain = match pushed {
                    Ok(c) => c,
                    Err(e) => {
                        self.chain_failed(ctx, known, "데이터 팩 오류", describe(&e));
                        return Transition::None;
                    }
                };
                manifests.insert(path, text);
                match chain.next_parent() {
                    Some(parent) => self.read_manifest(ctx, Some(chain), manifests, parent),
                    None => self.chain_complete(ctx, chain, manifests),
                }
            }
            Stage::Fonts {
                mut pending,
                mut done,
                next,
            } => {
                let mut i = 0;
                while i < pending.len() {
                    match pending[i].1.poll() {
                        Some(result) => done.push((pending.swap_remove(i).0, result)),
                        None => i += 1,
                    }
                }
                if !pending.is_empty() {
                    self.stage = Stage::Fonts {
                        pending,
                        done,
                        next,
                    };
                    return Transition::None;
                }
                for (id, bytes) in done {
                    ctx.gfx
                        .fonts
                        .install(id, bytes.as_deref().map_err(Clone::clone));
                }
                self.fonts_ready = true;
                self.progress = 0.1;
                if !ctx.gfx.fonts.missing().is_empty() {
                    ctx.toast(format!(
                        "Font missing: {}",
                        ctx.gfx.fonts.missing().join(", ")
                    ));
                }
                ctx.media.preload_sounds(
                    &[sfx::CURSOR, sfx::CONFIRM, sfx::CANCEL, sfx::ERROR]
                        .map(|k| format!("sfx/{k}")),
                );
                match next {
                    Next::Gallery => return Transition::replace(GalleryScreen::new()),
                    Next::Fail { title, details } => return self.fail(title, details, ctx),
                    Next::Files { files, manifests } => {
                        self.progress = 0.15;
                        self.stage = Stage::Files {
                            manifests,
                            batch: FileBatch::new(&ctx.data_root.top_pack(), files),
                        };
                    }
                }
            }
            Stage::Files {
                manifests,
                mut batch,
            } => {
                let done = batch.poll();
                let (n, total) = batch.progress();
                self.progress = 0.15 + 0.6 * (n as f32 / total.max(1) as f32);
                if let Some(cur) = batch.current() {
                    self.status = cur.to_string();
                }
                if !done {
                    self.stage = Stage::Files { manifests, batch };
                    return Transition::None;
                }
                let mut files = BTreeMap::new();
                let mut errors = Vec::new();
                for (rel, result) in batch.into_results() {
                    match result.and_then(|b| {
                        String::from_utf8(b).map_err(|_| "not valid UTF-8 text".to_string())
                    }) {
                        Ok(text) => {
                            files.insert(rel, text);
                        }
                        Err(e) => errors.push(format!("{rel}: {e}")),
                    }
                }
                if !errors.is_empty() {
                    return self.fail("데이터 팩 파일을 읽을 수 없습니다", errors, ctx);
                }
                // `Pack::load` reads the chain's `pack.toml` files through the same source.
                files.extend(manifests);
                self.status = "규칙 해석".into();
                self.progress = 0.8;
                self.stage = Stage::Parse(files);
            }
            Stage::Parse(files) => {
                let pack = match Pack::load(&files) {
                    Ok(p) => p,
                    Err(e) => return self.fail("데이터 팩 오류", vec![describe(&e)], ctx),
                };
                let issues = pack.validate();
                let mut errors = Vec::new();
                for issue in &issues {
                    let line = format!("{}: {}", issue.context, issue.msg);
                    match issue.severity {
                        Severity::Warning => {
                            macroquad::logging::warn!("pack: {}", line);
                        }
                        Severity::Error => errors.push(line),
                    }
                }
                if !errors.is_empty() {
                    let total = errors.len();
                    errors.truncate(MAX_LISTED_ISSUES);
                    if total > MAX_LISTED_ISSUES {
                        errors.push(format!("… 외 {}건", total - MAX_LISTED_ISSUES));
                    }
                    return self.fail("데이터 팩 검증 실패", errors, ctx);
                }
                #[cfg(not(target_arch = "wasm32"))]
                if let Original::Wanted(install) = &self.original {
                    let install = install.clone();
                    return self.start_conversion(ctx, &pack, install);
                }
                // Every following screen is laid out on the pack's canvas.
                ctx.gfx
                    .canvas
                    .set_size(canvas_size(&pack.manifest.presentation));
                // Saves written before the slots were split per pack move to this pack's slots.
                match saves::migrate_legacy(ctx.storage.as_mut(), &pack.manifest.id) {
                    Ok(0) => {}
                    Ok(n) => {
                        macroquad::logging::info!("moved {} saves to per-pack slots", n);
                    }
                    Err(e) => {
                        macroquad::logging::error!("save migration failed: {}", e);
                        ctx.toast(format!("이전 기록을 옮기지 못했습니다: {e}"));
                    }
                }
                // A written original pack in the chain on disk (`--data data/original`, or a mod
                // on top of one) copies from the pack it extends. The game's own original mode
                // (`Mounted`) is converted at every launch.
                #[cfg(not(target_arch = "wasm32"))]
                if matches!(self.original, Original::Off) {
                    let dir = std::path::Path::new(ctx.data_root.top_dir());
                    for (layer, why) in hero_import::pack::stale_packs(dir, &pack) {
                        macroquad::logging::warn!("original pack {}: {}", layer.display(), why);
                        ctx.toast(format!(
                            "원작 팩을 다시 변환하세요 (hero-tools original pack): {}",
                            layer.display()
                        ));
                    }
                }
                ctx.pack = Some(Rc::new(pack));
                let sounds: Vec<String> = sfx::ALL
                    .iter()
                    .map(|k| format!("sfx/{k}"))
                    .chain(std::iter::once(format!("bgm/{}", bgm::TITLE)))
                    .collect();
                ctx.media.preload_sounds(&sounds);
                ctx.media.preload_textures(&["ui/title"]);
                self.status = "음악·그림".into();
                self.progress = 0.9;
                self.stage = Stage::Media { since: ctx.time };
            }
            Stage::Media { since } => {
                if ctx.media.pending() > 0 && ctx.time - since < MEDIA_WAIT_SECONDS {
                    self.stage = Stage::Media { since };
                    return Transition::None;
                }
                self.progress = 1.0;
                return Transition::Flow(Flow::Title);
            }
            #[cfg(not(target_arch = "wasm32"))]
            Stage::Convert(mut convert) => {
                let Some(result) = convert.0.poll() else {
                    // The bar shows the conversion alone; loading the converted pack starts over.
                    self.progress = convert.0.fraction();
                    self.stage = Stage::Convert(convert);
                    return Transition::None;
                };
                let (job, root) = *convert;
                return self.conversion_done(ctx, job.install(), root, result);
            }
            Stage::Done => {}
        }
        Transition::None
    }

    /// The base pack has loaded: check the install and start converting it.
    #[cfg(not(target_arch = "wasm32"))]
    fn start_conversion(&mut self, ctx: &mut Ctx, base: &Pack, install: String) -> Transition {
        use crate::original::{check_folder, Conversion, FolderCheck, PACK_DIR};
        use hero_import::pack::PackOptions;
        let dir = std::path::PathBuf::from(&install);
        let check = check_folder(&dir);
        if !check.is_supported() {
            let title = match check {
                FolderCheck::Unreadable(_) => "원작 폴더를 찾을 수 없습니다",
                _ => "원작 폴더를 쓸 수 없습니다",
            };
            let mut lines = vec![
                format!("원작 폴더: {install}"),
                check.summary(),
                "폴더가 옮겨졌거나 지워졌다면 다시 골라 주세요. 기본 팩으로는 계속 플레이할 수 있습니다."
                    .to_string(),
            ];
            if !check.evidence().is_empty() {
                lines.push(String::new());
                lines.extend(check.evidence().iter().map(|e| format!("- {e}")));
            }
            for line in &lines {
                macroquad::logging::error!("original mode: {}", line);
            }
            return Transition::replace(ErrorScreen::original(title, lines));
        }
        let display = format!("원작 모드 (메모리 변환: {install})");
        let Some((root, extends)) = ctx.data_root.memory_pack(PACK_DIR, &display) else {
            return Transition::replace(ErrorScreen::original(
                "원작 모드를 시작할 수 없습니다",
                vec![format!(
                    "기본 팩 폴더 {} 옆에 원작 모드 팩을 둘 수 없습니다(드라이브 최상위 폴더). 기본 팩을 하위 폴더로 옮겨 주세요.",
                    ctx.data_root.display()
                )],
            ));
        };
        macroquad::logging::info!(
            "original mode: converting {} ({})",
            install,
            check.summary()
        );
        let options = PackOptions {
            // The new art's maps take seconds: drawn only while the setting asks for them (D27).
            remake_maps: ctx.settings.art == crate::settings::ArtStyle::Remake,
            ..PackOptions::for_pack(base, extends, None)
        };
        self.status = "원작 변환".into();
        self.progress = 0.0;
        self.stage = Stage::Convert(Box::new((Conversion::start(dir, options), root)));
        Transition::None
    }

    /// The conversion finished: mount the pack and load it, or say why it failed.
    #[cfg(not(target_arch = "wasm32"))]
    fn conversion_done(
        &mut self,
        ctx: &mut Ctx,
        install: &std::path::Path,
        root: crate::platform::DataRoot,
        result: Result<hero_import::pack::MemoryPack, String>,
    ) -> Transition {
        use crate::platform::memfs;
        let converted = match result {
            Ok(p) => p,
            Err(e) => {
                macroquad::logging::error!("original mode: {}: {}", install.display(), e);
                return Transition::replace(ErrorScreen::original(
                    "원작 변환 실패",
                    vec![
                        format!("원작 폴더: {}", install.display()),
                        "원작 파일을 변환하지 못했습니다. 파일이 손상되었거나 다른 판본일 수 있습니다."
                            .to_string(),
                        String::new(),
                        e,
                    ],
                ));
            }
        };
        let index = &converted.index;
        let mut incomplete = Vec::new();
        for (kind, report) in &index.assets {
            macroquad::logging::info!(
                "original mode: {} {:?}: {}",
                kind,
                report.status,
                report.summary
            );
            for e in &report.errors {
                macroquad::logging::warn!("original mode: {}: {}", kind, e);
            }
            if !report.ok() {
                incomplete.push(kind.clone());
            }
        }
        if !incomplete.is_empty() {
            ctx.toast(format!(
                "원작에서 일부를 변환하지 못해 기본 팩 그림을 씁니다: {}",
                incomplete.join(", ")
            ));
        }
        macroquad::logging::info!(
            "original mode: {} files mounted at {}",
            converted.files.len(),
            root.top_dir()
        );
        let remake_prefix = format!("gfx/{}/maps/", hero_import::remake::REMAKE_DIR);
        ctx.remake_maps_missing = !converted
            .files
            .keys()
            .any(|k| k.starts_with(&remake_prefix));
        memfs::mount(root.top_dir(), converted.files);
        ctx.media = Media::for_settings(root.clone(), &ctx.settings);
        ctx.data_root = root;
        // The original's music follows while the game runs.
        ctx.music = Some(crate::original::MusicRender::start(install.to_path_buf()));
        self.original = Original::Mounted;
        self.progress = 0.0;
        self.stage = Stage::Start;
        Transition::None
    }
}

/// Whether this launch should play the original mode (native, not with an explicit `--data`).
fn wanted_original(ctx: &Ctx) -> Original {
    if crate::platform::is_web() || crate::platform::explicit_data(&ctx.options) {
        return Original::Off;
    }
    match ctx.settings.original_to_play() {
        Some(dir) => Original::Wanted(dir.to_string()),
        None => Original::Off,
    }
}

impl Screen for LoadingScreen {
    fn name(&self) -> &'static str {
        "loading"
    }

    fn update(&mut self, ctx: &mut Ctx) -> Transition {
        // Parsing is synchronous; everything else advances once per frame.
        self.step(ctx)
    }

    fn draw(&self, ctx: &Ctx) {
        clear_background(theme::BACKGROUND);
        let gfx = &ctx.gfx;
        let (w, h) = (gfx.size().x, gfx.size().y);
        let mid = (h / 2.0).round();
        let bar = Rect::new(((w - 200.0) / 2.0).round(), mid + 35.0, 200.0, 6.0);
        fill_rect(bar, theme::GAUGE_BG);
        stroke_rect(bar, theme::BORDER_MID);
        let fill = Rect::new(
            bar.x + 1.0,
            bar.y + 1.0,
            (bar.w - 2.0) * self.progress,
            bar.h - 2.0,
        );
        fill_rect(fill, theme::TEXT_ACCENT);
        // Spinner dots (visible even before the fonts exist).
        for i in 0..8 {
            let a = i as f32 / 8.0 * std::f32::consts::TAU;
            let phase = ((ctx.time * 8.0) as i32).rem_euclid(8);
            let alpha = if i == phase { 1.0 } else { 0.25 };
            draw_circle(
                w / 2.0 + a.cos() * 8.0,
                mid + 10.0 + a.sin() * 8.0,
                1.5,
                theme::TEXT_ACCENT.with_alpha(alpha),
            );
        }
        if self.fonts_ready {
            gfx.text_aligned(
                "영걸전 Reloaded",
                0.0,
                mid - 39.0,
                w,
                Align::Center,
                TextStyle::main(theme::TEXT_ACCENT)
                    .size(2)
                    .shadow(theme::TEXT_SHADOW),
            );
            gfx.text_aligned(
                &format!("불러오는 중… {}", self.status),
                0.0,
                mid + 47.0,
                w,
                Align::Center,
                TextStyle::small(theme::TEXT_DIM),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parent_dirs_skip_the_top_pack() {
        assert!(parent_dirs(None).is_empty());
        let top = "id = \"ext\"\nname = \"ext\"\nversion = \"1\"\nextends = \"../base\"\n";
        let mut chain = PackChain::new(top).unwrap();
        assert!(parent_dirs(Some(&chain)).is_empty(), "parent not read yet");
        let base = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/base/pack.toml"),
        )
        .unwrap();
        chain.push_parent(&base).unwrap();
        assert_eq!(parent_dirs(Some(&chain)), ["../base"]);
    }

    #[test]
    fn the_chain_root_marks_where_the_original_pack_sits() {
        let manifest = |id: &str, extends: Option<&str>| {
            let mut toml = format!(
                "id = \"{id}\"
name = \"{id}\"
version = \"1\"
"
            );
            if let Some(dir) = extends {
                toml.push_str(&format!(
                    "extends = \"{dir}\"
"
                ));
            }
            toml
        };
        let chain = |layers: &[(&str, Option<&str>)]| {
            let mut chain = PackChain::new(&manifest(layers[0].0, layers[0].1)).unwrap();
            for (id, extends) in &layers[1..] {
                chain.push_parent(&manifest(id, *extends)).unwrap();
            }
            chain
        };
        let original = crate::platform::ORIGINAL_PACK_ID;
        let at = |dir: &str| DataRoot::from_dir(std::path::Path::new(dir), &[]);
        let face = "gfx/portraits/a.png";

        // The original mode (also the in-memory mount): the converted pack on top of the base.
        let root = chain_root(
            &at("/d/original"),
            &chain(&[(original, Some("../base")), ("base", None)]),
        );
        assert!(root.has_original_layer());
        assert_eq!(
            root.public_media_paths(face).unwrap()[0],
            "/d/original/../base/gfx/portraits/a.png"
        );
        // A mod on top of the original: the original is the first parent, the base below it.
        let root = chain_root(
            &at("/m"),
            &chain(&[
                ("mod", Some("../original")),
                (original, Some("../base")),
                ("base", None),
            ]),
        );
        assert_eq!(root.parent_packs(), ["../original", "../base"]);
        assert_eq!(
            root.public_media_paths(face).unwrap()[0],
            "/m/../base/gfx/portraits/a.png"
        );
        // No original pack: the face choice has nothing to reorder.
        let root = chain_root(&at("/b"), &chain(&[("base", None)]));
        assert!(!root.has_original_layer());
        assert_eq!(root.public_media_paths(face), None);
        // A chain read again forgets the previous chain's position.
        let stale = at("/d/original")
            .with_parent_packs(["../base"])
            .with_original_layer(Some(0));
        let root = chain_root(&stale, &chain(&[("base", None)]));
        assert!(!root.has_original_layer());
    }

    #[test]
    fn describes_missing_files_in_korean() {
        let e = PackError::Missing {
            file: "../base/rules/game.toml".into(),
        };
        assert_eq!(describe(&e), "../base/rules/game.toml: 파일이 없습니다");
    }
}
