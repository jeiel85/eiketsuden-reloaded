//! Platform layer: everything that differs between the native build and the browser build.
//!
//! * [`LaunchOptions`] — command line (`--data <dir>`, `--original <dir>`, `--gallery`) natively,
//!   the URL hash (`#gallery`) on the web.
//! * [`DataRoot`] — where the data pack lives. Natively it is resolved from `--data`, the
//!   `EIKETSUDEN_DATA` environment variable, `<exe dir>/data/base` and `./data/base` (first match
//!   wins); on the web it is the relative URL `data/base/` next to `index.html`. Text files of the
//!   pack are read through [`DataRoot::path`], media files through [`DataRoot::media_paths`], both
//!   with `macroquad::file::load_file`, which is a file read natively and an HTTP fetch on the web.
//! * **Layered packs**: a pack whose `pack.toml` says `extends = "../base"` is built on the packs
//!   of its chain (`hero_core::pack::PackChain`). Once the loading screen has read the chain it
//!   records the parent directories ([`DataRoot::with_parent_packs`]); media files are then looked
//!   up in the top pack first and then in each parent ([`DataRoot::media_paths`]).
//! * The optional **original-data overlay** (native only): a folder written by
//!   `hero-tools original extract` from the player's own copy of the original game, chosen with
//!   `--original <dir>` or the `EIKETSUDEN_ORIGINAL` environment variable. Media files are looked
//!   up there first ([`DataRoot::media_paths`]), then in the pack(s). The web build has no overlay.
//! * The **original mode** (native only): the player picks the folder of their own copy in the
//!   game (`crate::screens::original`), the choice is kept in the settings, and at every launch
//!   the loading screen converts it into a pack held in memory ([`memfs`]) that extends the
//!   base pack ([`DataRoot::memory_pack`]). An explicit `--data` / `EIKETSUDEN_DATA` wins over
//!   it ([`explicit_data`]).
//! * [`unix_now`] — wall clock time for save timestamps.
//! * [`clipboard`] — the system clipboard, for carrying a save to another device.
//! * [`storage`] — key/value persistence for saves and settings (files natively,
//!   `localStorage` on the web).

pub mod clipboard;
pub mod memfs;
pub mod storage;
#[cfg(target_arch = "wasm32")]
pub(crate) mod web;

use std::path::{Path, PathBuf};

/// Environment variable that points at the data pack directory (native builds).
pub const DATA_ENV: &str = "EIKETSUDEN_DATA";
/// Environment variable that points at an original-data overlay folder (native builds).
pub const ORIGINAL_ENV: &str = "EIKETSUDEN_ORIGINAL";
/// File every overlay folder written by `hero-tools original extract` contains.
pub const OVERLAY_INDEX: &str = "index.json";
/// Manifest id of the original mode's pack (`hero_import::pack::PACK_ID`, which the web build
/// does not link): the layer [`DataRoot::with_original_layer`] marks.
pub const ORIGINAL_PACK_ID: &str = "original";

/// Options chosen when the game was launched.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LaunchOptions {
    /// Explicit data pack directory (`--data <dir>`), native only.
    pub data_dir: Option<PathBuf>,
    /// Original-data overlay folder (`--original <dir>`), native only.
    pub original_dir: Option<PathBuf>,
    /// Start the UI gallery dev screen instead of the game (`--gallery` / `#gallery`).
    pub gallery: bool,
    /// Problems found while parsing the options (unknown flags, ...). Logged at startup.
    pub warnings: Vec<String>,
}

impl LaunchOptions {
    /// Options of this process: `std::env::args` natively, `window.location.hash` on the web.
    pub fn from_environment() -> LaunchOptions {
        #[cfg(not(target_arch = "wasm32"))]
        {
            LaunchOptions::parse_args(std::env::args().skip(1))
        }
        #[cfg(target_arch = "wasm32")]
        {
            LaunchOptions::parse_hash(&web::location_hash())
        }
    }

