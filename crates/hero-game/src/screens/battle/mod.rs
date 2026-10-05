//! Battle screen: the tactical map of a campaign battle on top of [`hero_core::battle`].
//!
//! * **Entry points** — [`BattleScreen::start`] builds the battle of a campaign `Battle` node
//!   (title card → objective window → `begin`, whose intro scene plays as a drama overlay);
//!   [`BattleScreen::resume`] continues the battle of a mid-battle save. Both are wired in
//!   `crate::flow`. When the battle is over the screen returns
//!   `Transition::Flow(Flow::BattleEnded(state))`.
//! * **State** — the [`BattleState`] is the single source of truth; after every applied action
//!   it is copied into `ctx.session.battle`, so 중단 기록 saves exactly what is on screen.
//! * **Animation** — actions return events; [`anim`] turns them into beats that move
//!   [`anim::UnitView`]s (what is drawn) until they catch up with the state.
//! * **Player phase** — [`player::PlayerUi`] is the command state machine (select, move,
//!   공격/책략/도구/대기, undo of an unconfirmed move); this module maps keyboard, mouse and
//!   touch onto it and draws the menus and forecasts.
//! * **AI phases** — one unit at a time from `next_ai_unit` / `ai_actions`, animated like the
//!   player's actions; holding confirm fast-forwards.
//! * **Quick save** — F5 saves at any moment ([`crate::quicksave`]): the state is already past
//!   whatever is animating, so it is saved as it is, together with the drama scenes the
//!   animation had queued but not started; loading shows those scenes first and then goes on
//!   from the state. Before the objective window is dismissed (the battle has not begun) there
//!   is nothing to save but the campaign: loading starts the battle from the top.
//!
//! * **Presentation** — the map is drawn with the tile size of the pack's tileset
//!   (`gfx/tiles/terrain.toml` `tile_size`, 16 pixels without one), from the map's picture layer
//!   (`gfx/maps/<image>.png`) when it has one, and every window is laid out relative to the
//!   canvas size (`[presentation] canvas` of `pack.toml`).
//! * **View options beyond the original** (`docs/DECISIONS.md` D25, off by default, read from
//!   the settings every frame): "위험 범위" tints every tile an enemy could attack next phase
//!   while the player browses the map ([`player::danger_tiles`], recomputed only when
//!   [`player::danger_key`] changes); "전투 연출 · 강화" shows HP damage as red `-123` numbers and
//!   shakes the map and its units, not the frame, on heavy or defeating hits
//!   ([`anim::Scene::shake_offset`] through [`camera::Camera::shake`]).
//!
//! Controls: arrows/WASD move the cursor, Z/Enter/Space confirm, X/Esc/right click cancel,
//! Tab/E and Q cycle through units that can still act, mouse at the screen edge / right-drag /
//! touch drag / wheel scroll the map, and holding confirm speeds animations up. Touch has no
//! cancel: a tap outside a menu or window steps back, and so does a second tap on a tile that
//! is not a target in the attack / strategy / item target modes.

mod anim;
mod camera;
mod draw;
mod hud;
mod player;
mod sprites;
#[cfg(test)]
mod testutil;
pub(crate) mod text;
mod tileset;

use crate::app::{Ctx, Enter, Screen, Transition};
use crate::assets::{AssetState, FirstOf};
use crate::audio::{bgm, sfx};
use crate::flow::Flow;
use crate::gfx::{draw_placeholder, fill_rect, Align, FontId, TextStyle};
use crate::quicksave::ResumePoint;
use crate::screens::drama::DramaScreen;
use crate::screens::error::ErrorScreen;
use crate::screens::saveload::SaveLoadScreen;
use crate::screens::settings::SettingsScreen;
use crate::ui::dialog::{ConfirmDialog, ConfirmEvent};
use crate::ui::menu::{Menu, MenuEvent, MenuItem};
use crate::ui::theme;
use crate::ui::window::{draw_highlight, draw_icon, draw_window, draw_window_ex, WindowStyle};
use anim::{Cue, EventPlayer, Scene};
use camera::{edge_direction, Camera, EDGE_PAN_SPEED};
use hero_core::battle::{Action, BattleEvent, BattleState, MapImage, Outcome, UnitId};
use hero_core::battledef::{EventAction, Side};
use hero_core::geom::Pos;
use hero_core::pack::{BattleFrame, Pack};
use hero_core::save::SceneResume;
use macroquad::prelude::*;
use player::{Command, Mode, PlayerUi, Request};
use sprites::{FxDef, UnitsFile};
use std::collections::{BTreeMap, VecDeque};
use std::rc::Rc;
use tileset::{MapRenderer, Tileset, DEFAULT_TILE};

/// Screen area of the map on a `canvas` sized canvas: the battle frame's map area, otherwise
/// everything below the top bar.
fn viewport(canvas: Vec2, frame: Option<&BattleFrame>) -> Rect {
    match frame {
        Some(f) => frame_rect(f.map),
        None => Rect::new(0.0, hud::TOP_BAR_H, canvas.x, canvas.y - hud::TOP_BAR_H),
    }
}

/// Left edge of a `w` wide window centred in the battle frame's `info` column: a window wider
/// than the column stays off the map (`map`) as far as the canvas allows.
fn column_x(info: Rect, w: f32, map: Rect, canvas: Vec2) -> f32 {
    (info.x + (info.w - w) / 2.0)
        .max(map.right())
        .min(canvas.x - w)
        .round()
}

/// An `[x, y, width, height]` area of a battle frame.
fn frame_rect([x, y, w, h]: [u32; 4]) -> Rect {
    Rect::new(x as f32, y as f32, w as f32, h as f32)
}

/// A button of the battle frame (`menu`, `allies`, `enemies`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FrameButton {
    Menu,
    Allies,
    Enemies,
}

/// The frame's buttons with their areas.
fn frame_buttons(f: &BattleFrame) -> impl Iterator<Item = (FrameButton, Rect)> {
    [
        (FrameButton::Menu, f.menu),
        (FrameButton::Allies, f.allies),
        (FrameButton::Enemies, f.enemies),
    ]
    .into_iter()
    .filter_map(|(b, a)| a.map(|a| (b, frame_rect(a))))
}

/// The frame's button at `p`, if any.
fn frame_button_at(f: &BattleFrame, p: Vec2) -> Option<FrameButton> {
    frame_buttons(f)
        .find(|(_, r)| r.contains(p))
        .map(|(b, _)| b)
}
/// Seconds the title card stays up.
const TITLE_SECONDS: f32 = 2.6;
/// Pause between AI units (seconds at normal speed).
const AI_PAUSE: f32 = 0.3;
/// Speed factor while confirm is held.
const FAST_FORWARD: f32 = 3.0;
/// Seconds after the last player unit acted before the phase ends by itself.
const AUTO_END_DELAY: f32 = 0.4;

/// Metadata files of the battle art, loaded when the screen opens.
#[derive(Default)]
struct Meta {
    tileset_req: Option<FirstOf>,
    units_req: Option<FirstOf>,
    fx_req: Option<FirstOf>,
    tileset: Option<Tileset>,
    /// Texture key of the map's picture layer (`maps/<image>`) until the map is built from it,
    /// or given up for the tileset.
    picture: Option<String>,
    units: UnitsFile,
    fx: BTreeMap<String, FxDef>,
    /// Media key per effect (`fx/<key>`), so drawing does not format strings.
    fx_textures: BTreeMap<String, String>,
}

impl Meta {
    fn units_loaded(&self) -> bool {
        self.units_req.is_none()
    }
}

/// Read a finished request as UTF-8 text.
fn request_text(req: &mut Option<FirstOf>) -> Option<Result<String, String>> {
    let result = req.as_mut()?.poll()?;
    *req = None;
    Some(result.and_then(|b| String::from_utf8(b).map_err(|e| e.to_string())))
}

enum Stage {
    Title {
        age: f32,
    },
    Objective,
    Battle,
    Result {
        lines: Vec<(String, Vec<(String, Color)>)>,
        title: String,
    },
}

/// Which overlay screen the battle is waiting for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Waiting {
    Drama,
    Screen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MenuKind {
    Command,
    Strategies,
    Items,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BattleMenuItem {
    EndTurn,
    Units,
    Objective,
    Save,
    QuickSave,
    QuickLoad,
    Settings,
    Title,
}

impl BattleMenuItem {
    const ALL: [BattleMenuItem; 8] = [
        BattleMenuItem::EndTurn,
        BattleMenuItem::Units,
        BattleMenuItem::Objective,
        BattleMenuItem::Save,
        BattleMenuItem::QuickSave,
        BattleMenuItem::QuickLoad,
        BattleMenuItem::Settings,
        BattleMenuItem::Title,
    ];

    fn label(self) -> &'static str {
        match self {
            BattleMenuItem::EndTurn => "턴 종료",
            BattleMenuItem::Units => "부대 일람",
            BattleMenuItem::Objective => "승리 조건",
            BattleMenuItem::Save => "중단 기록",
            BattleMenuItem::QuickSave => "순간 저장 (F5)",
            BattleMenuItem::QuickLoad => "순간 불러오기 (F9)",
            BattleMenuItem::Settings => "설정",
            BattleMenuItem::Title => "타이틀로",
        }
    }
}

