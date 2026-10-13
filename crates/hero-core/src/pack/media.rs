//! Native media check behind [`Pack::missing_media`]; the file conventions are those of
//! `docs/ASSETS.md`.

use super::validate::is_media_key;
use super::{Issue, Pack, Severity};
use crate::battledef::{EventAction, MapDef};
use crate::map::BattleMap;
use crate::media_index::{
    self, FxFile, TilesetFile, UnitsFile, ANY_CLASS, DEFAULT_TILE, FX_FILE as FX_TOML,
    TILESET_FILE as TILES_TOML, UNITS_FILE as UNITS_TOML,
};
use crate::script::{Cmd, DUEL_TERRAIN};
use serde::de::DeserializeOwned;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Unit sheet colours; one sheet per sprite key and side.
const SIDES: [&str; 3] = ["player", "ally", "enemy"];
const ICONS_TOML: &str = "gfx/ui/icons.toml";
/// Largest texture side many mobile GPUs load (WebGL 2 guarantees 2048); a bigger picture comes
/// out black there. The tileset-drawn map cache is split into pieces instead.
const MOBILE_TEXTURE: u32 = 4096;
/// Pixels of cache a map drawn from the tileset may take (the battle caches its static layers
/// with one tile of padding, 4 bytes a pixel): 4096² is 64 MB.
const CACHE_BUDGET: u128 = 4096 * 4096;
const UNKNOWN_PORTRAIT: &str = "gfx/portraits/_unknown.png";

impl Pack {
    /// Check that every media file the pack refers to exists below `root` (the top pack
    /// directory) — for a layered pack in the directory of any of its [`Pack::layers`], looked
    /// up top pack first like the game does, so a media index file (`units.toml`,
    /// `terrain.toml`, `fx.toml`, `icons.toml`) is read from the first pack that has one.
    ///
    /// The index files are read with the game's schema ([`crate::media_index`]): a missing field
    /// or a value of the wrong type, which the game could not read, is an error.
    ///
    /// Checked: unit sheets `gfx/units/<sprite>_<side>.png`, their `units.toml` entries and that
    /// each sheet is 4 × 6 of the entry's `frame`,
    /// officer and `@show` portraits `gfx/portraits/<key>.png` (warnings: `_unknown.png` is
    /// shown instead, which must exist), `bgm/<key>.ogg` of battles and dramas, `gfx/bg/<key>.png`
    /// and `sfx/<key>.(ogg|wav)` of dramas, a `gfx/tiles/terrain.toml` tile for every terrain,
    /// `gfx/fx/fx.toml` entries and strips for strategy effects, `gfx/ui/icons.toml` keys
    /// of item icons (warnings), and the picture layers `gfx/maps/<key>.png` of maps, whose
    /// size must be the map's size in tiles times the tileset's `tile_size`; and (warnings) that
    /// the fonts hold every Hanja of the pack's text.
    pub fn missing_media(&self, root: &Path) -> Vec<Issue> {
        let mut dirs: Vec<PathBuf> = self.layers.iter().map(|l| root.join(&l.dir)).collect();
        if dirs.is_empty() {
            dirs.push(root.to_path_buf());
        }
        let mut m = MediaCheck {
            dirs,
            issues: Vec::new(),
            reported: BTreeSet::new(),
            tile_size: DEFAULT_TILE,
        };
        m.units(self);
        m.portraits(self);
        m.audio_and_backgrounds(self);
        m.tiles(self);
        m.map_pictures(self);
        m.effects(self);
        m.icons(self);
        m.battle_frame(self);
        m.hanja();
        m.issues
    }
}

struct MediaCheck {
    /// Media directories in lookup order: the top pack, then the packs it extends.
    dirs: Vec<PathBuf>,
    issues: Vec<Issue>,
    /// Missing files already reported (each is reported once, at its first user).
    reported: BTreeSet<String>,
    /// `tile_size` of the tileset in use (read by [`MediaCheck::tiles`]).
    tile_size: u32,
}

impl MediaCheck {
    /// The file `rel` of the first pack directory that has it.
    fn find(&self, rel: &str) -> Option<PathBuf> {
        self.dirs
            .iter()
            .map(|dir| dir.join(rel))
            .find(|path| path.is_file())
    }