    /// Parse native command line arguments (without the program name).
    pub fn parse_args(args: impl IntoIterator<Item = String>) -> LaunchOptions {
        let mut opts = LaunchOptions::default();
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--gallery" => opts.gallery = true,
                "--data" => match args.next() {
                    Some(dir) => opts.data_dir = Some(PathBuf::from(dir)),
                    None => opts
                        .warnings
                        .push("--data needs a directory argument".into()),
                },
                "--original" => match args.next() {
                    Some(dir) => opts.original_dir = Some(PathBuf::from(dir)),
                    None => opts
                        .warnings
                        .push("--original needs a directory argument".into()),
                },
                other => {
                    if let Some(dir) = other.strip_prefix("--data=") {
                        opts.data_dir = Some(PathBuf::from(dir));
                    } else if let Some(dir) = other.strip_prefix("--original=") {
                        opts.original_dir = Some(PathBuf::from(dir));
                    } else {
                        opts.warnings
                            .push(format!("unknown argument `{other}` ignored"));
                    }
                }
            }
        }
        opts
    }

    /// Parse the URL fragment of the web build, e.g. `#gallery`.
    pub fn parse_hash(hash: &str) -> LaunchOptions {
        let mut opts = LaunchOptions::default();
        for word in hash.trim_start_matches('#').split(['&', ',']) {
            match word {
                "" => {}
                "gallery" => opts.gallery = true,
                other => opts
                    .warnings
                    .push(format!("unknown URL option `{other}` ignored")),
            }
        }
        opts
    }
}

/// Location of the data pack. Every pack-relative path goes through [`DataRoot::path`] or
/// [`DataRoot::media_paths`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataRoot {
    /// Prefix of the top pack joined with pack-relative paths; empty or ending with `/`.
    prefix: String,
    /// Human readable location for error messages.
    display: String,
    /// Every location that was considered, in order (for the "pack not found" error screen).
    candidates: Vec<String>,
    /// Directories of the packs the top pack extends, nearest first, relative to the top pack
    /// directory and `/`-separated (`../base`, as `hero_core::pack::PackLayer::dir`). Empty for
    /// a pack without `extends`.
    parents: Vec<String>,
    /// Prefix of the original-data overlay (ends with a separator), native only.
    media_overlay: Option<String>,
    /// Position in the chain (0: the top pack, then [`DataRoot::parents`] in order) of the
    /// original mode's pack, when the chain has one (see [`DataRoot::with_original_layer`]).
    original_layer: Option<usize>,
}

/// Turn a directory into a path prefix that pack-relative paths can be appended to.
fn dir_prefix(dir: &Path) -> String {
    let mut prefix = dir.to_string_lossy().into_owned();
    if !prefix.is_empty() && !prefix.ends_with('/') && !prefix.ends_with('\\') {
        prefix.push('/');
    }
    prefix
}