/// Windows opened from the battle menu (they are modal over the map).
enum Panel {
    None,
    Menu(Menu),
    Units {
        side: Side,
        ids: Vec<UnitId>,
        menu: Menu,
    },
    Objective,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Confirm {
    EndTurn,
    Title,
}

/// A scene waiting to be shown over a battle resumed from a quick save.
#[derive(Debug, Clone)]
enum QueuedScene {
    /// The scene that was half-way: continues where it was.
    Resume(Box<SceneResume>),
    /// A scene whose beat had not started: plays from its beginning.
    Fresh(String),
}

/// The AI unit being played.
#[derive(Debug, Clone, Copy)]
struct AiStep {
    unit: UnitId,
    /// Plans applied so far (a move, then the action).
    tries: u8,
}

/// Right mouse button gesture: a click cancels, a drag pans the map.
#[derive(Debug, Clone, Copy)]
struct RightDrag {
    origin: Vec2,
    last: Vec2,
    moved: bool,
}

/// The battle screen. See the module docs.
pub struct BattleScreen {
    pack: Rc<Pack>,
    /// The pack's battle frame: the screen is drawn in it (`[presentation.battle_frame]`).
    frame: Option<BattleFrame>,
    state: BattleState,
    /// A new battle (title card, objective, `begin`) rather than a resumed save.
    fresh: bool,
    stage: Stage,
    meta: Meta,
    map: MapRenderer,
    camera: Camera,
    scene: Scene,
    events: EventPlayer,
    ui: PlayerUi,
    mode_menu: Option<(MenuKind, Menu)>,
    panel: Panel,
    dialog: Option<(ConfirmDialog, Confirm)>,
    waiting: Option<Waiting>,
    cursor: Pos,
    /// Cursor tile when the current press began (tap-to-preview, tap-again-to-confirm).
    cursor_at_press: Option<Pos>,
    ai: Option<AiStep>,
    ai_pause: f32,
    /// Scenes to show over the battle before it goes on: the scene a quick save was made in
    /// and the ones its animation had queued (see the module docs).
    queued_scenes: VecDeque<QueuedScene>,
    /// The pending move can be undone (its events were a plain `Moved`).
    move_undoable: bool,
    /// Jump to the player's units when the queue runs dry after a player phase started.
    focus_player: bool,
    idle_time: f32,
    touch_seen: bool,
    /// Seconds since the pointer last moved (edge scrolling stops for a resting pointer).
    pointer_rest: f32,
    rdrag: Option<RightDrag>,
    /// Class id -> sprite key.
    sprite_of: BTreeMap<String, String>,
    /// Sprite key -> sheet texture keys per side (player, ally, enemy), built once so drawing
    /// does not format strings.
    sheets: BTreeMap<String, [String; 3]>,
    cues: Vec<Cue>,
    /// Unit levels when the screen opened (for the level-ups in the result window).
    start_levels: Vec<u32>,
    /// Tile pictures of changed terrain drawn over the map: the state's
    /// [`BattleState::map_images`] as far as the animation has shown them.
    shown_tiles: Vec<MapImage>,
    /// The map was drawn from the tileset before a terrain change; rebuild it.
    map_stale: bool,
    /// Tiles the enemies could attack next phase (the "위험 범위" view option, D25 X4) with the
    /// [`player::danger_key`] they were computed for; refreshed only when that changes.
    danger: Option<(Vec<i64>, Vec<Pos>)>,
}

impl BattleScreen {
    /// New battle `battle_id` for the session's campaign. Returns an error screen when the battle
    /// cannot be built (unknown id, broken data).
    pub fn start(ctx: &mut Ctx, battle_id: &str) -> Box<dyn Screen> {
        let (Some(pack), Some(session)) = (ctx.pack.clone(), ctx.session.as_ref()) else {
            return Box::new(ErrorScreen::recoverable(
                "전투 오류",
                vec!["진행 중인 캠페인이 없습니다.".into()],
            ));
        };
        let seed = crate::platform::unix_now() ^ ctx.frame.wrapping_mul(0x9E37_79B9_7F4A_7C15);
        match BattleState::new(&pack, battle_id, &session.campaign, seed) {
            Ok(state) => {
                if let Some(s) = ctx.session.as_mut() {
                    s.battle = Some(state.clone());
                }
                Box::new(BattleScreen::new(pack, state, true, ctx.gfx.size()))
            }
            Err(e) => Box::new(ErrorScreen::recoverable(
                "전투 오류",
                vec![format!("전투 `{battle_id}`를 시작할 수 없습니다: {e}")],
            )),
        }
    }

    /// Continue the battle stored in the session (mid-battle save). `scene` and `pending_scenes`
    /// come from a quick save (see the module docs) and are shown before the battle goes on.
    pub fn resume(
        ctx: &mut Ctx,
        scene: Option<SceneResume>,
        pending_scenes: Vec<String>,
    ) -> Box<dyn Screen> {
        let pack = ctx.pack.clone();
        let battle = ctx.session.as_ref().and_then(|s| s.battle.clone());
        match (pack, battle) {
            (Some(pack), Some(state)) if pack.battles.contains_key(&state.battle_id) => {
                // The rules panic on units whose class is gone (a save from another pack
                // version); refuse such a save instead.
                match state.units.iter().find(|u| pack.class(&u.class).is_none()) {
                    Some(u) => Box::new(ErrorScreen::recoverable(
                        "전투 오류",
                        vec![format!(
                            "기록된 부대 `{}`의 병종 `{}`이(가) 데이터 팩에 없습니다.",
                            u.name, u.class
                        )],
                    )),
                    None => {
                        let mut screen = BattleScreen::new(pack, state, false, ctx.gfx.size());
                        screen.queued_scenes = scene
                            .map(|s| QueuedScene::Resume(Box::new(s)))
                            .into_iter()
                            .chain(pending_scenes.into_iter().map(QueuedScene::Fresh))
                            .collect();
                        Box::new(screen)
                    }
                }
            }
            (Some(_), Some(state)) => Box::new(ErrorScreen::recoverable(
                "전투 오류",
                vec![format!(
                    "기록된 전투 `{}`가 데이터 팩에 없습니다.",
                    state.battle_id
                )],
            )),
            _ => Box::new(ErrorScreen::recoverable(
                "전투 오류",
                vec!["이어서 할 전투 기록이 없습니다.".into()],
            )),
        }
    }

    /// The screen for `state` on a `canvas` sized canvas. The map starts with the default tile
    /// size; [`BattleScreen::use_tile_size`] switches to the tileset's once it has loaded.
    fn new(pack: Rc<Pack>, state: BattleState, fresh: bool, canvas: Vec2) -> BattleScreen {
        let map = MapRenderer::new(&state.map, DEFAULT_TILE as f32);
        let frame = pack.manifest.presentation.battle_frame.clone();
        let mut camera = Camera::new(viewport(canvas, frame.as_ref()), map.size, map.tile);
        let scene = Scene::new(&state);
        let cursor = state
            .units
            .iter()
            .find(|u| u.side == Side::Player && u.is_active() && u.lord)
            .or_else(|| {
                state
                    .units
                    .iter()
                    .find(|u| u.side == Side::Player && u.is_active())
            })
            .map_or(Pos::new(0, 0), |u| u.pos);
        camera.snap_to(cursor);
        let mut sprite_of = BTreeMap::new();
        let mut sheets = BTreeMap::new();
        for c in pack.classes.values() {
            sprite_of.insert(c.id.clone(), c.sprite.clone());
            sheets.insert(
                c.sprite.clone(),
                [Side::Player, Side::Ally, Side::Enemy]
                    .map(|side| sprites::sheet_key(&c.sprite, side)),
            );
        }
        BattleScreen {
            pack,
            frame,
            fresh,
            stage: Stage::Title { age: 0.0 },
            meta: Meta::default(),
            map,
            camera,
            scene,
            events: EventPlayer::default(),
            ui: PlayerUi::default(),
            mode_menu: None,
            panel: Panel::None,
            dialog: None,
            waiting: None,
            cursor,
            cursor_at_press: None,
            ai: None,
            ai_pause: 0.0,
            queued_scenes: VecDeque::new(),
            move_undoable: false,
            focus_player: false,
            idle_time: 0.0,
            touch_seen: false,
            pointer_rest: 0.0,
            rdrag: None,
            sprite_of,
            sheets,
            cues: Vec::new(),
            start_levels: state.units.iter().map(|u| u.level).collect(),
            shown_tiles: state.map_images.clone(),
            map_stale: false,
            danger: None,
            state,
        }
    }

    // ----- resources ---------------------------------------------------------------------

    fn start_loading(&mut self, ctx: &Ctx) {
        let root = ctx.media.root();
        // Each index from the first place that has it, like the pictures it describes: the
        // overlay, the top pack, then its parents.
        self.meta.tileset_req = Some(FirstOf::new(root.media_paths(tileset::TILESET_FILE)));
        self.meta.units_req = Some(FirstOf::new(root.media_paths(sprites::UNITS_FILE)));
        self.meta.fx_req = Some(FirstOf::new(root.media_paths(sprites::FX_FILE)));
        let mut textures: Vec<String> = vec!["ui/flags".into(), "ui/icons".into()];
        for u in &self.state.units {
            let sprite = self
                .sprite_of
                .get(&u.class)
                .map_or(u.class.as_str(), |s| s.as_str());
            if let Some(key) = self.sheet(sprite, u.side) {
                if !textures.iter().any(|t| t == key) {
                    textures.push(key.to_string());
                }
            }
        }
        self.meta.picture = self
            .def()
            .map
            .image
            .as_ref()
            .map(|key| format!("maps/{key}"));
        textures.extend(self.meta.picture.iter().cloned());
        textures.extend(self.frame.iter().map(|f| f.image.clone()));
        for e in &self.def().events {
            for a in &e.actions {
                if let EventAction::SetTerrain {
                    image: Some(key), ..
                } = a
                {
                    textures.push(format!("maps/{key}"));
                }
            }
        }
        ctx.media.preload_textures(&textures);
        let mut sounds: Vec<String> = sfx::ALL.iter().map(|k| format!("sfx/{k}")).collect();
        for key in [self.bgm_for(Side::Player), self.bgm_for(Side::Enemy)] {
            sounds.push(format!("bgm/{key}"));
        }
        sounds.push(format!("bgm/{}", bgm::VICTORY));
        sounds.push(format!("bgm/{}", bgm::DEFEAT));
        ctx.media.preload_sounds(&sounds);
    }