    fn exists(&self, rel: &str) -> bool {
        self.find(rel).is_some()
    }

    fn push(&mut self, severity: Severity, context: &str, msg: String) {
        self.issues.push(Issue {
            severity,
            context: context.to_string(),
            msg,
        });
    }

    /// Report `rel` once if it does not exist.
    fn require(&mut self, severity: Severity, context: &str, rel: &str, what: &str) {
        if !self.exists(rel) && self.reported.insert(rel.to_string()) {
            self.push(severity, context, format!("missing {rel} ({what})"));
        }
    }

    /// Parse a media index file as `T` (each is read once); reports and returns `None` when it
    /// is missing or does not fit the schema.
    fn index<T: DeserializeOwned>(&mut self, rel: &str, context: &str) -> Option<T> {
        let Some(path) = self.find(rel) else {
            self.push(Severity::Error, context, format!("missing {rel}"));
            return None;
        };
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                let msg = format!("cannot read {}: {e}", path.display());
                self.push(Severity::Error, context, msg);
                return None;
            }
        };
        match media_index::parse::<T>(&text) {
            Ok(t) => Some(t),
            Err(e) => {
                self.push(Severity::Error, rel, e);
                None
            }
        }
    }

    fn units(&mut self, pack: &Pack) {
        if pack.classes.is_empty() {
            return;
        }
        let index: Option<UnitsFile> = self.index(UNITS_TOML, "unit sprites");
        let mut keys_seen = BTreeSet::new();
        for class in pack.classes.values() {
            let key = class.sprite.as_str();
            if keys_seen.insert(key) {
                self.sprite_sheets(&format!("class {}", class.id), key, index.as_ref());
            }
        }
        let Some(index) = &index else {
            return;
        };
        // Officers' own sprites: known officers and class sprite keys, and sheets like a
        // class's.
        let class_sprites: BTreeSet<&str> =
            pack.classes.values().map(|c| c.sprite.as_str()).collect();
        for (officer, by_class) in &index.officers {
            let ctx = format!("{UNITS_TOML} officer {officer}");
            if pack.officer(officer).is_none() {
                self.push(
                    Severity::Warning,
                    &ctx,
                    "names no officer of the pack: never drawn".to_string(),
                );
            }
            for (class_sprite, key) in by_class {
                if class_sprite != ANY_CLASS && !class_sprites.contains(class_sprite.as_str()) {
                    self.push(
                        Severity::Warning,
                        &ctx,
                        format!(
                            "`{class_sprite}` is no class's sprite key (nor `{ANY_CLASS}`): never drawn"
                        ),
                    );
                }
                if keys_seen.insert(key.as_str()) {
                    self.sprite_sheets(&ctx, key, Some(index));
                }
            }
        }
        for (status, key) in &index.statuses {
            let ctx = format!("{UNITS_TOML} status {status}");
            if !crate::data::StatusKind::ALL
                .iter()
                .any(|s| s.id() == status)
            {
                self.push(
                    Severity::Warning,
                    &ctx,
                    format!(
                        "names no status of the game ({}): never drawn",
                        crate::data::StatusKind::ALL
                            .iter()
                            .map(|s| format!("`{}`", s.id()))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                );
            }
            if keys_seen.insert(key.as_str()) {
                self.sprite_sheets(&ctx, key, Some(index));
            }
        }
    }

    /// The three side sheets of sprite `key` and its `[sprites.key]` entry, sized alike.
    fn sprite_sheets(&mut self, ctx: &str, key: &str, index: Option<&UnitsFile>) {
        let def = index.and_then(|f| f.sprites.get(key));
        for side in SIDES {
            let rel = format!("gfx/units/{key}_{side}.png");
            self.require(Severity::Error, ctx, &rel, "unit sprite sheet");
            // The battle cuts frames by `frame`, the camp by the sheet size (4 columns × 6
            // rows): both must agree. (Computed wide, so a huge `frame` cannot wrap around.)
            if let (Some(def), Some(path)) = (def, self.find(&rel)) {
                let want = (4 * u64::from(def.frame[0]), 6 * u64::from(def.frame[1]));
                match png_size(&path) {
                    Ok((w, h)) if (u64::from(w), u64::from(h)) == want => {}
                    Ok((w, h)) => self.push(
                        Severity::Error,
                        ctx,
                        format!(
                            "{rel} is {w}×{h} pixels; with frame = [{}, {}] in {UNITS_TOML} a sheet of 4 × 6 frames is {}×{}",
                            def.frame[0], def.frame[1], want.0, want.1
                        ),
                    ),
                    Err(e) => self.push(
                        Severity::Error,
                        ctx,
                        format!("{}: not a readable PNG: {e}", path.display()),
                    ),
                }
            }
        }
        if index.is_some() && def.is_none() {
            self.push(
                Severity::Error,
                ctx,
                format!("{UNITS_TOML} has no [sprites.{key}] entry"),
            );
        }
    }

    fn portraits(&mut self, pack: &Pack) {
        self.require(
            Severity::Error,
            "portraits",
            UNKNOWN_PORTRAIT,
            "fallback portrait",
        );
        for officer in pack.officers.values() {
            let rel = format!("gfx/portraits/{}.png", officer.portrait_key());
            self.require(
                Severity::Warning,
                &format!("officer {}", officer.id),
                &rel,
                "portrait; `_unknown` is shown instead",
            );
        }
        for scene in pack.scenes.values() {
            for cmd in &scene.cmds {
                if let Cmd::Show { who, .. } = cmd {
                    let key = pack
                        .speaker_officer(who)
                        .map_or(who.as_str(), |o| o.portrait_key());
                    let rel = format!("gfx/portraits/{key}.png");
                    self.require(
                        Severity::Warning,
                        &format!("scene {}", scene.id),
                        &rel,
                        "portrait; `_unknown` is shown instead",
                    );
                }
            }
        }
    }

    /// Music `key`: `bgm/<key>.ogg` or `bgm/<key>.wav`.
    fn require_music(&mut self, context: &str, key: &str) {
        let ogg = format!("bgm/{key}.ogg");
        if !self.exists(&format!("bgm/{key}.wav")) {
            self.require(Severity::Error, context, &ogg, "music");
        }
    }

    fn audio_and_backgrounds(&mut self, pack: &Pack) {
        for b in pack.battles.values() {
            for key in [&b.bgm, &b.bgm_enemy].into_iter().flatten() {
                self.require_music(&format!("battle {}", b.id), key);
            }
        }
        for scene in pack.scenes.values() {
            let ctx = format!("scene {}", scene.id);
            for cmd in &scene.cmds {
                match cmd {
                    Cmd::Bgm(Some(key)) => self.require_music(&ctx, key),
                    Cmd::Bg(Some(key)) => self.require(
                        Severity::Error,
                        &ctx,
                        &format!("gfx/bg/{key}.png"),
                        "background",
                    ),
                    Cmd::Picture(Some(key)) => self.require(
                        Severity::Error,
                        &ctx,
                        &format!("gfx/pictures/{key}.png"),
                        "picture",
                    ),
                    Cmd::Duel { left, right, bg } => {
                        // The terrain's background is looked up in the battle (and may be
                        // missing: a plain stage); a picture named after the word is never shown.
                        let hidden = format!("gfx/duel/{DUEL_TERRAIN}.png");
                        if bg.as_deref() == Some(DUEL_TERRAIN)
                            && self.exists(&hidden)
                            && self.reported.insert(hidden.clone())
                        {
                            self.push(
                                Severity::Warning,
                                &ctx,
                                format!(
                                    "{hidden} is never shown: `@duel ... {DUEL_TERRAIN}` is the \
                                     background of the terrain under the fighters"
                                ),
                            );
                        }
                        if let Some(bg) = bg.as_ref().filter(|b| *b != DUEL_TERRAIN) {
                            self.require(
                                Severity::Error,
                                &ctx,
                                &format!("gfx/duel/{bg}.png"),
                                "duel background",
                            );
                        }
                        for (officer, side) in [(left, "left"), (right, "right")] {
                            let own = format!("gfx/duel/{officer}.png");
                            let fallback = format!("gfx/duel/{side}.png");
                            if !self.exists(&own)
                                && !self.exists(&fallback)
                                && self.reported.insert(own.clone())
                            {
                                self.push(
                                    Severity::Warning,
                                    &ctx,
                                    format!(
                                        "missing {own} or {fallback} (duel rider); the fighter is not drawn"
                                    ),
                                );
                            }
                        }
                    }
                    Cmd::Sfx(key) => {
                        let ogg = format!("sfx/{key}.ogg");
                        let wav = format!("sfx/{key}.wav");
                        if !self.exists(&ogg)
                            && !self.exists(&wav)
                            && self.reported.insert(ogg.clone())
                        {
                            self.push(
                                Severity::Error,
                                &ctx,
                                format!("missing {ogg} or {wav} (sound effect)"),
                            );
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    fn tiles(&mut self, pack: &Pack) {
        if pack.terrain.is_empty() {
            return;
        }
        let Some(index) = self.index::<TilesetFile>(TILES_TOML, "terrain tiles") else {
            return;
        };
        if index.tile_size > 0 {
            self.tile_size = index.tile_size;
        } else {
            self.push(
                Severity::Error,
                TILES_TOML,
                "tile_size 0 must be a positive whole number of pixels".into(),
            );
        }
        let rel = format!("gfx/tiles/{}", index.image);
        self.require(Severity::Error, TILES_TOML, &rel, "terrain atlas");
        // A layer the game cannot draw is left out of the map.
        for problem in index.layers().1 {
            self.push(Severity::Error, TILES_TOML, problem);
        }
        for t in &pack.terrain {
            let key = t.tile_key();
            if !index.tiles.contains_key(key) {
                self.push(
                    Severity::Error,
                    &format!("terrain {}", t.id),
                    format!("{TILES_TOML} has no [tiles.{key}] entry"),
                );
            }
        }
    }

    /// Picture layers of the map files and of battles that write their map themselves. A
    /// picture of another size would be drawn misaligned with the rules grid, so the game
    /// falls back to the tileset for it; here that is an error.
    fn map_pictures(&mut self, pack: &Pack) {
        let mut users: Vec<(String, &MapDef)> = Vec::new();
        let library: Vec<(String, MapDef)> = pack
            .maps
            .values()
            .map(|m| (format!("map {}", m.id), m.to_def()))
            .collect();
        for (ctx, def) in &library {
            users.push((ctx.clone(), def));
        }
        for b in pack.battles.values() {
            if b.map.use_map.is_none() {
                users.push((format!("battle {}", b.id), &b.map));
            }
        }
        for (ctx, def) in users {
            let Ok(map) = BattleMap::parse(&def.rows, &def.legend, &pack.terrain) else {
                continue;
            };
            if def.image.is_none() {
                // Drawn from the tileset into a cache the size of the map (plus padding).
                // In u128: a huge tile_size times a big map overflows u64.
                let tile = u128::from(self.tile_size);
                let area = (map.width as u128 + 2) * tile * (map.height as u128 + 2) * tile;
                if area > CACHE_BUDGET {
                    self.push(
                        Severity::Warning,
                        &ctx,
                        format!(
                            "drawn from the tileset, this {}×{} map with {tile}-pixel tiles takes {} MB of texture memory in battle (more than {} MB): use a smaller tile_size or split the map",
                            map.width,
                            map.height,
                            (area * 4).div_ceil(1024 * 1024),
                            CACHE_BUDGET * 4 / (1024 * 1024)
                        ),
                    );
                }
                continue;
            }
            // Bad keys are reported by `Pack::validate`.
            let Some(key) = def.image.as_deref().filter(|k| is_media_key(k)) else {
                continue;
            };
            let rel = format!("gfx/maps/{key}.png");
            let Some(path) = self.find(&rel) else {
                self.require(Severity::Error, &ctx, &rel, "map picture");
                continue;
            };
            let tile = self.tile_size;
            // Computed wide: an absurd `tile_size` must not wrap around to a match.
            let want = (
                map.width as u64 * u64::from(tile),
                map.height as u64 * u64::from(tile),
            );
            match png_size(&path) {
                Ok((w, h)) if (u64::from(w), u64::from(h)) == want => {
                    if want.0.max(want.1) > u64::from(MOBILE_TEXTURE) {
                        self.push(
                            Severity::Warning,
                            &ctx,
                            format!(
                                "{rel} is {}×{} pixels: many phones cannot load a texture over {MOBILE_TEXTURE} pixels a side and draw it black (use smaller tiles, or draw the map from the tileset)",
                                want.0, want.1
                            ),
                        );
                    }
                }
                Ok((w, h)) => self.push(
                    Severity::Error,
                    &ctx,
                    format!(
                        "{rel} is {w}×{h} pixels; the map needs {}×{} ({}×{} tiles of {tile} px, the tileset's tile_size)",
                        want.0, want.1, map.width, map.height
                    ),
                ),
                Err(e) => self.push(
                    Severity::Error,
                    &ctx,
                    format!("{}: not a readable PNG: {e}", path.display()),
                ),
            }
        }
        // Tile pictures of `set_terrain` events: one tile each.
        for b in pack.battles.values() {
            for (i, e) in b.events.iter().enumerate() {
                for a in e.all_actions() {
                    let EventAction::SetTerrain {
                        image: Some(key), ..
                    } = a
                    else {
                        continue;
                    };
                    if !is_media_key(key) {
                        continue; // reported by `Pack::validate`
                    }
                    let ctx = format!("battle {} event {i}", b.id);
                    let rel = format!("gfx/maps/{key}.png");
                    let Some(path) = self.find(&rel) else {
                        self.require(Severity::Warning, &ctx, &rel, "tile picture");
                        continue;
                    };
                    let tile = self.tile_size;
                    match png_size(&path) {
                        Ok(size) if size == (tile, tile) => {}
                        Ok((w, h)) => self.push(
                            Severity::Warning,
                            &ctx,
                            format!(
                                "{rel} is {w}×{h} pixels; a tile picture is one tile ({tile}×{tile}, the tileset's tile_size)"
                            ),
                        ),
                        Err(e) => self.push(
                            Severity::Error,
                            &ctx,
                            format!("{}: not a readable PNG: {e}", path.display()),
                        ),
                    }
                }
            }
        }
    }

    fn effects(&mut self, pack: &Pack) {
        let users: Vec<_> = pack
            .strategies
            .values()
            .filter(|s| !s.fx.is_empty())
            .collect();
        if users.is_empty() {
            return;
        }
        let Some(index) = self.index::<FxFile>(FX_TOML, "strategy effects") else {
            return;
        };
        for s in users {
            let ctx = format!("strategy {}", s.id);
            if !index.fx.contains_key(&s.fx) {
                self.push(
                    Severity::Error,
                    &ctx,
                    format!("{FX_TOML} has no [fx.{}] entry", s.fx),
                );
            }
            self.require(
                Severity::Error,
                &ctx,
                &format!("gfx/fx/{}.png", s.fx),
                "effect strip",
            );
        }
    }

    /// The frames' pictures: the size of the canvas (the game fills the frame plainly
    /// otherwise).
    fn battle_frame(&mut self, pack: &Pack) {
        let presentation = &pack.manifest.presentation;
        let frames = [
            (
                "presentation.battle_frame",
                presentation.battle_frame.as_ref().map(|f| f.image.clone()),
            ),
            (
                "presentation.camp_frame",
                presentation.camp_frame.as_ref().map(|f| f.image.clone()),
            ),
        ];
        for (ctx, image) in frames {
            if let Some(image) = image {
                self.frame_picture(ctx, &image, presentation.canvas);
            }
        }
        if let Some(f) = &presentation.status_frame {
            self.frame_picture("presentation.status_frame", &f.image, f.size);
        }
    }

    fn frame_picture(&mut self, ctx: &str, image: &str, canvas: [u32; 2]) {
        let rel = format!("gfx/{image}.png");
        let Some(path) = self.find(&rel) else {
            self.require(Severity::Warning, ctx, &rel, "frame picture");
            return;
        };
        let [cw, ch] = canvas;
        match png_size(&path) {
            Ok((w, h)) if (w, h) == (cw, ch) => {}
            Ok((w, h)) => self.push(
                Severity::Warning,
                ctx,
                format!(
                    "{rel} is {w}×{h} pixels, the frame {cw}×{ch}: the frame is filled plainly"
                ),
            ),
            Err(e) => self.push(
                Severity::Warning,
                ctx,
                format!("{}: not a readable PNG: {e}", path.display()),
            ),
        }
    }

    fn icons(&mut self, pack: &Pack) {
        let users: Vec<_> = pack.items.values().filter(|i| !i.icon.is_empty()).collect();
        if users.is_empty() {
            return;
        }
        if !self.exists(ICONS_TOML) {
            self.push(
                Severity::Warning,
                "item icons",
                format!("missing {ICONS_TOML}; items are shown without icons"),
            );
            return;
        }
        let Some(index) = self.index::<toml::Table>(ICONS_TOML, "item icons") else {
            return;
        };
        let icons = index.get("icons").and_then(|v| v.as_table());
        for item in users {
            if !icons.is_some_and(|icons| icons.contains_key(&item.icon)) {
                self.push(
                    Severity::Warning,
                    &format!("item {}", item.id),
                    format!("{ICONS_TOML} has no icon `{}`", item.icon),
                );
            }
        }
    }
}

use super::FONT_FILES;

/// A CJK ideograph (extension A, unified, compatibility), as `tools/assets/build_fonts.py`
/// collects them for the base pack's fonts.
fn is_hanja(c: char) -> bool {
    matches!(c, '\u{3400}'..='\u{4DBF}' | '\u{4E00}'..='\u{9FFF}' | '\u{F900}'..='\u{FAFF}')
}

/// Every text file of the pack directory `dir` whose characters are shown: rules, battles,
/// maps and the other `.toml` files, dramas and `credits.txt` (licence texts are not shown).
fn text_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        // Not through symbolic links, which could loop.
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            text_files(&path, out);
        } else {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let shown = name == "credits.txt"
                || matches!(
                    path.extension().and_then(|e| e.to_str()),
                    Some("toml" | "drama")
                );
            if shown {
                out.push(path);
            }
        }
    }
}

impl MediaCheck {
    /// Hanja of the pack's text files (every layer) that a font lacks: the game draws them as
    /// blanks. The base pack's fonts hold Galmuri's own Hanja plus the ones its text needs.
    fn hanja(&mut self) {
        // Without a font the game falls back to a Latin-only one: every Korean text is blank.
        for font in FONT_FILES {
            self.require(
                Severity::Error,
                "fonts",
                font,
                "font; the game falls back to one without Hangul or Hanja",
            );
        }
        let mut files = Vec::new();
        for dir in &self.dirs {
            text_files(dir, &mut files);
        }
        let mut needed: BTreeSet<char> = BTreeSet::new();
        for f in files {
            if let Ok(text) = std::fs::read_to_string(&f) {
                needed.extend(text.chars().filter(|&c| is_hanja(c)));
            }
        }
        if needed.is_empty() {
            return;
        }
        for font in FONT_FILES {
            let Some(path) = self.find(font) else {
                continue; // reported above
            };
            let coverage = match std::fs::read(&path)
                .map_err(|e| e.to_string())
                .and_then(|b| super::cmap::coverage(&b))
            {
                Ok(c) => c,
                Err(e) => {
                    self.push(Severity::Warning, font, format!("{}: {e}", path.display()));
                    continue;
                }
            };
            let missing: Vec<char> = needed
                .iter()
                .copied()
                .filter(|&c| !coverage.contains(c))
                .collect();
            if !missing.is_empty() {
                let shown: String = missing.iter().take(40).collect();
                let more = if missing.len() > 40 {
                    format!(" and {} more", missing.len() - 40)
                } else {
                    String::new()
                };
                self.push(
                    Severity::Warning,
                    font,
                    format!(
                        "lacks {} Hanja the pack's text uses, drawn as blanks: {shown}{more} (add them to the font; the base pack's fonts are built by tools/assets/build_fonts.py)",
                        missing.len()
                    ),
                );
            }
        }
    }
}

/// Width and height of the PNG image at `path`, from its `IHDR` chunk (the first chunk of
/// every PNG), without decoding the image.
fn png_size(path: &Path) -> Result<(u32, u32), String> {
    use std::io::Read;
    const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];
    let mut head = [0u8; 24];
    std::fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut head))
        .map_err(|e| e.to_string())?;
    if head[..8] != SIGNATURE || &head[12..16] != b"IHDR" {
        return Err("no PNG signature and IHDR chunk".into());
    }
    let be = |at: usize| u32::from_be_bytes([head[at], head[at + 1], head[at + 2], head[at + 3]]);
    Ok((be(16), be(20)))
}