impl DataRoot {
    /// Resolve the data pack location (and, natively, the original-data overlay) for this
    /// platform.
    pub fn resolve(opts: &LaunchOptions) -> DataRoot {
        #[cfg(target_arch = "wasm32")]
        {
            let _ = opts;
            DataRoot::from_prefix("data/base/")
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let env = std::env::var_os(DATA_ENV).map(PathBuf::from);
            let exe_dir = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(Path::to_path_buf));
            let (dir, candidates) = resolve_data_dir(
                opts.data_dir.as_deref(),
                env.as_deref(),
                exe_dir.as_deref(),
                |p| p.join("pack.toml").is_file(),
            );
            let root = DataRoot::from_dir(&dir, &candidates);
            let original_env = std::env::var_os(ORIGINAL_ENV).map(PathBuf::from);
            match resolve_overlay_dir(opts.original_dir.as_deref(), original_env.as_deref(), |p| {
                p.join(OVERLAY_INDEX).is_file()
            }) {
                Ok(Some(overlay)) => {
                    macroquad::logging::info!(
                        "media: original-data overlay {} (looked up before the pack)",
                        overlay.display()
                    );
                    root.with_media_overlay(&overlay)
                }
                Ok(None) => root,
                Err(why) => {
                    macroquad::logging::warn!("{}", why);
                    root
                }
            }
        }
    }

    /// A root given as a URL/path prefix (used by the web build and tests).
    pub fn from_prefix(prefix: &str) -> DataRoot {
        let mut prefix = prefix.to_string();
        if !prefix.is_empty() && !prefix.ends_with('/') {
            prefix.push('/');
        }
        DataRoot {
            display: prefix.clone(),
            candidates: vec![prefix.clone()],
            prefix,
            parents: Vec::new(),
            media_overlay: None,
            original_layer: None,
        }
    }

    /// A root on the local file system. Pack-relative paths are appended with `/`, which every
    /// supported OS (Windows included) accepts as a separator.
    pub fn from_dir(dir: &Path, candidates: &[PathBuf]) -> DataRoot {
        DataRoot {
            prefix: dir_prefix(dir),
            display: dir.display().to_string(),
            candidates: candidates.iter().map(|p| p.display().to_string()).collect(),
            parents: Vec::new(),
            media_overlay: None,
            original_layer: None,
        }
    }

    /// The root of a pack held in memory ([`memfs`]) that extends the pack of `self`: its
    /// directory is `name` next to this pack's directory (so `<data>/original` for
    /// `<data>/base`), and it returns `(root, extends)` with the `extends` the pack must declare
    /// (`../base`). `display` names the pack in messages. `None` when this pack's directory has
    /// no name to stand next to (a drive or file system root).
    pub fn memory_pack(&self, name: &str, display: &str) -> Option<(DataRoot, String)> {
        let dir = memfs::normalize(&self.prefix);
        let (parent, base) = match dir.rsplit_once('/') {
            Some((parent, base)) => (format!("{parent}/"), base),
            None => (String::new(), dir.as_str()),
        };
        if base.is_empty() || base == ".." || base.ends_with(':') {
            return None;
        }
        let root = DataRoot {
            prefix: format!("{parent}{name}/"),
            display: display.to_string(),
            candidates: vec![display.to_string()],
            parents: Vec::new(),
            media_overlay: self.media_overlay.clone(),
            original_layer: None,
        };
        Some((root, format!("../{base}")))
    }

    /// The same root with an original-data overlay folder whose media files take precedence.
    pub fn with_media_overlay(mut self, dir: &Path) -> DataRoot {
        self.media_overlay = Some(dir_prefix(dir));
        self
    }

    /// The same root for a layered pack: `dirs` are the directories of the packs the top pack
    /// extends, nearest first, relative to the top pack directory (`../base`; the `dir` of every
    /// `hero_core::pack::PackLayer` after the first). Replaces any earlier list.
    pub fn with_parent_packs<I, S>(mut self, dirs: I) -> DataRoot
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.parents = dirs
            .into_iter()
            .map(Into::into)
            .map(|d: String| d.trim_matches('/').to_string())
            .filter(|d| !d.is_empty())
            .collect();
        self
    }

    /// The same root without the packs the top pack extends (the loader reads the chain again
    /// before it records them with [`DataRoot::with_parent_packs`]).
    pub fn top_pack(&self) -> DataRoot {
        self.clone()
            .with_parent_packs(Vec::<String>::new())
            .with_original_layer(None)
    }

    /// The same root knowing which pack of the chain is the original mode's (its position:
    /// 0 for the top pack, `n` for the `n`-th of [`DataRoot::parent_packs`]), so a view can
    /// prefer the packs below it ([`DataRoot::public_media_paths`]). An index past the chain
    /// counts as none.
    pub fn with_original_layer(mut self, layer: Option<usize>) -> DataRoot {
        self.original_layer = layer.filter(|&l| l <= self.parents.len());
        self
    }

    /// Directories of the packs the top pack extends (see [`DataRoot::with_parent_packs`]).
    pub fn parent_packs(&self) -> &[String] {
        &self.parents
    }

    /// `rel` in the pack whose directory is `dir` relative to the top pack (empty: the top pack).
    fn in_pack(&self, dir: &str, rel: &str) -> String {
        if dir.is_empty() {
            format!("{}{rel}", self.prefix)
        } else {
            format!("{}{dir}/{rel}", self.prefix)
        }
    }

    /// Path or URL of a file relative to the top pack (`pack.toml`, `rules/game.toml`,
    /// `../base/rules/terrain.toml` for a parent's file as the chain names it), for `load_file`.
    /// Media files (fonts, index files and `credits.txt` included) are looked up through the
    /// whole chain with [`DataRoot::media_paths`] instead.
    pub fn path(&self, rel: &str) -> String {
        self.in_pack("", rel.trim_start_matches('/'))
    }

    /// Candidate paths of a pack-relative **media** file, in lookup order: the original-data
    /// overlay (when one is active), the top pack, then every pack it extends. Text files of the
    /// pack (rules, dramas) are never overlaid; the loader reads them by their exact path.
    pub fn media_paths(&self, rel: &str) -> Vec<String> {
        let rel = rel.trim_start_matches('/');
        let mut paths = Vec::with_capacity(2 + self.parents.len());
        if let Some(overlay) = &self.media_overlay {
            paths.push(format!("{overlay}{rel}"));
        }
        paths.push(self.in_pack("", rel));
        paths.extend(self.parents.iter().map(|dir| self.in_pack(dir, rel)));
        paths
    }

    /// Candidate paths of a media file for a view that prefers the packs **below** the original
    /// mode's pack (`docs/DECISIONS.md` D25 X2: the base pack's public-domain portraits instead
    /// of the original's faces): those packs first, nearest first, then everything above them in
    /// the usual order ([`DataRoot::media_paths`]), so a file only the original has still shows.
    /// `None` when that order is the usual one: no original pack in the chain, or nothing below
    /// it.
    pub fn public_media_paths(&self, rel: &str) -> Option<Vec<String>> {
        let layer = self.original_layer?;
        if layer >= self.parents.len() {
            return None;
        }
        let rel = rel.trim_start_matches('/');
        let mut paths: Vec<String> = self.parents[layer..]
            .iter()
            .map(|dir| self.in_pack(dir, rel))
            .collect();
        let mut above = self.media_paths(rel);
        // The overlay (if any), the top pack and the parents up to the original's pack.
        above.truncate(above.len() - paths.len());
        paths.extend(above);
        Some(paths)
    }

    /// Whether the chain holds the original mode's pack below the top (what
    /// [`DataRoot::public_media_paths`] can reorder around).
    pub fn has_original_layer(&self) -> bool {
        self.original_layer.is_some_and(|l| l < self.parents.len())
    }

    /// Prefix of the active original-data overlay, if any.
    pub fn media_overlay(&self) -> Option<&str> {
        self.media_overlay.as_deref()
    }

    /// Directory (or URL prefix) of the top pack, ending with a separator (empty for the
    /// current directory).
    pub fn top_dir(&self) -> &str {
        &self.prefix
    }

    /// Where the pack is, for messages.
    pub fn display(&self) -> &str {
        &self.display
    }

    /// Every location that was considered, first match first.
    pub fn candidates(&self) -> &[String] {
        &self.candidates
    }
}