    fn poll_meta(&mut self, ctx: &Ctx) {
        if let Some(r) = request_text(&mut self.meta.tileset_req) {
            match r.and_then(|s| Tileset::parse(&s)) {
                Ok((ts, warnings)) => {
                    for w in warnings {
                        macroquad::logging::warn!("{}: {}", tileset::TILESET_FILE, w);
                    }
                    ctx.media.preload_textures(&[ts.texture.as_str()]);
                    self.use_tile_size(ts.tile_size);
                    self.meta.tileset = Some(ts);
                }
                Err(e) => {
                    macroquad::logging::warn!(
                        "{} unavailable, drawing flat terrain: {}",
                        tileset::TILESET_FILE,
                        e
                    );
                }
            }
        }
        if let Some(r) = request_text(&mut self.meta.units_req) {
            match r.and_then(|s| sprites::parse_units(&s)) {
                Ok(units) => {
                    // Officers' own sprites (the original's Liu Bei, Lü Bu, Cao Cao) of the
                    // units in this battle: their sheets load now.
                    let mut textures = Vec::new();
                    for u in &self.state.units {
                        let Some(own) = u.officer.as_deref().and_then(|o| units.officers.get(o))
                        else {
                            continue;
                        };
                        for key in own.values() {
                            let keys = self.sheets.entry(key.clone()).or_insert_with(|| {
                                [Side::Player, Side::Ally, Side::Enemy]
                                    .map(|side| sprites::sheet_key(key, side))
                            });
                            textures.extend(keys.iter().cloned());
                        }
                    }
                    // Any unit may come under a status.
                    for key in units.statuses.values() {
                        let keys = self.sheets.entry(key.clone()).or_insert_with(|| {
                            [Side::Player, Side::Ally, Side::Enemy]
                                .map(|side| sprites::sheet_key(key, side))
                        });
                        textures.extend(keys.iter().cloned());
                    }
                    textures.sort();
                    textures.dedup();
                    ctx.media.preload_textures(&textures);
                    self.meta.units = units;
                }
                Err(e) => {
                    macroquad::logging::warn!(
                        "{} unavailable, using 16x16 frames: {}",
                        sprites::UNITS_FILE,
                        e
                    );
                }
            }
        }
        if let Some(r) = request_text(&mut self.meta.fx_req) {
            match r.and_then(|s| sprites::parse_fx(&s)) {
                Ok(fx) => {
                    let keys: Vec<String> = fx.keys().map(|k| format!("fx/{k}")).collect();
                    ctx.media.preload_textures(&keys);
                    self.meta.fx_textures =
                        fx.keys().map(|k| (k.clone(), format!("fx/{k}"))).collect();
                    self.meta.fx = fx;
                }
                Err(e) => {
                    macroquad::logging::warn!(
                        "{} unavailable, no effects: {}",
                        sprites::FX_FILE,
                        e
                    );
                }
            }
        }
        if self.map_stale && self.map.is_built() {
            self.map_stale = false;
            self.map = MapRenderer::new(&self.state.map, self.map.tile);
        }
        if !self.map.is_built() && self.meta.tileset_req.is_none() {
            // The tile size is known now (the tileset's, or the default without one), so the
            // picture layer can be checked against the map; one that is missing or does not
            // fit is given up with a warning and the tileset draws the map.
            if let Some(key) = self.meta.picture.clone() {
                match ctx.media.texture_state(&key) {
                    AssetState::Loading => return,
                    AssetState::Ready => {
                        if let Some(picture) = ctx.media.texture(&key) {
                            let size = vec2(picture.width(), picture.height());
                            if self.map.fits(size) {
                                self.map.use_picture(picture);
                                return;
                            }
                            macroquad::logging::warn!(
                                "gfx/{}.png is {}x{} pixels, the map needs {}x{}; drawing the tileset",
                                key,
                                size.x,
                                size.y,
                                self.map.size.x,
                                self.map.size.y
                            );
                        }
                    }
                    AssetState::Missing => {
                        macroquad::logging::warn!(
                            "gfx/{}.png unavailable; drawing the tileset",
                            key
                        );
                    }
                }
                self.meta.picture = None;
            }
            match &self.meta.tileset {
                Some(ts) => match ctx.media.texture_state(&ts.texture) {
                    AssetState::Loading => {}
                    AssetState::Ready => {
                        let atlas = ctx.media.texture(&ts.texture);
                        self.map
                            .build(&self.state.map, &self.pack, Some(ts), atlas.as_ref());
                    }
                    AssetState::Missing => {
                        self.map.build(&self.state.map, &self.pack, None, None);
                    }
                },
                None => self.map.build(&self.state.map, &self.pack, None, None),
            }
        }
    }

    // ----- helpers -----------------------------------------------------------------------

    /// Size of a map tile in virtual pixels (the tileset's `tile_size`).
    fn tile(&self) -> f32 {
        self.map.tile
    }

    /// The animation reached a terrain change at `pos`: draw the tile's new picture, or redraw
    /// the tileset map with the new terrain.
    fn show_terrain(&mut self, pos: Pos) {
        self.shown_tiles.retain(|m| m.pos != pos);
        match self.state.map_images.iter().find(|m| m.pos == pos) {
            Some(m) => self.shown_tiles.push(m.clone()),
            None => self.map_stale = !self.map.uses_picture(),
        }
    }

    /// Lay the map out with `tile` pixel tiles (the tileset's `tile_size`): a new map renderer
    /// and camera, centred on the cursor again. Called before the map is first built.
    fn use_tile_size(&mut self, tile: f32) {
        if tile == self.map.tile {
            return;
        }
        self.map = MapRenderer::new(&self.state.map, tile);
        self.camera = Camera::new(self.camera.viewport, self.map.size, tile);
        self.camera.snap_to(self.cursor);
    }

    /// Sprite key a unit of `officer` (if any) and `class`, `confused` or not, is drawn with:
    /// the status's (`units.toml` `[statuses]`), else the officer's own (`[officers]`), else
    /// the class's.
    fn unit_sprite<'a>(&'a self, officer: Option<&str>, class: &'a str, confused: bool) -> &'a str {
        let class_sprite = self.sprite_of.get(class).map_or(class, |s| s.as_str());
        let statuses: &[&str] = if confused {
            &[hero_core::data::StatusKind::Confused.id()]
        } else {
            &[]
        };
        self.meta.units.sprite_for(officer, class_sprite, statuses)
    }

    /// Texture key of a unit sheet.
    fn sheet(&self, sprite: &str, side: Side) -> Option<&str> {
        let i = match side {
            Side::Player => 0,
            Side::Ally => 1,
            Side::Enemy => 2,
        };
        self.sheets.get(sprite).map(|keys| keys[i].as_str())
    }

    fn def(&self) -> &hero_core::battledef::BattleDef {
        self.state.def(&self.pack)
    }

    fn bgm_for(&self, side: Side) -> String {
        let def = self.def();
        match side {
            Side::Enemy => def
                .bgm_enemy
                .clone()
                .unwrap_or_else(|| bgm::ENEMY.to_string()),
            Side::Player | Side::Ally => def.bgm.clone().unwrap_or_else(|| bgm::BATTLE.to_string()),
        }
    }

    fn speed(&self, ctx: &Ctx) -> f32 {
        let held = ctx.input.key_down(KeyCode::Z)
            || ctx.input.key_down(KeyCode::Enter)
            || ctx.input.key_down(KeyCode::Space)
            || (ctx.input.down()
                && self.ui.mode == Mode::Browse
                && self.state.phase != Side::Player);
        ctx.settings.battle_speed.multiplier() * if held { FAST_FORWARD } else { 1.0 }
    }

    /// Keep the danger tiles of the "위험 범위" option up to date while the player's phase waits
    /// for input (D25 X4); dropped while the option is off.
    fn refresh_danger(&mut self, ctx: &Ctx) {
        if !ctx.settings.danger_range {
            self.danger = None;
            return;
        }
        if self.state.phase != Side::Player || !self.events.is_idle() {
            return;
        }
        let key = player::danger_key(&self.state);
        if self.danger.as_ref().is_none_or(|(k, _)| *k != key) {
            let tiles = player::danger_tiles(&self.state, &self.pack);
            self.danger = Some((key, tiles));
        }
    }

    /// Whether the danger tiles show: the option is on and the player browses the map with
    /// nothing selected and no window open.
    fn shows_danger(&self, ctx: &Ctx) -> bool {
        ctx.settings.danger_range
            && matches!(self.ui.mode, Mode::Browse)
            && matches!(self.panel, Panel::None)
            && self.dialog.is_none()
            && self.waiting.is_none()
            && self.mode_menu.is_none()
    }

    fn store_session(&self, ctx: &mut Ctx) {
        if let Some(s) = ctx.session.as_mut() {
            s.battle = Some(self.state.clone());
        }
    }

    /// Display name of a unit reference (tag or officer id) for objective texts.
    fn ref_name(&self, r: &str) -> String {
        if let Some(u) = self.state.find_unit(r) {
            return self.state.units[u].name.clone();
        }
        self.pack
            .officer(r)
            .map_or_else(|| r.to_string(), |o| o.name.clone())
    }

    /// Apply an action, keep the session in sync and queue its animation.
    fn apply(&mut self, ctx: &mut Ctx, action: Action) -> Option<Vec<BattleEvent>> {
        match self.state.apply(&self.pack, action.clone()) {
            Ok(events) => {
                self.store_session(ctx);
                self.events
                    .push(anim::plan(&events, &self.state, &self.pack, &self.meta.fx));
                if self.events.take_finished() {
                    // Nothing to animate (a plain wait): show the result right away.
                    self.events_done();
                }
                Some(events)
            }
            Err(e) => {
                macroquad::logging::error!("battle action {:?} rejected: {}", action, e);
                ctx.sfx(sfx::ERROR);
                None
            }
        }
    }

    fn play_phase_music(&self, ctx: &mut Ctx, side: Side) {
        let key = self.bgm_for(side);
        ctx.audio.play_bgm(&key);
    }

    /// Carry out the cues of the animation; returns a transition for drama overlays.
    fn handle_cues(&mut self, ctx: &mut Ctx) -> Transition {
        let mut out = Transition::None;
        for cue in std::mem::take(&mut self.cues) {
            match cue {
                Cue::Sfx(k) => ctx.sfx(k),
                Cue::Follow(p) => self.camera.keep_visible(p, 2.5),
                Cue::Center(p) => self.camera.center_on(p),
                Cue::PhaseMusic(side) => {
                    self.play_phase_music(ctx, side);
                    if side == Side::Player {
                        self.focus_player = true;
                    }
                }
                Cue::Jingle(victory) => {
                    let key = if victory { bgm::VICTORY } else { bgm::DEFEAT };
                    // Without the jingle file, the short effect of the same name stands in.
                    if ctx.media.sound_state(&format!("bgm/{key}")) == AssetState::Missing {
                        ctx.audio.stop_bgm();
                        ctx.sfx(if victory { sfx::VICTORY } else { sfx::DEFEAT });
                    } else {
                        ctx.audio.play_jingle(key);
                    }
                }
                Cue::Terrain(pos) => self.show_terrain(pos),
                Cue::Drama(scene) => out = self.open_drama(ctx, &scene),
            }
        }
        out
    }

    /// Show the drama scene `scene` over the battle.
    fn open_drama(&mut self, ctx: &mut Ctx, scene: &str) -> Transition {
        // The map stays drawn under the overlay without updates: never leave it shaken.
        self.scene.shake = 0.0;
        self.camera.shake = Vec2::ZERO;
        if self.pack.scene(scene).is_none() {
            macroquad::logging::warn!("battle drama scene `{}` not found", scene);
            self.events.resume();
            return Transition::None;
        }
        // The scene runs on the campaign's state: it sees the flags the battle has set so far
        // (they would reach the campaign only when it is over).
        if let Some(session) = ctx.session.as_mut() {
            session.campaign.merge_battle_flags(&self.state);
        }
        self.waiting = Some(Waiting::Drama);
        let terrain = officer_terrain(&self.pack, &self.state);
        Transition::push(DramaScreen::battle_overlay(ctx, scene, terrain))
    }

    /// Show the next scene a quick save left queued (see [`QueuedScene`]).
    fn open_queued_scene(&mut self, ctx: &mut Ctx) -> Transition {
        match self.queued_scenes.pop_front() {
            Some(QueuedScene::Resume(resume)) => {
                // Its campaign flags are in the save already; only the screen is rebuilt.
                self.waiting = Some(Waiting::Drama);
                Transition::push(DramaScreen::restore(ctx, *resume))
            }
            Some(QueuedScene::Fresh(scene)) => self.open_drama(ctx, &scene),
            None => Transition::None,
        }
    }

    /// Run [`BattleScreen::events_done`] and rebuild the menus once the queued batches have
    /// played out, however the queue ran dry (see [`EventPlayer::take_finished`]).
    fn settle_events(&mut self, ctx: &Ctx) {
        if self.events.take_finished() {
            self.events_done();
            self.refresh_mode_menu(ctx);
        }
    }

    /// The animation queue ran dry: snap the views to the state and continue the flow.
    fn events_done(&mut self) {
        self.scene.sync(&self.state);
        if matches!(self.ui.mode, Mode::Walking { .. }) {
            self.ui.walked(&self.state, self.move_undoable);
            self.move_undoable = false;
        }
        if self.state.phase == Side::Player && self.focus_player && self.state.outcome.is_none() {
            self.focus_player = false;
            self.ui.reset();
            let focus = player::cycle_actor(&self.state, None, 1).or_else(|| {
                self.state
                    .units
                    .iter()
                    .position(|u| u.lord && u.is_active())
            });
            if let Some(u) = focus {
                self.cursor = self.state.units[u].pos;
                self.camera.center_on(self.cursor);
            }
        }
        self.idle_time = 0.0;
    }

    fn begin_battle(&mut self, ctx: &mut Ctx) {
        let events = self.state.begin(&self.pack);
        self.store_session(ctx);
        self.events
            .push(anim::plan(&events, &self.state, &self.pack, &self.meta.fx));
        self.stage = Stage::Battle;
    }

    fn open_result(&mut self) {
        let def = self.def();
        let item_name = |id: &str| {
            self.pack
                .item(id)
                .map_or(id.to_string(), |i| i.name.clone())
        };
        let (title, sections) = match self.state.outcome {
            Some(Outcome::Victory) => {
                let mut sections = Vec::new();
                sections.push((
                    "전리품".to_string(),
                    vec![(
                        format!("금 {}", crate::ui::format::thousands(self.state.gold_found)),
                        theme::TEXT_ACCENT,
                    )],
                ));
                let mut counts: BTreeMap<String, u32> = BTreeMap::new();
                for i in &self.state.items_found {
                    *counts.entry(item_name(i)).or_default() += 1;
                }
                if !counts.is_empty() {
                    let items = counts
                        .into_iter()
                        .map(|(n, c)| if c > 1 { format!("{n} ×{c}") } else { n })
                        .collect::<Vec<_>>()
                        .join(", ");
                    sections[0].1.push((items, theme::TEXT));
                }
                let grown = self.level_ups();
                if !grown.is_empty() {
                    sections.push(("성장".to_string(), grown));
                }
                if let Some(b) = &def.bonus {
                    let line = if self.state.bonus_done {
                        (
                            format!("달성 — 출진 부대 경험치 +{}", b.exp),
                            theme::TEXT_GOOD,
                        )
                    } else {
                        (format!("미달성 — {}", b.desc), theme::TEXT_DIM)
                    };
                    sections.push(("보너스 목표".to_string(), vec![line]));
                }
                (format!("{} 승리", def.name), sections)
            }
            Some(Outcome::Defeat(reason)) => {
                let lord = self
                    .state
                    .units
                    .iter()
                    .find(|u| u.lord)
                    .map(|u| u.name.clone());
                (
                    format!("{} 패배", def.name),
                    vec![(
                        "패인".to_string(),
                        vec![(text::defeat_text(reason, lord.as_deref()), theme::TEXT_BAD)],
                    )],
                )
            }
            None => return,
        };
        self.stage = Stage::Result {
            lines: sections,
            title,
        };
    }

    /// `이름 Lv a → b` for every player unit that gained levels in this battle.
    fn level_ups(&self) -> Vec<(String, Color)> {
        let mut lines: Vec<String> = self
            .state
            .units
            .iter()
            .zip(&self.start_levels)
            .filter(|(u, &from)| u.side == Side::Player && u.level > from)
            .map(|(u, from)| format!("{} Lv {from} → {}", u.name, u.level))
            .collect();
        // Keep the window small: two per line.
        let mut out = Vec::new();
        while !lines.is_empty() {
            let take: Vec<String> = lines.drain(..lines.len().min(2)).collect();
            out.push((take.join("   "), theme::TEXT_GOOD));
        }
        out
    }

    // ----- AI ----------------------------------------------------------------------------

    fn ai_update(&mut self, ctx: &mut Ctx, dt: f32) {
        if self.ai_pause > 0.0 {
            self.ai_pause -= dt;
            return;
        }
        let side = self.state.phase;
        let Some(step) = self.ai else {
            match self.state.next_ai_unit() {
                Some(id) => {
                    self.ai = Some(AiStep { unit: id, tries: 0 });
                    let p = self.state.units[id].pos;
                    self.cursor = p;
                    if !self.camera.is_visible(p) {
                        self.camera.center_on(p);
                    } else {
                        self.camera.keep_visible(p, 3.0);
                    }
                    self.ai_pause = AI_PAUSE;
                }
                None => {
                    self.ai = None;
                    if self.apply(ctx, Action::EndPhase).is_none() {
                        macroquad::logging::error!("the AI phase could not be ended");
                    }
                }
            }
            return;
        };
        let id = step.unit;
        if !self.state.can_act(id) || self.state.phase != side {
            self.ai = None;
            return;
        }
        let plan = if step.tries < 2 {
            self.state.ai_actions(&self.pack, id).into_iter().next()
        } else {
            None
        };
        let Some(action) = plan else {
            self.ai_wait(ctx, id);
            return;
        };
        let is_move = matches!(action, Action::Move { .. });
        if self.apply(ctx, action).is_none() {
            self.ai_wait(ctx, id);
            return;
        }
        if is_move && self.state.can_act(id) {
            self.ai = Some(AiStep {
                unit: id,
                tries: step.tries + 1,
            });
        } else {
            if self.state.can_act(id) {
                self.ai_wait(ctx, id);
            }
            self.ai = None;
        }
    }

    /// Let an AI unit wait (planning failed or produced nothing); if even that is refused, end
    /// the phase so the battle cannot get stuck.
    fn ai_wait(&mut self, ctx: &mut Ctx, id: UnitId) {
        self.ai = None;
        if self.apply(ctx, Action::Wait { unit: id }).is_none() {
            self.apply(ctx, Action::EndPhase);
        }
    }

    // ----- player phase ------------------------------------------------------------------

    fn open_battle_menu(&mut self, ctx: &mut Ctx) {
        let items: Vec<MenuItem> = BattleMenuItem::ALL
            .iter()
            .map(|i| MenuItem::new(i.label()))
            .collect();
        let mut menu = Menu::new(items);
        let w = menu.fit_width(&ctx.gfx).max(110.0);
        let h = menu.rect().h;
        let vp = self.camera.viewport;
        menu.set_width(w);
        menu.set_position(
            (vp.x + (vp.w - w) / 2.0).round(),
            (vp.y + (vp.h - h) / 2.0).round(),
        );
        ctx.sfx(sfx::CONFIRM);
        self.panel = Panel::Menu(menu);
    }

    /// A tap on one of the battle frame's buttons while no command is under way: `menu` opens
    /// the battle menu, `allies` and `enemies` the unit lists. `true` when it took the tap.
    fn frame_buttons(&mut self, ctx: &mut Ctx) -> bool {
        let Some(f) = &self.frame else {
            return false;
        };
        if !matches!(self.ui.mode, Mode::Browse) {
            return false;
        }
        let Some(p) = ctx.input.tap() else {
            return false;
        };
        let pressed = frame_button_at(f, p);
        let Some(button) = pressed else {
            return false;
        };
        ctx.input.consume();
        match button {
            FrameButton::Menu => self.open_battle_menu(ctx),
            FrameButton::Allies | FrameButton::Enemies => {
                ctx.sfx(sfx::CONFIRM);
                self.open_unit_list(if button == FrameButton::Allies {
                    Side::Player
                } else {
                    Side::Enemy
                });
            }
        }
        true
    }