/// Whether the data pack was chosen explicitly (`--data` or [`DATA_ENV`]); the original mode
/// chosen in the game then does not apply. Always `false` on the web.
pub fn explicit_data(opts: &LaunchOptions) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        opts.data_dir.is_some() || std::env::var_os(DATA_ENV).is_some_and(|v| !v.is_empty())
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = opts;
        false
    }
}

/// Pick the data pack directory. Explicit choices (`--data`, then the environment variable) are
/// used even when they do not contain a pack so the error screen can name them; otherwise the
/// first implicit location containing `pack.toml` wins, falling back to `./data/base`.
/// Returns the chosen directory and every candidate that was considered.
pub fn resolve_data_dir(
    arg: Option<&Path>,
    env: Option<&Path>,
    exe_dir: Option<&Path>,
    has_pack: impl Fn(&Path) -> bool,
) -> (PathBuf, Vec<PathBuf>) {
    if let Some(dir) = arg {
        return (dir.to_path_buf(), vec![dir.to_path_buf()]);
    }
    if let Some(dir) = env.filter(|d| !d.as_os_str().is_empty()) {
        return (dir.to_path_buf(), vec![dir.to_path_buf()]);
    }
    let mut candidates = Vec::new();
    if let Some(exe) = exe_dir {
        candidates.push(exe.join("data").join("base"));
    }
    candidates.push(PathBuf::from("data").join("base"));
    let chosen = candidates
        .iter()
        .find(|c| has_pack(c))
        .cloned()
        .unwrap_or_else(|| PathBuf::from("data").join("base"));
    (chosen, candidates)
}

/// Pick the original-data overlay folder: `--original` wins over the environment variable (an
/// empty variable counts as unset). A folder without [`OVERLAY_INDEX`] is refused with a
/// message saying how to create one, so pointing at the install itself is caught.
pub fn resolve_overlay_dir(
    arg: Option<&Path>,
    env: Option<&Path>,
    is_overlay: impl Fn(&Path) -> bool,
) -> Result<Option<PathBuf>, String> {
    let Some(dir) = arg.or(env.filter(|d| !d.as_os_str().is_empty())) else {
        return Ok(None);
    };
    if is_overlay(dir) {
        Ok(Some(dir.to_path_buf()))
    } else {
        Err(format!(
            "original-data overlay {} ignored: it has no {OVERLAY_INDEX}. Create one from your own \
             copy with `hero-tools original extract <install> --out <dir>` (see docs/ORIGINAL_DATA.md)",
            dir.display()
        ))
    }
}

/// Seconds since the Unix epoch (0 if the clock is unavailable or before 1970).
pub fn unix_now() -> u64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let secs = web::now_seconds();
        if secs.is_finite() && secs > 0.0 {
            secs as u64
        } else {
            0
        }
    }
}

/// Whether the pointer is known to have left the game (the canvas, or the page lost focus):
/// the browser tells (`hero_web.js`); natively macroquad does not, so this is always `false`
/// there and callers fall back to how long the pointer has rested.
pub fn pointer_left() -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        !web::pointer_inside()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        false
    }
}

/// `true` in the browser build.
pub const fn is_web() -> bool {
    cfg!(target_arch = "wasm32")
}

/// Whether the game may offer a "quit" command (browsers cannot close their tab).
pub const fn can_quit() -> bool {
    !is_web()
}

/// Whether a fullscreen toggle is offered. Browsers only allow fullscreen from inside an input
/// event handler, which the game loop is not, so the web build leaves it to the browser.
pub const fn can_toggle_fullscreen() -> bool {
    !is_web()
}

/// Tell the host page that the first frame is on screen (removes the HTML loading overlay).
/// No-op natively.
pub fn notify_ready() {
    #[cfg(target_arch = "wasm32")]
    web::ready();
}

/// File name of the native crash report inside the user data directory.
pub const CRASH_LOG: &str = "crash.log";