    fn open_unit_list(&mut self, side: Side) {
        let ids: Vec<UnitId> = self
            .state
            .units
            .iter()
            .filter(|u| u.side == side && u.is_active())
            .map(|u| u.id)
            .collect();
        let items: Vec<MenuItem> = ids
            .iter()
            .map(|&id| {
                let u = &self.state.units[id];
                let class = self
                    .pack
                    .class(&u.class)
                    .map_or(u.class.as_str(), |c| c.name.as_str());
                let mut label = format!("{class} Lv{}", u.level);
                if side == Side::Player && u.acted {
                    label.push_str(" (행동 끝)");
                }
                MenuItem::new(label)
                    .tag(u.name.clone())
                    .detail(format!("{}/{}", u.hp, u.max_hp))
            })
            .collect();
        let empty = items.is_empty();
        let mut menu = Menu::new(if empty {
            vec![MenuItem::new("— 없음 —").enabled(false)]
        } else {
            items
        })
        .rows(9);
        menu.tag_width = 64.0;
        menu.wrap = false;
        let vp = self.camera.viewport;
        menu.set_width(UNIT_LIST_W);
        menu.set_position((vp.x + (vp.w - UNIT_LIST_W) / 2.0).round(), vp.y + 30.0);
        self.panel = Panel::Units { side, ids, menu };
    }

    /// Rebuild the command / strategy / item menu when the mode changed.
    fn refresh_mode_menu(&mut self, ctx: &Ctx) {
        let wanted = match &self.ui.mode {
            Mode::Command { .. } => Some(MenuKind::Command),
            Mode::Strategies { .. } => Some(MenuKind::Strategies),
            Mode::Items { .. } => Some(MenuKind::Items),
            _ => None,
        };
        if self.mode_menu.as_ref().map(|(k, _)| *k) == wanted {
            return;
        }
        let Some(kind) = wanted else {
            self.mode_menu = None;
            return;
        };
        let unit = self.ui.unit().expect("menu modes have a unit");
        let (items, width) = match &self.ui.mode {
            Mode::Command { .. } => (
                Command::ALL
                    .iter()
                    .map(|c| {
                        MenuItem::new(c.label()).enabled(player::command_enabled(
                            &self.state,
                            &self.pack,
                            unit,
                            *c,
                        ))
                    })
                    .collect::<Vec<_>>(),
                76.0,
            ),
            Mode::Strategies { list, .. } => (
                list.iter()
                    .map(|e| {
                        MenuItem::new(e.name.clone())
                            .tag(String::new())
                            .detail(format!("MP {}", e.mp))
                            .enabled(e.aims.is_ok())
                    })
                    .collect(),
                176.0,
            ),
            Mode::Items { list, .. } => (
                list.iter()
                    .map(|e| {
                        MenuItem::new(e.name.clone())
                            .tag(String::new())
                            .detail(format!("×{}", e.count))
                            .enabled(e.targets.is_ok())
                    })
                    .collect(),
                176.0,
            ),
            _ => return,
        };
        let mut menu = Menu::new(items).rows(7);
        if kind != MenuKind::Command {
            menu.tag_width = 18.0;
            menu.wrap = false;
        }
        let w = if kind == MenuKind::Command {
            menu.fit_width(&ctx.gfx).max(width)
        } else {
            width
        };
        menu.set_width(w);
        let h = menu.rect().h;
        let size = self.tile();
        let canvas = ctx.gfx.size();
        let vp = self.camera.viewport;
        // In a battle frame the menus stay over the map (the frame's panel shows the unit).
        let area = match self.frame {
            Some(_) => vp,
            None => Rect::new(0.0, 0.0, canvas.x, canvas.y),
        };
        let tile = self.camera.tile_screen(self.state.units[unit].pos);
        let (x, y) = if kind == MenuKind::Command {
            // Beside the unit, on the right when there is room.
            let x = if tile.x + size + 6.0 + w <= area.right() - 4.0 {
                tile.x + size + 6.0
            } else {
                tile.x - w - 6.0
            };
            (x, tile.y + size / 2.0 - h / 2.0)
        } else {
            // Lists go to the side of the screen away from the unit.
            let x = if tile.x < area.center().x {
                area.right() - w - 8.0
            } else {
                area.x + 8.0
            };
            (x, vp.y + 6.0)
        };
        let x = x.clamp(area.x + 4.0, area.right() - w - 4.0).round();
        let y = y.clamp(vp.y + 4.0, area.bottom() - h - 4.0).round();
        menu.set_position(x, y);
        self.mode_menu = Some((kind, menu));
    }

    /// Carry out a decision of the command state machine.
    fn handle_request(&mut self, ctx: &mut Ctx, req: Request) {
        match req {
            Request::None => {}
            Request::Invalid => ctx.sfx(sfx::ERROR),
            Request::BattleMenu => self.open_battle_menu(ctx),
            Request::Restore(snapshot) => {
                self.state = *snapshot;
                self.store_session(ctx);
                self.scene.sync(&self.state);
                if let Some(u) = self.ui.unit() {
                    self.cursor = self.state.units[u].pos;
                }
                ctx.sfx(sfx::CANCEL);
            }
            Request::Apply(action) => {
                let moving = matches!(action, Action::Move { .. });
                match self.apply(ctx, action) {
                    Some(events) => {
                        if moving {
                            self.move_undoable = player::move_is_undoable(&events);
                        } else {
                            // The unit has acted; the selection ends.
                            self.ui.reset();
                        }
                    }
                    None => self.ui.reset(),
                }
            }
        }
        self.refresh_mode_menu(ctx);
    }

    /// Target list of the attack / item target modes (for keyboard cycling).
    fn cycle_targets(&self) -> Vec<Pos> {
        match &self.ui.mode {
            Mode::Attack { targets, .. } => {
                targets.iter().map(|&t| self.state.units[t].pos).collect()
            }
            Mode::ItemTarget { list, index, .. } => match &list[*index].targets {
                Ok(t) => t.iter().map(|&u| self.state.units[u].pos).collect(),
                Err(_) => Vec::new(),
            },
            _ => Vec::new(),
        }
    }

    fn is_target_mode(&self) -> bool {
        matches!(
            self.ui.mode,
            Mode::Attack { .. } | Mode::Aim { .. } | Mode::ItemTarget { .. }
        )
    }

    /// Put the cursor on the first target when a target mode opens.
    fn snap_cursor_to_target(&mut self) {
        let first = match &self.ui.mode {
            Mode::Aim { list, index, .. } => {
                let aims = list[*index].aims.as_ref().ok();
                aims.and_then(|a| {
                    a.iter()
                        .copied()
                        .find(|p| self.state.unit_at(*p).is_some())
                        .or_else(|| a.first().copied())
                })
            }
            _ => self.cycle_targets().first().copied(),
        };
        if let Some(p) = first {
            self.cursor = p;
            self.camera.keep_visible(p, 2.0);
        }
    }

    fn player_update(&mut self, ctx: &mut Ctx, dt: f32) -> Transition {
        // Modal dialog.
        if let Some((dialog, kind)) = self.dialog.as_mut() {
            let kind = *kind;
            match dialog.update(ctx) {
                ConfirmEvent::Yes => {
                    self.dialog = None;
                    match kind {
                        Confirm::EndTurn => {
                            self.ui.reset();
                            self.refresh_mode_menu(ctx);
                            self.apply(ctx, Action::EndPhase);
                        }
                        Confirm::Title => return Transition::Flow(Flow::Title),
                    }
                }
                ConfirmEvent::No => self.dialog = None,
                ConfirmEvent::None => {}
            }
            return Transition::None;
        }

        // Battle menu and its windows.
        match std::mem::replace(&mut self.panel, Panel::None) {
            Panel::None => {}
            Panel::Menu(mut menu) => {
                match menu.update(ctx) {
                    MenuEvent::Selected(i) => return self.battle_menu(ctx, BattleMenuItem::ALL[i]),
                    MenuEvent::Cancelled => {}
                    // A tap outside closes it (touch has no cancel key).
                    _ if tapped_outside(ctx, menu.rect()) => {
                        ctx.sfx(sfx::CANCEL);
                        ctx.input.consume();
                    }
                    _ => self.panel = Panel::Menu(menu),
                }
                return Transition::None;
            }
            Panel::Units {
                side,
                ids,
                mut menu,
            } => {
                let current = UNIT_TABS.iter().position(|s| *s == side).unwrap_or(0);
                let switch = match ctx.input.nav() {
                    Some(crate::input::Dir::Left) => Some((current + 2) % 3),
                    Some(crate::input::Dir::Right) => Some((current + 1) % 3),
                    _ => ctx
                        .input
                        .tap()
                        .and_then(|p| unit_tab_at(self.camera.viewport, p)),
                };
                if let Some(tab) = switch {
                    let next = UNIT_TABS[tab];
                    ctx.sfx(sfx::CURSOR);
                    ctx.input.consume();
                    self.open_unit_list(next);
                    return Transition::None;
                }
                match menu.update(ctx) {
                    MenuEvent::Selected(i) => {
                        if let Some(&u) = ids.get(i) {
                            self.cursor = self.state.units[u].pos;
                            self.camera.center_on(self.cursor);
                        }
                    }
                    MenuEvent::Cancelled => {}
                    // A tap outside the window (tabs included) closes it.
                    _ if tapped_outside(ctx, unit_list_frame(menu.rect())) => {
                        ctx.sfx(sfx::CANCEL);
                        ctx.input.consume();
                    }
                    _ => self.panel = Panel::Units { side, ids, menu },
                }
                return Transition::None;
            }
            Panel::Objective => {
                if ctx.input.confirm() || ctx.input.cancel() {
                    ctx.sfx(sfx::CANCEL);
                    ctx.input.consume();
                } else {
                    self.panel = Panel::Objective;
                }
                return Transition::None;
            }
        }

        // Command / strategy / item menus.
        if let Some((_, menu)) = self.mode_menu.as_mut() {
            let ev = menu.update(ctx);
            let over = ctx.input.pointer().is_some_and(|p| menu.rect().contains(p));
            match ev {
                MenuEvent::Selected(i) => {
                    let req = self.ui.choose(&self.state, &self.pack, i);
                    self.handle_request(ctx, req);
                    if self.is_target_mode() {
                        self.snap_cursor_to_target();
                    }
                }
                MenuEvent::Cancelled => {
                    let req = self.ui.cancel(&self.state, &self.pack);
                    self.handle_request(ctx, req);
                }
                _ => {
                    // A tap outside the menu steps back (touch friendly).
                    if ctx.input.tap().is_some() && !over {
                        ctx.sfx(sfx::CANCEL);
                        let req = self.ui.cancel(&self.state, &self.pack);
                        self.handle_request(ctx, req);
                    }
                }
            }
            return Transition::None;
        }

        if self.frame_buttons(ctx) {
            return Transition::None;
        }
        self.map_input(ctx, dt);

        // Everyone has acted: end the phase by itself.
        if self.ui.mode == Mode::Browse
            && self.state.phase == Side::Player
            && player::cycle_actor(&self.state, None, 1).is_none()
        {
            self.idle_time += dt;
            if self.idle_time >= AUTO_END_DELAY {
                self.idle_time = 0.0;
                self.apply(ctx, Action::EndPhase);
            }
        } else {
            self.idle_time = 0.0;
        }
        Transition::None
    }

    fn battle_menu(&mut self, ctx: &mut Ctx, item: BattleMenuItem) -> Transition {
        match item {
            BattleMenuItem::EndTurn => {
                if player::cycle_actor(&self.state, None, 1).is_some() {
                    self.dialog = Some((
                        ConfirmDialog::new(
                            &ctx.gfx,
                            "아직 행동하지 않은 부대가 있습니다. 턴을 종료할까요?",
                        ),
                        Confirm::EndTurn,
                    ));
                } else {
                    self.ui.reset();
                    self.apply(ctx, Action::EndPhase);
                }
                Transition::None
            }
            BattleMenuItem::Units => {
                self.open_unit_list(Side::Player);
                Transition::None
            }
            BattleMenuItem::Objective => {
                self.panel = Panel::Objective;
                Transition::None
            }
            BattleMenuItem::Save => match ctx.session.as_ref() {
                Some(session) => {
                    let save = session.to_save(&self.pack);
                    self.waiting = Some(Waiting::Screen);
                    Transition::push(SaveLoadScreen::save(save))
                }
                None => {
                    ctx.sfx(sfx::ERROR);
                    ctx.toast("기록할 게임이 없습니다.");
                    Transition::None
                }
            },
            BattleMenuItem::QuickSave => Transition::QuickSave,
            BattleMenuItem::QuickLoad => Transition::QuickLoad,
            BattleMenuItem::Settings => {
                self.waiting = Some(Waiting::Screen);
                Transition::push(SettingsScreen::new())
            }
            BattleMenuItem::Title => {
                self.dialog = Some((
                    ConfirmDialog::new(
                        &ctx.gfx,
                        "전투를 중단하고 타이틀로 돌아갈까요? 기록하지 않은 진행은 사라집니다.",
                    )
                    .default_no(),
                    Confirm::Title,
                ));
                Transition::None
            }
        }
    }

    /// Cursor, camera and map clicks while no menu is open.
    fn map_input(&mut self, ctx: &mut Ctx, dt: f32) {
        let input = &ctx.input;
        if input.pressed() {
            self.cursor_at_press = Some(self.cursor);
        }
        let pointer = input.pointer();
        let hover = pointer.and_then(|p| self.camera.tile_at(p));
        let vp = self.camera.viewport;

        // Camera: left drag (touch / mouse), right drag, wheel, mouse at the edges.
        if let Some(drag) = input.drag() {
            if vp.contains(drag.origin) {
                self.camera.pan(-drag.delta);
            }
        }
        let right_down = is_mouse_button_down(MouseButton::Right);
        let mut right_click = false;
        match (self.rdrag, right_down, pointer) {
            (None, true, Some(p)) if input.right_click() => {
                self.rdrag = Some(RightDrag {
                    origin: p,
                    last: p,
                    moved: false,
                });
            }
            (Some(mut r), true, Some(p)) => {
                if !r.moved && r.origin.distance(p) > crate::input::DRAG_THRESHOLD {
                    r.moved = true;
                }
                if r.moved {
                    self.camera.pan(r.last - p);
                }
                r.last = p;
                self.rdrag = Some(r);
            }
            (Some(r), false, _) | (Some(r), true, None) => {
                self.rdrag = None;
                right_click = !r.moved;
            }
            _ => {}
        }
        // The wheel scrolls two tiles per step (Shift: sideways).
        let wheel = input.wheel();
        if wheel != 0 && pointer.is_some_and(|p| vp.contains(p)) {
            let step = wheel as f32 * 2.0 * self.tile();
            if input.key_down(KeyCode::LeftShift) || input.key_down(KeyCode::RightShift) {
                self.camera.pan(vec2(step, 0.0));
            } else {
                self.camera.pan(vec2(0.0, step));
            }
        }
        if !touches().is_empty() {
            self.touch_seen = true;
        }
        if !self.touch_seen
            && self.rdrag.is_none()
            && !input.down()
            && !crate::platform::pointer_left()
        {
            if let Some(p) = pointer {
                // In the browser leaving the canvas is reported, so a pointer resting at the edge
                // keeps scrolling; natively the rest time stands in for it.
                let rest = if crate::platform::is_web() {
                    0.0
                } else {
                    self.pointer_rest
                };
                let d = edge_direction(vp, p, rest);
                if d != Vec2::ZERO {
                    self.camera.pan(d * EDGE_PAN_SPEED * dt);
                }
            }
        }

        // Hover moves the cursor (real pointer movement only).
        if input.pointer_moved() && input.drag().is_none() && self.rdrag.is_none_or(|r| !r.moved) {
            if let Some(t) = hover {
                self.cursor = t;
            }
        }

        // Keyboard.
        let key_cancel = input.cancel() && !input.right_click();
        if let Some(dir) = input.nav() {
            let targets = self.cycle_targets();
            if !targets.is_empty() {
                let i = targets.iter().position(|p| *p == self.cursor);
                let step = match dir {
                    crate::input::Dir::Left | crate::input::Dir::Up => -1,
                    _ => 1,
                };
                let n = targets.len() as i32;
                let next = match i {
                    Some(i) => (i as i32 + step).rem_euclid(n) as usize,
                    None => 0,
                };
                self.cursor = targets[next];
            } else {
                let (dx, dy) = dir.delta();
                let m = &self.state.map;
                self.cursor = Pos::new(
                    (self.cursor.x + dx).clamp(0, m.width - 1),
                    (self.cursor.y + dy).clamp(0, m.height - 1),
                );
            }
            ctx.sfx(sfx::CURSOR);
            self.camera.keep_visible(self.cursor, 2.0);
        }
        let cycle = if ctx.input.key_pressed(KeyCode::Tab) || ctx.input.key_pressed(KeyCode::E) {
            Some(1)
        } else if ctx.input.key_pressed(KeyCode::Q) {
            Some(-1)
        } else {
            None
        };
        if let Some(step) = cycle {
            if matches!(
                self.ui.mode,
                Mode::Browse | Mode::Move { .. } | Mode::Inspect { .. }
            ) {
                let from = self.ui.unit().or_else(|| self.state.unit_at(self.cursor));
                match player::cycle_actor(&self.state, from, step) {
                    Some(u) => {
                        self.cursor = self.state.units[u].pos;
                        self.camera.center_on(self.cursor);
                        self.ui.select(&self.state, &self.pack, u);
                        ctx.sfx(sfx::CURSOR);
                    }
                    None => ctx.sfx(sfx::ERROR),
                }
            }
        }

        // Confirm: key on the cursor, tap on a tile.
        let tap_tile = ctx.input.tap().and_then(|p| self.camera.tile_at(p));
        let mut confirm_at = None;
        if ctx.input.confirm_key() {
            confirm_at = Some(self.cursor);
        } else if let Some(t) = tap_tile {
            let previewed = self.cursor_at_press == Some(t);
            self.cursor = t;
            // In target modes the first tap only previews (touch has no hover).
            if !self.is_target_mode() || previewed {
                confirm_at = Some(t);
            } else {
                ctx.sfx(sfx::CURSOR);
            }
        }
        if let Some(at) = confirm_at {
            let by_tap = !ctx.input.confirm_key();
            let target_mode = self.is_target_mode();
            let before = std::mem::discriminant(&self.ui.mode);
            let req = self.ui.confirm(&self.state, &self.pack, at);
            if req == Request::Invalid && target_mode && by_tap && self.touch_seen {
                // Touch has no cancel key or right click: confirming a tile that is not a
                // target (tapping it a second time) steps back to the menu the target mode
                // came from, and from there a tap outside the menu steps back further. Mouse
                // play keeps the error sound (right click cancels).
                self.step_back(ctx);
                return;
            }
            let changed = std::mem::discriminant(&self.ui.mode) != before;
            if changed || matches!(req, Request::Apply(_)) {
                ctx.sfx(sfx::CONFIRM);
            }
            self.handle_request(ctx, req);
            return;
        }
        if key_cancel || right_click {
            self.step_back(ctx);
        }
    }

    /// Cancel one level of the command flow on the map.
    fn step_back(&mut self, ctx: &mut Ctx) {
        // Stepping back puts the cursor on the unit that was selected.
        if let Some(u) = self.ui.unit() {
            ctx.sfx(sfx::CANCEL);
            self.cursor = self.state.units[u].pos;
        }
        let req = self.ui.cancel(&self.state, &self.pack);
        self.handle_request(ctx, req);
    }
}

/// Sides in the order of the unit list tabs.
const UNIT_TABS: [Side; 3] = [Side::Player, Side::Ally, Side::Enemy];
/// Width of the unit list, centred in the map viewport.
const UNIT_LIST_W: f32 = 300.0;

/// Rectangle of a side tab of the unit list (index into [`UNIT_TABS`]) over the map viewport
/// `vp`.
fn unit_tab_rect(vp: Rect, i: usize) -> Rect {
    let x0 = vp.x + (vp.w - UNIT_LIST_W) / 2.0;
    Rect::new(x0 + 4.0 + i as f32 * 62.0, vp.y + 6.0, 58.0, 19.0)
}

/// Index of the unit list tab under a tap.
fn unit_tab_at(vp: Rect, p: Vec2) -> Option<usize> {
    (0..UNIT_TABS.len()).find(|&i| unit_tab_rect(vp, i).contains(p))
}

/// Window of the unit list around its menu (the side tabs sit in its top strip).
fn unit_list_frame(menu: Rect) -> Rect {
    Rect::new(menu.x - 4.0, menu.y - 26.0, menu.w + 8.0, menu.h + 30.0)
}

/// A tap landed outside `area` this frame.
fn tapped_outside(ctx: &Ctx, area: Rect) -> bool {
    ctx.input.tap().is_some_and(|p| !area.contains(p))
}

impl Screen for BattleScreen {
    fn name(&self) -> &'static str {
        "battle"
    }