/// Record a panic where the player (and a bug report) can find it. Called from the panic hook
/// installed by `main.rs`.
///
/// * Always logged as an error (stderr natively, the browser console on the web).
/// * Natively also written to [`CRASH_LOG`] in the user data directory, because release builds
///   on Windows have no console window.
/// * On the web the page shows the message over the (now frozen) canvas.
pub fn report_panic(message: &str) {
    macroquad::logging::error!("panic: {}", message);
    #[cfg(not(target_arch = "wasm32"))]
    {
        let Some(dir) = storage::user_data_dir() else {
            return;
        };
        let report = format!(
            "Eiketsuden Reloaded {} crashed at unix time {}\n{}\n",
            env!("CARGO_PKG_VERSION"),
            unix_now(),
            message
        );
        let path = dir.join(CRASH_LOG);
        let written = std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&path, report));
        match written {
            Ok(()) => eprintln!("crash report written to {}", path.display()),
            // Nothing else can be done inside a panic hook; at least say why the file is missing.
            Err(e) => eprintln!("cannot write crash report {}: {e}", path.display()),
        }
    }
    #[cfg(target_arch = "wasm32")]
    web::show_panic(message);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_native_arguments() {
        let o = LaunchOptions::parse_args(args(&["--gallery", "--data", "packs/base"]));
        assert!(o.gallery);
        assert_eq!(o.data_dir, Some(PathBuf::from("packs/base")));
        assert!(o.warnings.is_empty());

        let o = LaunchOptions::parse_args(args(&["--data=/x/y", "--bogus"]));
        assert_eq!(o.data_dir, Some(PathBuf::from("/x/y")));
        assert!(!o.gallery);
        assert_eq!(o.warnings.len(), 1);

        let o = LaunchOptions::parse_args(args(&["--data"]));
        assert_eq!(o.data_dir, None);
        assert_eq!(o.warnings.len(), 1);

        let o = LaunchOptions::parse_args(args(&["--original", "D:/overlay"]));
        assert_eq!(o.original_dir, Some(PathBuf::from("D:/overlay")));
        let o = LaunchOptions::parse_args(args(&["--original=/o", "--data=/d"]));
        assert_eq!(o.original_dir, Some(PathBuf::from("/o")));
        assert_eq!(o.data_dir, Some(PathBuf::from("/d")));
        let o = LaunchOptions::parse_args(args(&["--original"]));
        assert_eq!(o.original_dir, None);
        assert_eq!(o.warnings, vec!["--original needs a directory argument"]);
    }

    #[test]
    fn overlay_dir_precedence_and_validation() {
        let yes = |_: &Path| true;
        assert_eq!(resolve_overlay_dir(None, None, yes), Ok(None));
        assert_eq!(
            resolve_overlay_dir(None, Some(Path::new("")), yes),
            Ok(None)
        );
        assert_eq!(
            resolve_overlay_dir(Some(Path::new("/a")), Some(Path::new("/b")), yes),
            Ok(Some(PathBuf::from("/a")))
        );
        assert_eq!(
            resolve_overlay_dir(None, Some(Path::new("/b")), yes),
            Ok(Some(PathBuf::from("/b")))
        );
        let err = resolve_overlay_dir(Some(Path::new("/install")), None, |_| false).unwrap_err();
        assert!(err.contains("/install"), "{err}");
        assert!(err.contains("hero-tools original extract"), "{err}");
    }

    #[test]
    fn media_paths_try_the_overlay_first() {
        let root = DataRoot::from_dir(Path::new("/games/hero/data/base"), &[]);
        assert_eq!(
            root.media_paths("gfx/a.png"),
            vec!["/games/hero/data/base/gfx/a.png"]
        );
        assert_eq!(root.media_overlay(), None);
        let root = root.with_media_overlay(Path::new("/home/me/overlay"));
        assert_eq!(
            root.media_paths("/gfx/a.png"),
            vec![
                "/home/me/overlay/gfx/a.png",
                "/games/hero/data/base/gfx/a.png"
            ]
        );
        assert_eq!(root.media_overlay(), Some("/home/me/overlay/"));
        // Pack text files are never overlaid.
        assert_eq!(root.path("pack.toml"), "/games/hero/data/base/pack.toml");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_original_pack_id_is_the_converters() {
        assert_eq!(ORIGINAL_PACK_ID, hero_import::pack::PACK_ID);
    }

    #[test]
    fn public_media_paths_prefer_the_packs_below_the_original() {
        // The original mode: the converted pack on top of the base pack.
        let root = DataRoot::from_dir(Path::new("/d/original"), &[]).with_parent_packs(["../base"]);
        assert_eq!(root.public_media_paths("gfx/portraits/a.png"), None);
        let root = root.with_original_layer(Some(0));
        assert_eq!(
            root.public_media_paths("gfx/portraits/a.png").unwrap(),
            vec![
                "/d/original/../base/gfx/portraits/a.png",
                "/d/original/gfx/portraits/a.png"
            ]
        );
        // The usual order is untouched.
        assert_eq!(
            root.media_paths("gfx/portraits/a.png")[0],
            "/d/original/gfx/portraits/a.png"
        );
        // A mod on top of the original: the mod's own file still comes before the original's.
        let modded = DataRoot::from_dir(Path::new("/m"), &[])
            .with_parent_packs(["../d/original", "../d/base"])
            .with_original_layer(Some(1))
            .with_media_overlay(Path::new("/o"));
        assert_eq!(
            modded.public_media_paths("x.png").unwrap(),
            vec![
                "/m/../d/base/x.png",
                "/o/x.png",
                "/m/x.png",
                "/m/../d/original/x.png"
            ]
        );
        // The original at the bottom, or a layer past the chain: nothing to prefer.
        let bottom = DataRoot::from_dir(Path::new("/m"), &[])
            .with_parent_packs(["../d/original"])
            .with_original_layer(Some(1));
        assert_eq!(bottom.public_media_paths("x.png"), None);
        let past = DataRoot::from_dir(Path::new("/m"), &[]).with_original_layer(Some(3));
        assert_eq!(past.public_media_paths("x.png"), None);
        // The top pack alone forgets the chain position with the chain.
        assert_eq!(root.top_pack().public_media_paths("x.png"), None);
    }

    #[test]
    fn media_paths_search_the_top_pack_then_its_parents() {
        let root = DataRoot::from_dir(Path::new("/mods/ext"), &[])
            .with_parent_packs(["../base", "../../core/"]);
        assert_eq!(root.parent_packs(), ["../base", "../../core"]);
        assert_eq!(
            root.media_paths("sfx/x.wav"),
            vec![
                "/mods/ext/sfx/x.wav",
                "/mods/ext/../base/sfx/x.wav",
                "/mods/ext/../../core/sfx/x.wav"
            ]
        );
        // The overlay still comes first.
        let root = root.with_media_overlay(Path::new("/o"));
        assert_eq!(
            root.media_paths("gfx/a.png")[..2],
            ["/o/gfx/a.png", "/mods/ext/gfx/a.png"]
        );
        // The top pack alone: exact paths, overlay kept.
        let top = root.top_pack();
        assert!(top.parent_packs().is_empty());
        assert_eq!(top.media_overlay(), Some("/o/"));
        assert_eq!(
            top.path("../base/rules/game.toml"),
            "/mods/ext/../base/rules/game.toml"
        );

        // The web build fetches relative URLs, which the browser resolves.
        let web = DataRoot::from_prefix("data/ext").with_parent_packs(["../base"]);
        assert_eq!(
            web.media_paths("bgm/title.ogg"),
            vec!["data/ext/bgm/title.ogg", "data/ext/../base/bgm/title.ogg"]
        );
    }

    #[test]
    fn paths_are_relative_to_the_top_pack() {
        let tmp = std::env::temp_dir().join(format!("hero-game-layers-{}", std::process::id()));
        let top = tmp.join("ext");
        let root = DataRoot::from_dir(&top, &[]).with_parent_packs(["../base"]);
        let top_prefix = dir_prefix(&top);
        // A parent's file is named through the chain's own path, never searched for.
        assert_eq!(root.path("/pack.toml"), format!("{top_prefix}pack.toml"));
        assert_eq!(
            root.path("../base/rules/game.toml"),
            format!("{top_prefix}../base/rules/game.toml")
        );
    }

    #[test]
    fn parses_url_hash() {
        assert!(LaunchOptions::parse_hash("#gallery").gallery);
        assert!(LaunchOptions::parse_hash("gallery").gallery);
        assert!(!LaunchOptions::parse_hash("").gallery);
        let o = LaunchOptions::parse_hash("#foo&gallery");
        assert!(o.gallery);
        assert_eq!(o.warnings.len(), 1);
    }

    #[test]
    fn explicit_data_dir_wins_even_without_pack() {
        let (dir, cands) = resolve_data_dir(
            Some(Path::new("/mods/x")),
            Some(Path::new("/env/y")),
            Some(Path::new("/opt/game")),
            |_| false,
        );
        assert_eq!(dir, PathBuf::from("/mods/x"));
        assert_eq!(cands, vec![PathBuf::from("/mods/x")]);

        let (dir, _) = resolve_data_dir(None, Some(Path::new("/env/y")), None, |_| false);
        assert_eq!(dir, PathBuf::from("/env/y"));
    }

    #[test]
    fn implicit_data_dir_prefers_exe_dir_then_cwd() {
        let exe = Path::new("/opt/game");
        let (dir, cands) = resolve_data_dir(None, None, Some(exe), |p| p.starts_with("/opt"));
        assert_eq!(dir, exe.join("data").join("base"));
        assert_eq!(cands.len(), 2);

        let (dir, _) = resolve_data_dir(None, Some(Path::new("")), Some(exe), |p| {
            p == Path::new("data/base")
        });
        assert_eq!(dir, PathBuf::from("data").join("base"));

        // Nothing found: fall back to ./data/base so the error screen has a concrete path.
        let (dir, cands) = resolve_data_dir(None, None, Some(exe), |_| false);
        assert_eq!(dir, PathBuf::from("data").join("base"));
        assert_eq!(cands.len(), 2);
    }

    #[test]
    fn data_root_paths() {
        let web = DataRoot::from_prefix("data/base");
        assert_eq!(web.path("pack.toml"), "data/base/pack.toml");
        assert_eq!(web.path("/fonts/a.ttf"), "data/base/fonts/a.ttf");

        let native = DataRoot::from_dir(Path::new("/games/hero/data/base"), &[]);
        assert_eq!(
            native.path("rules/game.toml"),
            "/games/hero/data/base/rules/game.toml"
        );
        let trailing = DataRoot::from_dir(Path::new("base/"), &[]);
        assert_eq!(trailing.path("pack.toml"), "base/pack.toml");
    }

    #[test]
    fn memory_pack_stands_next_to_the_base_pack() {
        let base = DataRoot::from_dir(Path::new(r"D:\games\hero\data\base"), &[]);
        let (root, extends) = base.memory_pack("original", "원작").unwrap();
        assert_eq!(extends, "../base");
        assert_eq!(root.display(), "원작");
        assert_eq!(
            root.path("pack.toml"),
            "D:/games/hero/data/original/pack.toml"
        );
        let root = root.with_parent_packs([extends]);
        assert_eq!(
            root.media_paths("gfx/ui/title.png"),
            [
                "D:/games/hero/data/original/gfx/ui/title.png",
                "D:/games/hero/data/original/../base/gfx/ui/title.png"
            ]
        );

        let relative = DataRoot::from_dir(Path::new("data/base"), &[]);
        let (root, extends) = relative.memory_pack("original", "원작").unwrap();
        assert_eq!(
            (root.path("a"), extends.as_str()),
            ("data/original/a".to_string(), "../base")
        );
        let bare = DataRoot::from_dir(Path::new("base"), &[]);
        assert_eq!(
            bare.memory_pack("original", "원작").unwrap().0.path("a"),
            "original/a"
        );

        for rootless in ["/", "D:/", ".."] {
            let r = DataRoot::from_dir(Path::new(rootless), &[]);
            assert!(r.memory_pack("original", "원작").is_none(), "{rootless}");
        }
    }
}