    fn on_enter(&mut self, ctx: &mut Ctx, how: Enter) {
        match how {
            Enter::Fresh => {
                self.start_loading(ctx);
                self.play_phase_music(ctx, self.state.phase);
            }
            Enter::Resumed => {
                match self.waiting.take() {
                    Some(Waiting::Drama) => self.events.resume(),
                    Some(Waiting::Screen) | None => {}
                }
                // A drama or the settings may have changed the music.
                if self.state.outcome.is_none() {
                    self.play_phase_music(ctx, self.scene.hud.phase);
                }
            }
        }
    }

    fn update(&mut self, ctx: &mut Ctx) -> Transition {
        let t = self.update_frame(ctx);
        // After the frame's events and input: the tiles drawn this frame already follow the
        // moves and arrivals the frame finished (before, they lagged one frame behind).
        self.refresh_danger(ctx);
        t
    }

    fn draw(&self, ctx: &Ctx) {
        let screen = ctx.gfx.screen();
        let vp = self.camera.viewport;
        fill_rect(screen, Color::from_hex(0x0b0f1c));
        self.draw_map(ctx);
        let player_turn = matches!(self.stage, Stage::Battle)
            && self.events.is_idle()
            && self.state.phase == Side::Player
            && self.state.outcome.is_none();
        if player_turn {
            if self.shows_danger(ctx) {
                self.draw_danger();
            }
            self.draw_highlights(ctx);
        }
        self.draw_units(ctx);
        self.draw_fx(ctx);
        let show_cursor = matches!(self.stage, Stage::Battle)
            && self.state.outcome.is_none()
            && (player_turn || self.ai.is_some());
        if show_cursor {
            let color = if self.is_target_mode() {
                Color::from_hex(0xff6a5a)
            } else {
                theme::CURSOR_ARROW
            };
            // During AI phases the cursor rides on the acting unit.
            let at = match self.ai {
                Some(a) => self.camera.map_to_screen(self.scene.views[a.unit].pos),
                None => self.camera.tile_screen(self.cursor),
            };
            hud::draw_cursor(at, self.tile(), ctx.time, color);
        }
        for f in &self.scene.floats {
            hud::draw_float(&ctx.gfx, self.camera.map_to_screen(f.at), self.tile(), f);
        }
        let gold = ctx.session.as_ref().map_or(0, |s| s.campaign.gold);
        match &self.frame {
            Some(f) => {
                hud::draw_battle_frame(ctx, ctx.media.texture(&f.image).as_ref(), vp);
                hud::draw_frame_title(
                    ctx,
                    frame_rect(f.title),
                    frame_rect(f.status),
                    &self.def().name,
                    &self.scene.hud,
                    self.state.turn_limit,
                    gold,
                );
                if let Some(area) = f.weather {
                    let r = frame_rect(area);
                    draw_icon(
                        ctx,
                        text::weather_icon(self.scene.hud.weather),
                        vec2((r.center().x - 8.0).round(), (r.center().y - 8.0).round()),
                    );
                }
                // The button under the mouse lights up while the buttons can be used (touch
                // leaves no pointer over them).
                let usable = player_turn
                    && matches!(self.panel, Panel::None)
                    && self.dialog.is_none()
                    && self.waiting.is_none()
                    && self.mode_menu.is_none()
                    && matches!(self.ui.mode, Mode::Browse)
                    && !self.touch_seen;
                if let Some(p) = ctx.input.pointer().filter(|_| usable) {
                    if let Some((_, r)) = frame_buttons(f).find(|(_, r)| r.contains(p)) {
                        draw_highlight(r, false, ctx.time);
                    }
                }
            }
            None => hud::draw_top_bar(
                ctx,
                &self.def().name,
                &self.scene.hud,
                self.state.turn_limit,
                gold,
            ),
        }

        match &self.stage {
            Stage::Title { age } => {
                let sub = if self.fresh {
                    self.def().location.clone()
                } else {
                    format!("{} — 이어서", self.def().location)
                };
                hud::draw_title_card(ctx, &self.def().name, &sub, *age);
                return;
            }
            Stage::Objective => {
                fill_rect(screen, Color::new(0.0, 0.0, 0.02, 0.45));
                self.draw_objective(ctx, "Z / 클릭 — 출진");
                return;
            }
            Stage::Result { lines, title } => {
                fill_rect(screen, Color::new(0.0, 0.0, 0.02, 0.5));
                hud::draw_text_window(ctx, title, lines, "Z / 클릭 — 계속");
                return;
            }
            Stage::Battle => {}
        }

        let modal =
            !matches!(self.panel, Panel::None) || self.dialog.is_some() || self.waiting.is_some();
        let list_open = matches!(
            self.mode_menu,
            Some((MenuKind::Strategies | MenuKind::Items, _))
        );
        if player_turn && !modal && !list_open {
            // With the command menu open the acting unit's panel stays visible.
            let forecast = self.mode_menu.is_none() && self.has_forecast();
            self.draw_panels(ctx, !forecast);
            if forecast {
                self.draw_forecast(ctx);
            }
        } else if (!self.events.is_idle() || self.ai.is_some())
            && self.scene.banner.is_none()
            && self.scene.popup.is_none()
        {
            self.draw_panels(ctx, false);
        }
        if player_turn {
            self.draw_mode_menu(ctx);
            self.draw_panel(ctx);
        }
        if let Some(c) = &self.scene.caption {
            hud::draw_caption(ctx, &c.text, c.age, vp);
        }
        if let Some(b) = &self.scene.banner {
            hud::draw_banner(ctx, b, vp);
        }
        if let Some(p) = &self.scene.popup {
            let below = p.unit.is_some_and(|u| {
                let y = self.camera.map_to_screen(self.scene.views[u].pos).y;
                y < vp.y + vp.h * 0.5
            });
            hud::draw_popup(ctx, p, &self.state, vp, below);
        }
        if let Some(v) = self.scene.outcome {
            hud::draw_outcome(ctx, v, self.scene.outcome_age);
        }
        if let Some((dialog, _)) = &self.dialog {
            fill_rect(screen, Color::new(0.0, 0.0, 0.0, 0.35));
            dialog.draw(ctx);
        }
        if !self.events.is_idle()
            && self.state.phase != Side::Player
            && self.scene.outcome.is_none()
        {
            ctx.gfx.text_aligned(
                "Z 길게 누르기: 빨리 감기",
                0.0,
                screen.h - 13.0,
                screen.w - 6.0,
                Align::Right,
                TextStyle::small(theme::TEXT_DIM).shadow(theme::TEXT_SHADOW),
            );
        }
    }

    fn resume_point(&self, _ctx: &Ctx) -> Option<ResumePoint> {
        self.report()
    }
}

impl BattleScreen {
    /// Input: nothing but the screen itself. Output: the battle for a quick save, or `None`
    /// while a new battle's title card or objective window is up.
    ///
    /// Why the state is saved as it is even while a move or an enemy attack is animating: the
    /// state is the source of truth and is already past the animation (`apply` runs the rules
    /// first and animates the events after), which loading simply snaps to. What the animation
    /// still owed the player is the scenes of its queued drama beats, so those are kept.
    /// Why `None` before the battle begins: `state.begin` has not run, so the saved state would
    /// load as a battle with no intro; the campaign node alone loads it from the top. A battle
    /// that was itself resumed is different: its state is a real one, even under its title card.
    fn report(&self) -> Option<ResumePoint> {
        if self.fresh && matches!(self.stage, Stage::Title { .. } | Stage::Objective) {
            return None;
        }
        let mut scene = None;
        let mut pending_scenes = Vec::new();
        for queued in &self.queued_scenes {
            match queued {
                QueuedScene::Resume(resume) => scene = Some(resume.clone()),
                QueuedScene::Fresh(id) => pending_scenes.push(id.clone()),
            }
        }
        pending_scenes.extend(self.events.pending_dramas());
        Some(ResumePoint::Battle {
            state: Box::new(self.state.clone()),
            pending_scenes,
            scene,
        })
    }
}

/// The terrain id under each officer of `state` on the map, for the duels of its scenes
/// (`@duel ... terrain`). Retreated officers count at their last cell, after the ones on the
/// map: a duel's loser retreats in the actions of the same event, which have all been applied
/// when its scene plays (one that retreated turns earlier counts too, a case the converted
/// duels do not have).
fn officer_terrain(pack: &Pack, state: &BattleState) -> BTreeMap<String, String> {
    let mut terrain = BTreeMap::new();
    for on_map in [true, false] {
        for u in &state.units {
            let wanted = if on_map {
                u.is_active()
            } else {
                u.state == hero_core::battle::UnitState::Retreated
            };
            let (Some(officer), true) = (u.officer.as_ref(), wanted) else {
                continue;
            };
            if let Some(t) = state.terrain_at(pack, u.pos) {
                terrain
                    .entry(officer.to_string())
                    .or_insert_with(|| t.id.to_string());
            }
        }
    }
    terrain
}

impl BattleScreen {
    /// One frame of [`Screen::update`], before the danger tiles are brought up to date.
    fn update_frame(&mut self, ctx: &mut Ctx) -> Transition {
        self.poll_meta(ctx);
        let dt = ctx.dt;
        self.pointer_rest = if ctx.input.pointer_moved() {
            0.0
        } else {
            self.pointer_rest + dt
        };
        let speed = self.speed(ctx);
        self.camera.update(dt);
        self.scene.enhanced = ctx.settings.battle_fx == crate::settings::BattleFx::Enhanced;
        self.scene.tick(dt * speed);
        self.camera.set_shake(self.scene.shake_offset());

        match &mut self.stage {
            Stage::Title { age } => {
                *age += dt;
                let skip = *age > 0.4 && ctx.input.confirm();
                if *age >= TITLE_SECONDS || skip {
                    ctx.input.consume();
                    if self.fresh {
                        self.stage = Stage::Objective;
                        ctx.sfx(sfx::CONFIRM);
                    } else {
                        self.stage = Stage::Battle;
                        // Resumed mid-battle: announce whose phase it is.
                        let ev = [BattleEvent::PhaseStart {
                            side: self.state.phase,
                            turn: self.state.turn,
                        }];
                        self.events
                            .push(anim::plan(&ev, &self.state, &self.pack, &self.meta.fx));
                    }
                }
                return Transition::None;
            }
            Stage::Objective => {
                if ctx.input.confirm() || ctx.input.cancel() {
                    ctx.input.consume();
                    ctx.sfx(sfx::CONFIRM);
                    self.begin_battle(ctx);
                }
                return Transition::None;
            }
            Stage::Result { .. } => {
                if ctx.input.confirm() {
                    ctx.input.consume();
                    ctx.sfx(sfx::CONFIRM);
                    return Transition::Flow(Flow::BattleEnded(Box::new(self.state.clone())));
                }
                return Transition::None;
            }
            Stage::Battle => {}
        }

        // Scenes a quick save left over: shown before the battle goes on (the overlay closing
        // brings the screen back here for the next one).
        if !self.queued_scenes.is_empty() {
            return self.open_queued_scene(ctx);
        }
        // A batch whose last beat was a drama ran dry when the overlay closed (`resume`).
        self.settle_events(ctx);
        // Animations first; input waits until they are done.
        if !self.events.is_idle() {
            let skip = ctx.input.confirm();
            let mut cues = std::mem::take(&mut self.cues);
            self.events
                .update(dt * speed, skip, &mut self.scene, &self.meta.fx, &mut cues);
            self.cues = cues;
            let t = self.handle_cues(ctx);
            if skip {
                ctx.input.consume();
            }
            self.settle_events(ctx);
            return t;
        }
        if self.state.outcome.is_some() {
            self.open_result();
            return Transition::None;
        }
        if self.state.phase != Side::Player {
            self.ai_update(ctx, dt * speed);
            return Transition::None;
        }
        self.player_update(ctx, dt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screens::drama::OVERLAY_TOOL_TOP;
    use hero_core::battle::UnitState;
    use hero_core::drama::DramaRunner;
    use hero_core::save::SceneKind;

    fn screen(fresh: bool) -> BattleScreen {
        let (pack, state) = testutil::sishui();
        BattleScreen::new(pack, state, fresh, Vec2::new(480.0, 270.0))
    }

    fn scene_record() -> SceneResume {
        SceneResume {
            kind: SceneKind::Overlay,
            runner: DramaRunner {
                scene: "p1_intro".into(),
                pc: 2,
                pending_choice: None,
                finished: false,
            },
            stage: Vec::new(),
            shown: None,
            last_text: None,
            choice: None,
            bgm: None,
            terrain: BTreeMap::new(),
            backlog: Vec::new(),
            fingerprint: None,
        }
    }

    fn battle_report(screen: &BattleScreen) -> (BattleState, Vec<String>, Option<SceneResume>) {
        match screen.report() {
            Some(ResumePoint::Battle {
                state,
                pending_scenes,
                scene,
            }) => (*state, pending_scenes, scene.map(|s| *s)),
            other => panic!("expected a battle report, got {other:?}"),
        }
    }

    /// A new battle has not begun under its title card and objective window: nothing to save but
    /// the campaign, which loads it from the top. From then on the state is saved as it is.
    #[test]
    fn a_new_battle_reports_only_once_it_has_begun() {
        let mut s = screen(true);
        assert!(s.report().is_none());
        s.stage = Stage::Objective;
        assert!(s.report().is_none());
        s.stage = Stage::Battle;
        let (state, pending, scene) = battle_report(&s);
        assert_eq!(state, s.state);
        assert!(pending.is_empty() && scene.is_none());
    }

    /// A battle that was itself resumed has a real state even while its title card is up:
    /// dropping it there would lose the battle.
    #[test]
    fn a_resumed_battle_is_reported_under_its_title_card() {
        let s = screen(false);
        assert!(matches!(s.stage, Stage::Title { .. }));
        let (state, ..) = battle_report(&s);
        assert_eq!(state, s.state);
    }

    /// The scenes the animation had queued are kept, after the ones a quick load has yet to
    /// show, and the scene that was half-way is reported as such.
    #[test]
    fn queued_scenes_are_reported_in_order() {
        let mut s = screen(false);
        s.stage = Stage::Battle;
        s.queued_scenes = VecDeque::from([
            QueuedScene::Resume(Box::new(scene_record())),
            QueuedScene::Fresh("p1_rein".into()),
        ]);
        let beats = anim::plan(
            &[
                BattleEvent::Drama {
                    scene: "p1_duel".into(),
                },
                BattleEvent::Drama {
                    scene: "p1_outro".into(),
                },
            ],
            &s.state,
            &s.pack,
            &BTreeMap::new(),
        );
        s.events.push(beats);

        let (_, pending, scene) = battle_report(&s);
        assert_eq!(pending, vec!["p1_rein", "p1_duel", "p1_outro"]);
        assert_eq!(scene, Some(scene_record()));
    }

    #[test]
    fn duels_see_the_terrain_under_the_officers_and_under_those_just_gone() {
        let (pack, mut state) = testutil::sishui();
        let officers: Vec<usize> = state
            .units
            .iter()
            .filter(|u| u.officer.is_some() && u.is_active())
            .map(|u| u.id)
            .take(2)
            .collect();
        assert_eq!(officers.len(), 2);
        let names: Vec<String> = officers
            .iter()
            .map(|&id| state.units[id].officer.as_ref().unwrap().to_string())
            .collect();
        let name = |id: usize| names[officers.iter().position(|&o| o == id).unwrap()].clone();
        let under = |state: &BattleState, id: usize| {
            state
                .terrain_at(&pack, state.units[id].pos)
                .unwrap()
                .id
                .to_string()
        };
        let map = officer_terrain(&pack, &state);
        assert_eq!(map[&name(officers[0])], under(&state, officers[0]));
        // Retreated by the event that plays the duel: still at their last cell.
        state.units[officers[1]].state = UnitState::Retreated;
        let map = officer_terrain(&pack, &state);
        assert_eq!(map[&name(officers[1])], under(&state, officers[1]));
        // Not yet on the map: not there.
        state.units[officers[1]].state = UnitState::Hidden;
        assert!(!officer_terrain(&pack, &state).contains_key(&name(officers[1])));
    }

    /// The default (and smallest allowed), VGA and the largest allowed canvas.
    const CANVASES: [Vec2; 3] = [
        crate::gfx::DEFAULT_CANVAS,
        Vec2::new(640.0, 480.0),
        Vec2::new(1280.0, 800.0),
    ];

    /// Drama overlays over the battle (intro, events, outro) put their toolbar at
    /// `OVERLAY_TOOL_TOP`, below the top bar of the screen underneath: the battle HUD's top bar
    /// must end at least 4 pixels above it, so the bar stays readable and the buttons tappable.
    #[test]
    fn top_bar_leaves_room_for_the_drama_overlay_toolbar() {
        for canvas in CANVASES {
            let top_bar_bottom = viewport(canvas, None).y;
            assert_eq!(top_bar_bottom, hud::TOP_BAR_H);
            assert!(
                top_bar_bottom + 4.0 <= OVERLAY_TOOL_TOP,
                "the battle top bar ({top_bar_bottom} px) runs into the drama overlay toolbar \
                 at {OVERLAY_TOOL_TOP} px"
            );
        }
    }

    #[test]
    fn windows_in_the_frame_column_stay_off_the_map() {
        let (info, map) = (
            Rect::new(448.0, 74.0, 176.0, 196.0),
            Rect::new(16.0, 32.0, 416.0, 352.0),
        );
        let canvas = vec2(640.0, 400.0);
        // Centred when it fits, off the map when wider, inside the canvas before all.
        assert_eq!(column_x(info, 96.0, map, canvas), 488.0);
        assert_eq!(column_x(info, 186.0, map, canvas), 443.0);
        assert_eq!(column_x(info, 208.0, map, canvas), 432.0);
        assert_eq!(column_x(info, 210.0, map, canvas), 430.0);
    }

    #[test]
    fn viewport_and_unit_tabs_follow_the_canvas() {
        // The base pack's layout.
        let vp = viewport(crate::gfx::DEFAULT_CANVAS, None);
        assert_eq!(vp, Rect::new(0.0, 16.0, 480.0, 254.0));
        assert_eq!(unit_tab_rect(vp, 0), Rect::new(94.0, 22.0, 58.0, 19.0));
        for canvas in CANVASES {
            let vp = viewport(canvas, None);
            assert_eq!((vp.right(), vp.bottom()), (canvas.x, canvas.y));
            let (first, last) = (unit_tab_rect(vp, 0), unit_tab_rect(vp, 2));
            // The tabs sit inside the centred unit list.
            assert!(first.x > (canvas.x - UNIT_LIST_W) / 2.0);
            assert!(last.right() < (canvas.x + UNIT_LIST_W) / 2.0);
            assert_eq!(unit_tab_at(vp, last.center()), Some(2));
        }
    }

    #[test]
    fn frame_buttons_are_found_where_the_frame_puts_them() {
        let mut frame: BattleFrame = toml::from_str(
            "image = \"ui/frame\"\nmap = [16, 32, 416, 352]\ninfo = [448, 74, 176, 196]\n\
             title = [224, 8, 174, 16]\nstatus = [448, 34, 78, 28]\nmenu = [15, 7, 66, 18]\n\
             allies = [528, 31, 33, 34]\nenemies = [560, 31, 33, 34]\n",
        )
        .unwrap();
        assert_eq!(
            frame_button_at(&frame, vec2(40.0, 15.0)),
            Some(FrameButton::Menu)
        );
        assert_eq!(
            frame_button_at(&frame, vec2(540.0, 40.0)),
            Some(FrameButton::Allies)
        );
        assert_eq!(
            frame_button_at(&frame, vec2(580.0, 40.0)),
            Some(FrameButton::Enemies)
        );
        assert_eq!(frame_button_at(&frame, vec2(200.0, 200.0)), None);
        // A frame without buttons has none.
        frame.menu = None;
        assert_eq!(frame_button_at(&frame, vec2(40.0, 15.0)), None);
    }
}
