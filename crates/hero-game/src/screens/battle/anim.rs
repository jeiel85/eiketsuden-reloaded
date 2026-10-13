//! Event animation: turns the [`BattleEvent`]s returned by `BattleState::apply` into timed
//! [`Beat`]s and plays them one after another on the drawable [`Scene`].
//!
//! `apply` changes the battle state at once, so the screen never draws units straight from the
//! state: it draws [`UnitView`]s, which the beats move, hurt, level up and remove step by step.
//! When the queue runs dry the screen snaps every view back to the state ([`UnitView::sync`]),
//! which also corrects anything a beat did not touch.
//!
//! Beats never touch the screen directly; side effects the screen must carry out (sounds, music,
//! camera moves, drama overlays) are emitted as [`Cue`]s. A `Drama` beat blocks the queue until
//! the screen calls [`EventPlayer::resume`] after the overlay has closed.
//!
//! Positions and motion are measured in **map tiles** (`(x, y)` is the top-left corner of tile
//! `(x, y)`), so the animation does not depend on the tileset's tile size; the screen multiplies
//! by the tile size when it draws. Motion amounts are designed in pixels of a [`REF_TILE`]
//! pixel tile and scale with the tile size.

use super::sprites::{FxDef, Pose};
use super::text;
use crate::audio::sfx;
use hero_core::battle::{BattleEvent, BattleState, StrategyHit, Unit, UnitId, Weather};
use hero_core::battledef::Side;
use hero_core::data::StatusKind;
use hero_core::geom::{Dir, Pos};
use hero_core::pack::Pack;
use macroquad::prelude::*;
use std::collections::{BTreeMap, VecDeque};

/// Seconds per tile of a walk.
pub const STEP_SECONDS: f32 = 0.1;
/// Moment of impact inside a strike.
const IMPACT: f32 = 0.14;
/// Strike length.
const STRIKE_END: f32 = 0.9;
/// Tile size (pixels) the motion amounts below are designed for; they are divided by it to get
/// tile units.
pub const REF_TILE: f32 = 16.0;
/// Pixels (of a [`REF_TILE`] tile) an attacker lunges towards its target.
const LUNGE: f32 = 5.0;
/// Fallback length of a missing effect strip.
const DEFAULT_FX_SECONDS: f32 = 0.6;
/// Enhanced presentation (D25 X5): seconds the map shakes after a heavy or defeating hit.
pub const SHAKE_SECONDS: f32 = 0.25;
/// Enhanced presentation: largest shake offset in canvas pixels.
pub const SHAKE_PX: f32 = 3.0;
/// Enhanced presentation: a hit taking at least this share of the target's maximum HP shakes the
/// map (a defeating hit always does).
pub const SHAKE_SHARE: f32 = 0.25;

/// Knock-back of a hit in pixels of a [`REF_TILE`] tile, after the PC original: below 100
/// damage no reaction, 100–299 a step back, from 300 on pushed further the bigger the hit.
/// Defeated units fly off separately.
pub fn knockback(damage: i32) -> f32 {
    match damage {
        d if d < 100 => 0.0,
        d if d < 300 => 3.0,
        d => (6 + (d - 300) / 60).min(16) as f32,
    }
}

fn dir_vec(from: Pos, to: Pos) -> Vec2 {
    let d = vec2((to.x - from.x) as f32, (to.y - from.y) as f32);
    if d.length_squared() > 0.0 {
        d.normalize()
    } else {
        vec2(0.0, 1.0)
    }
}

/// Map position (in tiles) of a tile's top-left corner.
fn tile_pos(p: Pos) -> Vec2 {
    vec2(p.x as f32, p.y as f32)
}

/// A motion amount designed in pixels of a [`REF_TILE`] tile, in tiles.
fn ref_px(px: f32) -> f32 {
    px / REF_TILE
}

/// What the screen draws for one unit.
#[derive(Debug, Clone, PartialEq)]
pub struct UnitView {
    pub visible: bool,
    pub alpha: f32,
    /// Map position (tiles) of the tile the unit stands on (between tiles while walking).
    pub pos: Vec2,
    /// Animation offset in tiles (lunge, knock-back, fly-off).
    pub offset: Vec2,
    pub facing: Dir,
    pub pose: Pose,
    pub class: String,
    pub side: Side,
    pub hp: f32,
    pub max_hp: i32,
    pub mp: i32,
    pub max_mp: i32,
    pub morale: i32,
    pub level: u32,
    pub exp: u32,
    pub confused: bool,
    /// Greyed: acted in its own phase.
    pub acted: bool,
    /// Seconds of hit flicker left.
    pub flash: f32,
    /// Direction of the blow that defeated the unit (it flies off that way when it retreats).
    pub knocked: Option<Vec2>,
}

impl UnitView {
    pub fn new(u: &Unit, phase: Side) -> UnitView {
        let mut v = UnitView {
            visible: false,
            alpha: 1.0,
            pos: Vec2::ZERO,
            offset: Vec2::ZERO,
            facing: u.facing,
            pose: Pose::Idle,
            class: String::new(),
            side: u.side,
            hp: 0.0,
            max_hp: 1,
            mp: 0,
            max_mp: 0,
            morale: 0,
            level: 1,
            exp: 0,
            confused: false,
            acted: false,
            flash: 0.0,
            knocked: None,
        };
        v.sync(u, phase);
        v
    }

    /// Snap to the unit's state.
    pub fn sync(&mut self, u: &Unit, phase: Side) {
        self.visible = u.is_active();
        self.alpha = 1.0;
        self.pos = tile_pos(u.pos);
        self.offset = Vec2::ZERO;
        self.facing = u.facing;
        self.pose = Pose::Idle;
        self.class = u.class.clone();
        self.side = u.side;
        self.hp = u.hp as f32;
        self.max_hp = u.max_hp;
        self.mp = u.mp;
        self.max_mp = u.max_mp;
        self.morale = u.morale;
        self.level = u.level;
        self.exp = u.exp;
        self.confused = u.has_status(StatusKind::Confused);
        self.acted = u.side == phase && u.acted;
        self.flash = 0.0;
        self.knocked = None;
    }

    /// Tile the view stands on (rounded while walking).
    pub fn tile(&self) -> Pos {
        Pos::new(self.pos.x.round() as i32, self.pos.y.round() as i32)
    }
}

/// Colour class of a floating text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloatKind {
    Damage,
    /// HP damage in the enhanced presentation (D25 X5): red, with a minus sign.
    Hit,
    Heal,
    Mp,
    Morale,
    Exp,
    Info,
    Miss,
}

/// Rising text above a unit (damage numbers, `+6 EXP`, `퇴각` ...).
#[derive(Debug, Clone, PartialEq)]
pub struct FloatText {
    pub text: String,
    pub kind: FloatKind,
    /// Map position (tiles) of the tile the text belongs to.
    pub at: Vec2,
    /// Extra lines stack upwards.
    pub row: u8,
    pub age: f32,
    pub life: f32,
}

/// An effect strip playing on a tile.
#[derive(Debug, Clone, PartialEq)]
pub struct FxSpawn {
    pub key: String,
    /// Map position (tiles) of the tile centre.
    pub center: Vec2,
    pub age: f32,
    pub life: f32,
}

/// Colour of a banner band.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Player,
    Ally,
    Enemy,
    Neutral,
    Good,
}

impl Tone {
    pub fn of(side: Side) -> Tone {
        match side {
            Side::Player => Tone::Player,
            Side::Ally => Tone::Ally,
            Side::Enemy => Tone::Enemy,
        }
    }
}

/// A caption across the middle of the map.
#[derive(Debug, Clone, PartialEq)]
pub struct BannerView {
    pub title: String,
    pub subtitle: Option<String>,
    pub icon: Option<&'static str>,
    pub tone: Tone,
    pub age: f32,
    pub duration: f32,
}

/// Seconds a banner slides in / fades out.
pub const BANNER_IN: f32 = 0.25;
pub const BANNER_OUT: f32 = 0.3;

impl BannerView {
    /// Opacity 0..=1 at the current age.
    pub fn alpha(&self) -> f32 {
        let a_in = (self.age / BANNER_IN).min(1.0);
        let a_out = ((self.duration - self.age) / BANNER_OUT).clamp(0.0, 1.0);
        a_in.min(a_out)
    }
}

/// A small window with a title and lines (level up, promotion, new strategy).
#[derive(Debug, Clone, PartialEq)]
pub struct Popup {
    pub title: String,
    pub lines: Vec<String>,
    /// Unit the popup is about (its portrait is shown).
    pub unit: Option<UnitId>,
    pub age: f32,
}

/// Short caption naming an action (`관우 · 초열`).
#[derive(Debug, Clone, PartialEq)]
pub struct Caption {
    pub text: String,
    pub age: f32,
    pub life: f32,
}

/// Top bar values as the animation has reached them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HudView {
    pub turn: u32,
    pub phase: Side,
    pub weather: Weather,
    pub gold_found: i64,
}

/// Everything the beats animate.
#[derive(Debug, Clone)]
pub struct Scene {
    pub views: Vec<UnitView>,
    pub floats: Vec<FloatText>,
    pub fx: Vec<FxSpawn>,
    pub banner: Option<BannerView>,
    pub popup: Option<Popup>,
    pub caption: Option<Caption>,
    /// `Some(victory)` while the outcome banner is shown.
    pub outcome: Option<bool>,
    pub outcome_age: f32,
    pub hud: HudView,
    /// The enhanced hit presentation is chosen (`Settings::battle_fx`, D25 X5); the screen sets
    /// it every frame, so a change applies from the next hit on.
    pub enhanced: bool,
    /// Seconds of map shake left (enhanced presentation only).
    pub shake: f32,
}

impl Scene {
    pub fn new(state: &BattleState) -> Scene {
        Scene {
            views: state
                .units
                .iter()
                .map(|u| UnitView::new(u, state.phase))
                .collect(),
            floats: Vec::new(),
            fx: Vec::new(),
            banner: None,
            popup: None,
            caption: None,
            outcome: None,
            outcome_age: 0.0,
            hud: HudView {
                turn: state.turn,
                phase: state.phase,
                weather: state.weather,
                gold_found: state.gold_found,
            },
            enhanced: false,
            shake: 0.0,
        }
    }

    /// Snap every unit and the top bar to the state (after the queue ran dry, after an undo).
    pub fn sync(&mut self, state: &BattleState) {
        self.views.resize_with(state.units.len(), || {
            UnitView::new(&state.units[0], state.phase)
        });
        for (v, u) in self.views.iter_mut().zip(&state.units) {
            v.sync(u, state.phase);
        }
        self.hud = HudView {
            turn: state.turn,
            phase: state.phase,
            weather: state.weather,
            gold_found: state.gold_found,
        };
    }

    /// Age floating texts, effects and the caption; drop finished ones.
    pub fn tick(&mut self, dt: f32) {
        for f in &mut self.floats {
            f.age += dt;
        }
        self.floats.retain(|f| f.age < f.life);
        for f in &mut self.fx {
            f.age += dt;
        }
        self.fx.retain(|f| f.age < f.life);
        if let Some(c) = &mut self.caption {
            c.age += dt;
            if c.age >= c.life {
                self.caption = None;
            }
        }
        for v in &mut self.views {
            v.flash = (v.flash - dt).max(0.0);
        }
        if self.outcome.is_some() {
            self.outcome_age += dt;
        }
        self.shake = (self.shake - dt).max(0.0);
    }

    /// Offset (canvas pixels) of the map and its units while the map shakes: alternating
    /// left/right and up/down every 30 ms of animation time, fading out.
    pub fn shake_offset(&self) -> Vec2 {
        if self.shake <= 0.0 {
            return Vec2::ZERO;
        }
        let amp = (SHAKE_PX * self.shake / SHAKE_SECONDS).round();
        let step = ((SHAKE_SECONDS - self.shake) / 0.03) as i32;
        let x = if step % 2 == 0 { amp } else { -amp };
        let y = if step % 4 < 2 { -amp / 2.0 } else { amp / 2.0 };
        vec2(x, y.round())
    }

    /// HP damage shown over `unit`: its number, and in the enhanced presentation a red `-123`
    /// plus a shake when the hit takes [`SHAKE_SHARE`] of the unit's maximum HP or defeats it.
    /// Call before the unit's HP bar starts draining.
    fn damage_float(&mut self, unit: UnitId, damage: i32) {
        if !self.enhanced {
            self.float(unit, damage.to_string(), FloatKind::Damage);
            return;
        }
        let v = &self.views[unit];
        let lethal = v.hp - damage as f32 <= 0.0;
        if damage > 0 && (lethal || damage as f32 >= SHAKE_SHARE * v.max_hp as f32) {
            self.shake = SHAKE_SECONDS;
        }
        let text = if damage > 0 {
            format!("-{damage}")
        } else {
            damage.to_string()
        };
        self.float(unit, text, FloatKind::Hit);
    }

    fn float(&mut self, unit: UnitId, text: impl Into<String>, kind: FloatKind) {
        let at = self.views[unit].pos;
        // Stack above the texts still shown on the same tile.
        let row = self
            .floats
            .iter()
            .filter(|f| f.at == at && f.age < f.life)
            .count()
            .min(3) as u8;
        self.floats.push(FloatText {
            text: text.into(),
            kind,
            at,
            row,
            age: 0.0,
            life: 1.1,
        });
    }

    /// Play effect `key` on the tile whose top-left corner is at map position `tile`.
    fn spawn_fx(&mut self, key: &str, tile: Vec2, fx: &BTreeMap<String, FxDef>) {
        if let Some(def) = fx.get(key) {
            self.fx.push(FxSpawn {
                key: key.to_string(),
                center: tile + vec2(0.5, 0.5),
                age: 0.0,
                life: def.duration(),
            });
        }
    }
}

/// Side effects of beats for the screen to carry out.
#[derive(Debug, Clone, PartialEq)]
pub enum Cue {
    Sfx(&'static str),
    /// Keep this tile comfortably inside the view.
    Follow(Pos),
    /// Centre the view on this tile.
    Center(Pos),
    /// Switch to the music of this side's phase.
    PhaseMusic(Side),
    /// Victory (`true`) or defeat jingle.
    Jingle(bool),
    /// Play a drama scene as an overlay, with the officers' terrain as it began
    /// ([`BattleEvent::Drama`]); the queue waits for [`EventPlayer::resume`].
    Drama(String, BTreeMap<String, String>),
    /// Show the map with this tile's new terrain (`set_terrain`).
    Terrain(Pos),
}

/// One timed step of the event animation.
#[derive(Debug, Clone, PartialEq)]
pub struct Beat {
    pub kind: BeatKind,
    t: f32,
    started: bool,
    /// Internal progress marker (impact reached, results shown, ...).
    stage: u8,
    /// HP bars this beat is moving.
    drains: Vec<Drain>,
    /// Last walk step announced (camera follow / step sound).
    mark: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BeatKind {
    Banner(BannerView),
    Phase {
        side: Side,
        turn: u32,
        limit: u32,
    },
    Weather(Weather),
    Walk {
        unit: UnitId,
        path: Vec<Pos>,
    },
    Strike {
        attacker: UnitId,
        defender: UnitId,
        damage: i32,
        morale_loss: i32,
        ranged: bool,
        counter: bool,
    },
    Strategy {
        caster: UnitId,
        caption: String,
        fx: Option<String>,
        fx_len: f32,
        hits: Vec<StrategyHit>,
        /// MP the caster pays (0 when cast from a scroll).
        mp: i32,
        /// The strategy changes morale on purpose (morale-up/down); otherwise the morale lost
        /// to its damage is not shown, as for physical hits.
        morale_effect: bool,
    },
    Item {
        user: UnitId,
        target: UnitId,
        caption: String,
        healed: i32,
        morale: i32,
        /// Strategy scroll: the strategy beat that follows does the rest.
        scroll: bool,
    },
    Regen(Vec<(UnitId, i32, i32, i32)>),
    Confused(UnitId),
    Recovered(UnitId),
    Exp(Vec<(UnitId, u32)>),
    LevelUp {
        unit: UnitId,
        popup: Popup,
        level: u32,
        hp_gain: i32,
        mp_gain: i32,
    },
    Popup(Popup),
    Promoted {
        unit: UnitId,
        popup: Popup,
        to: String,
    },
    Retreat(UnitId),
    Spawn(Vec<UnitId>),
    Treasure {
        banner: BannerView,
        gold: i64,
    },
    Drama(String, BTreeMap<String, String>),
    /// A tile's terrain changed: the view centres on it while the map shows the change.
    Terrain(Pos),
    Outcome(bool),
}

impl Beat {
    pub fn new(kind: BeatKind) -> Beat {
        Beat {
            kind,
            t: 0.0,
            started: false,
            stage: 0,
            drains: Vec::new(),
            mark: 0,
        }
    }
}

fn banner(
    title: impl Into<String>,
    subtitle: Option<String>,
    tone: Tone,
    duration: f32,
) -> BannerView {
    BannerView {
        title: title.into(),
        subtitle,
        icon: None,
        tone,
        age: 0.0,
        duration,
    }
}

fn popup(title: impl Into<String>, lines: Vec<String>, unit: Option<UnitId>) -> Popup {
    Popup {
        title: title.into(),
        lines,
        unit,
        age: 0.0,
    }
}

/// Seconds a popup stays up unless dismissed (and the minimum before a dismissal counts).
const POPUP_SECONDS: f32 = 1.8;
const POPUP_MIN: f32 = 0.35;
const OUTCOME_SECONDS: f32 = 2.6;
const OUTCOME_MIN: f32 = 0.8;
/// Seconds the view rests on a tile whose terrain an event changed.
const TERRAIN_SECONDS: f32 = 0.6;

/// Turn one batch of events into beats. Names are resolved now (the state is the one after
/// the events happened, but names and classes do not change).
pub fn plan(
    events: &[BattleEvent],
    state: &BattleState,
    pack: &Pack,
    fx: &BTreeMap<String, FxDef>,
) -> Vec<Beat> {
    let name = |u: UnitId| state.units[u].name.clone();
    let item_name = |id: &str| pack.item(id).map_or(id.to_string(), |i| i.name.clone());
    let mut beats: Vec<Beat> = Vec::new();
    // A strategy cast from a scroll follows its `ItemUsed` and costs no MP.
    let mut scroll_user: Option<UnitId> = None;
    for ev in events {
        let kind = match ev {
            BattleEvent::PhaseStart { side, turn } => BeatKind::Phase {
                side: *side,
                turn: *turn,
                limit: state.turn_limit,
            },
            BattleEvent::Moved { unit, path } => BeatKind::Walk {
                unit: *unit,
                path: path.clone(),
            },
            BattleEvent::Strike {
                attacker,
                defender,
                damage,
                morale_loss,
                counter,
            } => {
                let a = &state.units[*attacker];
                let ranged = a.pos.chebyshev(state.units[*defender].pos) > 1
                    || pack.class(&a.class).is_some_and(|c| c.family == "archer");
                BeatKind::Strike {
                    attacker: *attacker,
                    defender: *defender,
                    damage: *damage,
                    morale_loss: *morale_loss,
                    ranged,
                    counter: *counter,
                }
            }
            BattleEvent::StrategyUsed {
                caster,
                strategy,
                hits,
                ..
            } => {
                let def = pack.strategy(strategy);
                let sname = def.map_or(strategy.clone(), |s| s.name.clone());
                let key = def.map(|s| s.fx.clone()).filter(|k| !k.is_empty());
                let fx_len = key
                    .as_deref()
                    .and_then(|k| fx.get(k))
                    .map_or(DEFAULT_FX_SECONDS, |d| d.duration());
                let from_scroll = scroll_user.take() == Some(*caster);
                BeatKind::Strategy {
                    caster: *caster,
                    caption: format!("{} · {sname}", name(*caster)),
                    fx: key,
                    fx_len,
                    hits: hits.clone(),
                    mp: if from_scroll {
                        0
                    } else {
                        def.map_or(0, |s| s.mp)
                    },
                    morale_effect: def.is_some_and(|s| {
                        s.effects
                            .iter()
                            .any(|e| matches!(e, hero_core::data::Effect::Morale { .. }))
                    }),
                }
            }
            BattleEvent::ItemUsed {
                user,
                target,
                item,
                healed,
                morale,
            } => {
                let scroll = pack.item(item).is_some_and(|d| d.strategy.is_some());
                if scroll {
                    scroll_user = Some(*user);
                }
                BeatKind::Item {
                    user: *user,
                    target: *target,
                    caption: format!("{} · {}", name(*user), item_name(item)),
                    healed: *healed,
                    morale: *morale,
                    scroll,
                }
            }
            BattleEvent::Regenerated {
                unit,
                hp,
                mp,
                morale,
            } => {
                let entry = (*unit, *hp, *mp, *morale);
                if let Some(Beat {
                    kind: BeatKind::Regen(list),
                    ..
                }) = beats.last_mut()
                {
                    list.push(entry);
                    continue;
                }
                BeatKind::Regen(vec![entry])
            }
            BattleEvent::Confused { unit } => BeatKind::Confused(*unit),
            BattleEvent::StatusExpired { unit, .. } => BeatKind::Recovered(*unit),
            BattleEvent::WeatherChanged { weather } => BeatKind::Weather(*weather),
            BattleEvent::ExpGained { unit, amount } => {
                if let Some(Beat {
                    kind: BeatKind::Exp(list),
                    ..
                }) = beats.last_mut()
                {
                    list.push((*unit, *amount));
                    continue;
                }
                BeatKind::Exp(vec![(*unit, *amount)])
            }
            BattleEvent::LevelUp {
                unit,
                level,
                hp_gain,
                mp_gain,
            } => {
                let mut lines = vec![format!("Lv {} → {level}", level.saturating_sub(1))];
                lines.push(format!("병력 +{hp_gain}"));
                if *mp_gain != 0 {
                    lines.push(format!("MP {mp_gain:+}"));
                }
                BeatKind::LevelUp {
                    unit: *unit,
                    popup: popup(format!("{} 레벨 업!", name(*unit)), lines, Some(*unit)),
                    level: *level,
                    hp_gain: *hp_gain,
                    mp_gain: *mp_gain,
                }
            }
            BattleEvent::Promoted { unit, from, to } => {
                let cname = |id: &str| pack.class(id).map_or(id.to_string(), |c| c.name.clone());
                BeatKind::Promoted {
                    unit: *unit,
                    popup: popup(
                        format!("{} 전직!", name(*unit)),
                        vec![format!("{} → {}", cname(from), cname(to))],
                        Some(*unit),
                    ),
                    to: to.clone(),
                }
            }
            BattleEvent::Learned { unit, strategy } => {
                let sname = pack
                    .strategy(strategy)
                    .map_or(strategy.clone(), |s| s.name.clone());
                BeatKind::Popup(popup(
                    "새 책략",
                    vec![format!("{} {} 익혔다!", name(*unit), text::object(&sname))],
                    Some(*unit),
                ))
            }
            BattleEvent::Retreated { unit } => BeatKind::Retreat(*unit),
            BattleEvent::Spawned { units } => BeatKind::Spawn(units.clone()),
            BattleEvent::TreasureFound { unit, item, gold } => {
                let iname = item.as_deref().map(item_name);
                BeatKind::Treasure {
                    banner: banner(
                        text::treasure_text(iname.as_deref(), *gold),
                        Some(format!("{} 보물을 찾았다", text::subject(&name(*unit)))),
                        Tone::Good,
                        1.6,
                    ),
                    gold: *gold,
                }
            }
            BattleEvent::ItemDropped { unit, item } => BeatKind::Banner(banner(
                format!("{} 얻었다!", text::object(&item_name(item))),
                Some(format!("{} 떨어뜨린 물건", text::subject(&name(*unit)))),
                Tone::Good,
                1.6,
            )),
            BattleEvent::Drama { scene, terrain } => {
                BeatKind::Drama(scene.clone(), terrain.clone())
            }
            BattleEvent::TerrainChanged { pos } => BeatKind::Terrain(*pos),
            BattleEvent::ObjectiveChanged { text } => {
                BeatKind::Banner(banner("목표 변경", Some(text.clone()), Tone::Neutral, 2.0))
            }
            BattleEvent::BonusAchieved { exp } => BeatKind::Banner(banner(
                "보너스 달성!",
                Some(format!("승리하면 출진한 전원 경험치 +{exp}")),
                Tone::Good,
                1.8,
            )),
            BattleEvent::Victory => BeatKind::Outcome(true),
            BattleEvent::Defeat(_) => BeatKind::Outcome(false),
        };
        beats.push(Beat::new(kind));
    }
    beats
}

/// Plays beats in order. See the module docs.
#[derive(Debug, Default)]
pub struct EventPlayer {
    queue: VecDeque<Beat>,
    current: Option<Beat>,
    blocked: bool,
    /// A batch was pushed and [`EventPlayer::take_finished`] has not reported it yet.
    unsettled: bool,
}

impl EventPlayer {
    pub fn push(&mut self, beats: Vec<Beat>) {
        self.queue.extend(beats);
        self.unsettled = true;
    }

    /// Nothing is playing or waiting.
    pub fn is_idle(&self) -> bool {
        self.current.is_none() && self.queue.is_empty() && !self.blocked
    }

    /// Whether the pushed batches have all played out since the last report; `true` once per
    /// time the queue runs dry. The queue can run dry during [`EventPlayer::update`], at
    /// [`EventPlayer::resume`] (a batch whose last beat is a drama ends when the overlay closes,
    /// with no update in between) or right away (a batch with nothing to animate), so the
    /// screen asks this instead of watching `is_idle` around `update`.
    pub fn take_finished(&mut self) -> bool {
        let finished = self.unsettled && self.is_idle();
        if finished {
            self.unsettled = false;
        }
        finished
    }

    /// Scenes of the drama beats still queued, in order: not started yet, so a quick save has
    /// to keep them (the battle state is already past them). A drama beat that has started is
    /// no longer here: the overlay showing it is on the screen stack.
    pub fn pending_dramas(&self) -> Vec<String> {
        self.queue
            .iter()
            .filter_map(|beat| match &beat.kind {
                BeatKind::Drama(scene, _) => Some(scene.clone()),
                _ => None,
            })
            .collect()
    }

    /// Waiting for a drama overlay to close.
    #[cfg(test)]
    pub fn is_blocked(&self) -> bool {
        self.blocked
    }

    /// The drama overlay closed: continue with the next beat.
    pub fn resume(&mut self) {
        self.blocked = false;
    }

    /// Unit the current beat is about (the camera and info panel follow it).
    pub fn focus_unit(&self) -> Option<UnitId> {
        match &self.current.as_ref()?.kind {
            BeatKind::Walk { unit, .. } => Some(*unit),
            BeatKind::Strike { defender, .. } => Some(*defender),
            BeatKind::Strategy { caster, .. } => Some(*caster),
            _ => None,
        }
    }

    /// Advance by `dt` (already scaled by the battle speed). `skip` dismisses banners and
    /// popups (a confirm press).
    pub fn update(
        &mut self,
        dt: f32,
        skip: bool,
        scene: &mut Scene,
        fx: &BTreeMap<String, FxDef>,
        cues: &mut Vec<Cue>,
    ) {
        let mut budget = dt;
        // Several short beats may finish within one frame; bound the loop anyway.
        for _ in 0..16 {
            if self.blocked {
                return;
            }
            if self.current.is_none() {
                match self.queue.pop_front() {
                    Some(b) => self.current = Some(b),
                    None => return,
                }
            }
            let beat = self.current.as_mut().expect("current beat");
            let done = step(beat, budget, skip, scene, fx, cues);
            if let BeatKind::Drama(..) = beat.kind {
                self.blocked = true;
            }
            if !done {
                return;
            }
            self.current = None;
            // Following beats start at the next frame's time budget.
            budget = 0.0;
        }
    }
}

/// An HP bar moving from one value to another.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Drain {
    unit: UnitId,
    from: f32,
    to: f32,
}

/// Seconds an HP bar takes to reach its new value.
const DRAIN_SECONDS: f32 = 0.4;

/// Start moving `unit`'s displayed HP by `delta`.
fn drain(drains: &mut Vec<Drain>, scene: &Scene, unit: UnitId, delta: f32) {
    let v = &scene.views[unit];
    drains.push(Drain {
        unit,
        from: v.hp,
        to: (v.hp + delta).clamp(0.0, v.max_hp.max(1) as f32),
    });
}

/// Set the displayed HP of every drain `k` (0..=1) of the way.
fn apply_drains(drains: &[Drain], scene: &mut Scene, k: f32) {
    let k = k.clamp(0.0, 1.0);
    for d in drains {
        scene.views[d.unit].hp = d.from + (d.to - d.from) * k;
    }
}

/// Advance one beat; returns whether it finished.
fn step(
    beat: &mut Beat,
    dt: f32,
    skip: bool,
    scene: &mut Scene,
    fx: &BTreeMap<String, FxDef>,
    cues: &mut Vec<Cue>,
) -> bool {
    let Beat {
        kind,
        t,
        started,
        stage,
        drains,
        mark,
    } = beat;
    let first = !*started;
    *started = true;
    *t += dt;
    let t = *t;
    match kind {
        BeatKind::Banner(b) => {
            b.age = t;
            show_banner(b, first, skip, scene, cues, None)
        }
        BeatKind::Phase { side, turn, limit } => {
            if first {
                scene.hud.phase = *side;
                scene.hud.turn = *turn;
                // A new phase: nobody has acted in it yet.
                for v in &mut scene.views {
                    v.acted = false;
                }
                cues.push(Cue::PhaseMusic(*side));
                cues.push(Cue::Sfx(sfx::PHASE));
            }
            let mut b = banner(
                text::phase_title(*side),
                Some(text::turn_text(*turn, *limit)),
                Tone::of(*side),
                1.4,
            );
            b.age = t;
            show_banner(&b, first, skip, scene, cues, None)
        }
        BeatKind::Weather(w) => {
            if first {
                scene.hud.weather = *w;
            }
            let mut b = banner(
                format!("날씨: {}", text::weather_name(*w)),
                None,
                Tone::Neutral,
                1.2,
            );
            b.icon = Some(text::weather_icon(*w));
            b.age = t;
            show_banner(&b, first, skip, scene, cues, None)
        }
        BeatKind::Walk { unit, path } => {
            let v = &mut scene.views[*unit];
            v.visible = true;
            if path.len() < 2 {
                if let Some(p) = path.first() {
                    v.pos = tile_pos(*p);
                }
                return true;
            }
            let steps = (path.len() - 1) as f32;
            let s = (t / STEP_SECONDS).min(steps);
            let i = (s.floor() as usize).min(path.len() - 2);
            let f = s - i as f32;
            let (a, b) = (path[i], path[i + 1]);
            v.pos = tile_pos(a).lerp(tile_pos(b), f);
            v.facing = Dir::towards(a, b);
            v.pose = Pose::Walk;
            if *mark != i + 1 {
                *mark = i + 1;
                cues.push(Cue::Follow(b));
                if i % 2 == 0 {
                    cues.push(Cue::Sfx(sfx::STEP));
                }
            }
            if s >= steps {
                v.pos = tile_pos(*path.last().expect("path has tiles"));
                v.pose = Pose::Idle;
                return true;
            }
            false
        }
        BeatKind::Strike {
            attacker,
            defender,
            damage,
            morale_loss,
            ranged,
            counter,
        } => {
            let (a, d) = (*attacker, *defender);
            let (ap, dp) = (scene.views[a].tile(), scene.views[d].tile());
            let dir = dir_vec(ap, dp);
            if first {
                cues.push(Cue::Follow(dp));
                scene.views[a].facing = Dir::towards(ap, dp);
                scene.views[d].facing = Dir::towards(dp, ap);
                scene.views[a].pose = Pose::Attack;
                if *ranged {
                    cues.push(Cue::Sfx(sfx::ARROW));
                }
                if *counter {
                    scene.float(a, "반격", FloatKind::Info);
                }
            }
            // Attacker lunge: out until the impact, back afterwards.
            let lunge = if *ranged {
                0.0
            } else if t < IMPACT {
                t / IMPACT
            } else {
                (1.0 - (t - IMPACT) / 0.3).max(0.0)
            };
            scene.views[a].offset = dir * ref_px(LUNGE) * lunge;
            if t >= IMPACT && *stage == 0 {
                *stage = 1;
                let lethal = scene.views[d].hp - *damage as f32 <= 0.0;
                let heavy = lethal || *damage >= 300;
                cues.push(Cue::Sfx(if heavy { sfx::HIT_HEAVY } else { sfx::HIT }));
                let key = if *ranged { "arrow" } else { "slash" };
                let at = scene.views[d].pos;
                scene.spawn_fx(key, at, fx);
                scene.damage_float(d, *damage);
                drain(drains, scene, d, -(*damage as f32));
                let v = &mut scene.views[d];
                v.pose = Pose::Hurt;
                v.flash = 0.3;
                v.morale = (v.morale - *morale_loss).max(0);
                if lethal {
                    v.knocked = Some(dir);
                }
            }
            if *stage >= 1 {
                apply_drains(drains, scene, (t - IMPACT) / DRAIN_SECONDS);
                let v = &mut scene.views[d];
                if v.knocked.is_some() {
                    // Pushed back hard; the retreat beat carries it off the field.
                    v.offset = dir * ref_px(10.0) * ((t - IMPACT) / 0.18).clamp(0.0, 1.0);
                } else {
                    let out = ((t - IMPACT) / 0.16).clamp(0.0, 1.0);
                    let back = ((t - 0.62) / 0.2).clamp(0.0, 1.0);
                    v.offset = dir * ref_px(knockback(*damage)) * out * (1.0 - back);
                }
            }
            if t >= STRIKE_END {
                apply_drains(drains, scene, 1.0);
                let av = &mut scene.views[a];
                av.offset = Vec2::ZERO;
                av.pose = Pose::Idle;
                let dv = &mut scene.views[d];
                if dv.knocked.is_none() {
                    dv.offset = Vec2::ZERO;
                    dv.pose = Pose::Idle;
                }
                return true;
            }
            false
        }
        BeatKind::Strategy {
            caster,
            caption,
            fx: key,
            fx_len,
            hits,
            mp,
            morale_effect,
        } => {
            const CAST: f32 = 0.3;
            let c = *caster;
            let impact = CAST + *fx_len * 0.5;
            let end = (impact + 0.6).max(CAST + *fx_len);
            if first {
                scene.views[c].pose = Pose::Attack;
                scene.views[c].mp = (scene.views[c].mp - *mp).max(0);
                scene.caption = Some(Caption {
                    text: caption.clone(),
                    age: 0.0,
                    life: end + 0.2,
                });
                if let Some(h) = hits.first() {
                    cues.push(Cue::Follow(scene.views[h.unit].tile()));
                }
            }
            if t >= CAST && *stage == 0 {
                *stage = 1;
                if let Some(k) = key.as_deref() {
                    for h in hits.iter() {
                        let at = scene.views[h.unit].pos;
                        scene.spawn_fx(k, at, fx);
                    }
                    if let Some(s) = fx_sound(k) {
                        cues.push(Cue::Sfx(s));
                    }
                }
            }
            if t >= impact && *stage == 1 {
                *stage = 2;
                scene.views[c].pose = Pose::Idle;
                let mut hurt = false;
                for h in hits.iter() {
                    let u = h.unit;
                    if !h.success {
                        scene.float(u, "실패", FloatKind::Miss);
                        continue;
                    }
                    if h.damage > 0 {
                        hurt = true;
                        scene.damage_float(u, h.damage);
                        scene.views[u].pose = Pose::Hurt;
                        scene.views[u].flash = 0.3;
                    }
                    if h.healed > 0 {
                        scene.float(u, format!("+{}", h.healed), FloatKind::Heal);
                    }
                    if h.morale != 0 {
                        if *morale_effect {
                            scene.float(u, text::morale_text(h.morale), FloatKind::Morale);
                            // Morale arrow (unless the strategy's own effect already is one).
                            let arrow = if h.morale > 0 {
                                "morale_up"
                            } else {
                                "morale_down"
                            };
                            if key.as_deref() != Some(arrow) {
                                let at = scene.views[u].pos;
                                scene.spawn_fx(arrow, at, fx);
                            }
                        }
                        let v = &mut scene.views[u];
                        v.morale = (v.morale + h.morale).clamp(0, 100);
                    }
                    if h.damage == 0 && h.healed == 0 && h.morale == 0 && h.status.is_none() {
                        scene.float(u, "효과 없음", FloatKind::Miss);
                    }
                    let delta = (h.healed - h.damage) as f32;
                    if delta != 0.0 {
                        drain(drains, scene, u, delta);
                    }
                }
                if hurt {
                    cues.push(Cue::Sfx(sfx::HIT));
                }
            }
            if *stage >= 2 {
                apply_drains(drains, scene, (t - impact) / DRAIN_SECONDS);
            }
            if t >= end {
                apply_drains(drains, scene, 1.0);
                for h in hits.iter() {
                    if scene.views[h.unit].pose == Pose::Hurt {
                        scene.views[h.unit].pose = Pose::Idle;
                    }
                }
                return true;
            }
            false
        }
        BeatKind::Item {
            user,
            target,
            caption,
            healed,
            morale,
            scroll,
        } => {
            if first {
                scene.views[*user].pose = Pose::Attack;
                scene.caption = Some(Caption {
                    text: caption.clone(),
                    age: 0.0,
                    life: 1.2,
                });
                cues.push(Cue::Follow(scene.views[*target].tile()));
            }
            if *scroll {
                // The strategy beat that follows shows the effect.
                if t >= 0.5 {
                    scene.views[*user].pose = Pose::Idle;
                    return true;
                }
                return false;
            }
            if t >= 0.2 && *stage == 0 {
                *stage = 1;
                let at = scene.views[*target].pos;
                if *healed > 0 {
                    scene.spawn_fx("heal", at, fx);
                    cues.push(Cue::Sfx(sfx::HEAL));
                } else if *morale > 0 {
                    scene.spawn_fx("morale_up", at, fx);
                    cues.push(Cue::Sfx(sfx::MORALE_UP));
                }
            }
            const SHOW: f32 = 0.45;
            if t >= SHOW && *stage == 1 {
                *stage = 2;
                scene.views[*user].pose = Pose::Idle;
                if *healed > 0 {
                    scene.float(*target, format!("+{healed}"), FloatKind::Heal);
                    drain(drains, scene, *target, *healed as f32);
                }
                if *morale != 0 {
                    scene.float(*target, text::morale_text(*morale), FloatKind::Morale);
                    let v = &mut scene.views[*target];
                    v.morale = (v.morale + *morale).clamp(0, 100);
                }
                if *healed == 0 && *morale == 0 {
                    scene.float(*target, "효과 없음", FloatKind::Miss);
                }
            }
            if *stage >= 2 {
                apply_drains(drains, scene, (t - SHOW) / DRAIN_SECONDS);
            }
            if t >= 1.0 {
                apply_drains(drains, scene, 1.0);
                return true;
            }
            false
        }
        BeatKind::Regen(list) => {
            if first {
                let mut any_hp = false;
                for &(u, hp, mp, morale) in list.iter() {
                    if hp > 0 {
                        scene.float(u, format!("+{hp}"), FloatKind::Heal);
                        drain(drains, scene, u, hp as f32);
                        any_hp = true;
                    }
                    if mp > 0 {
                        scene.float(u, format!("MP +{mp}"), FloatKind::Mp);
                        let v = &mut scene.views[u];
                        v.mp = (v.mp + mp).min(v.max_mp);
                    }
                    if morale > 0 {
                        scene.float(u, text::morale_text(morale), FloatKind::Morale);
                        let v = &mut scene.views[u];
                        v.morale = (v.morale + morale).min(100);
                    }
                }
                if any_hp {
                    cues.push(Cue::Sfx(sfx::HEAL));
                }
            }
            apply_drains(drains, scene, t / DRAIN_SECONDS);
            t >= 0.8
        }
        BeatKind::Confused(u) => {
            if first {
                let at = scene.views[*u].pos;
                scene.spawn_fx("confuse", at, fx);
                scene.float(*u, "혼란", FloatKind::Morale);
                scene.views[*u].confused = true;
                cues.push(Cue::Sfx(sfx::CONFUSE));
                cues.push(Cue::Follow(scene.views[*u].tile()));
            }
            t >= 0.8
        }
        BeatKind::Recovered(u) => {
            if first {
                scene.float(*u, "혼란 회복", FloatKind::Info);
                scene.views[*u].confused = false;
            }
            t >= 0.6
        }
        BeatKind::Exp(list) => {
            if first {
                for &(u, amount) in list.iter() {
                    scene.float(u, format!("+{amount} EXP"), FloatKind::Exp);
                    let v = &mut scene.views[u];
                    v.exp = v.exp.saturating_add(amount);
                }
            }
            t >= 0.6
        }
        BeatKind::LevelUp {
            unit,
            popup,
            level,
            hp_gain,
            mp_gain,
        } => {
            if first {
                let at = scene.views[*unit].pos;
                scene.spawn_fx("levelup", at, fx);
                cues.push(Cue::Sfx(sfx::LEVELUP));
                cues.push(Cue::Follow(scene.views[*unit].tile()));
                let v = &mut scene.views[*unit];
                v.level = *level;
                v.max_hp += *hp_gain;
                v.hp = (v.hp + *hp_gain as f32).min(v.max_hp as f32);
                v.max_mp += *mp_gain;
                v.mp = (v.mp + *mp_gain).clamp(0, v.max_mp.max(0));
            }
            show_popup(popup, t, skip, scene)
        }
        BeatKind::Popup(p) => show_popup(p, t, skip, scene),
        BeatKind::Promoted { unit, popup, to } => {
            if first {
                let at = scene.views[*unit].pos;
                scene.spawn_fx("levelup", at, fx);
                cues.push(Cue::Sfx(sfx::LEVELUP));
                scene.views[*unit].class = to.clone();
            }
            show_popup(popup, t, skip, scene)
        }
        BeatKind::Retreat(u) => {
            let u = *u;
            if first {
                cues.push(Cue::Sfx(sfx::RETREAT));
                scene.float(u, "퇴각", FloatKind::Info);
            }
            const LEN: f32 = 0.7;
            let k = (t / LEN).clamp(0.0, 1.0);
            let v = &mut scene.views[u];
            match v.knocked {
                // Knocked off the field: accelerating flight, fading at the end.
                Some(dir) => {
                    v.pose = Pose::Hurt;
                    v.offset = dir * ref_px(10.0 + 260.0 * k * k);
                    v.alpha = 1.0 - ((k - 0.6) / 0.4).clamp(0.0, 1.0);
                }
                // Retreat without a blow (event, rout): sink and fade.
                None => {
                    v.offset = vec2(0.0, ref_px(4.0 * k));
                    v.alpha = 1.0 - k;
                }
            }
            if t >= LEN {
                v.visible = false;
                v.alpha = 1.0;
                v.offset = Vec2::ZERO;
                v.knocked = None;
                v.pose = Pose::Idle;
                return true;
            }
            false
        }
        BeatKind::Spawn(units) => {
            if first {
                let n = units.len().max(1) as i32;
                let (sx, sy) = units.iter().fold((0, 0), |(x, y), &u| {
                    let p = scene.views[u].tile();
                    (x + p.x, y + p.y)
                });
                cues.push(Cue::Center(Pos::new(sx / n, sy / n)));
            }
            let fade = ((t - 0.3) / 0.6).clamp(0.0, 1.0);
            for &u in units.iter() {
                let v = &mut scene.views[u];
                v.visible = true;
                v.alpha = fade;
            }
            let tone = units
                .first()
                .map_or(Tone::Enemy, |&u| Tone::of(scene.views[u].side));
            let mut b = banner("원군 출현!", None, tone, 1.6);
            b.age = t;
            let done = show_banner(&b, first, skip && t > 0.9, scene, cues, Some(sfx::PHASE));
            if done {
                for &u in units.iter() {
                    scene.views[u].alpha = 1.0;
                }
            }
            done
        }
        BeatKind::Treasure { banner: b, gold } => {
            if first {
                scene.hud.gold_found += *gold;
            }
            b.age = t;
            show_banner(b, first, skip, scene, cues, Some(sfx::TREASURE))
        }
        BeatKind::Drama(scene_id, terrain) => {
            if first {
                cues.push(Cue::Drama(scene_id.clone(), terrain.clone()));
            }
            true
        }
        BeatKind::Terrain(pos) => {
            if first {
                cues.push(Cue::Center(*pos));
                cues.push(Cue::Terrain(*pos));
            }
            t >= TERRAIN_SECONDS || skip
        }
        BeatKind::Outcome(victory) => {
            if first {
                scene.outcome = Some(*victory);
                scene.outcome_age = 0.0;
                cues.push(Cue::Jingle(*victory));
            }
            let done = t >= OUTCOME_SECONDS || (skip && t > OUTCOME_MIN);
            if done {
                scene.outcome = None;
            }
            done
        }
    }
}

/// Show a banner whose age is already set; returns whether it has finished (a confirm press
/// dismisses it after a moment).
fn show_banner(
    b: &BannerView,
    first: bool,
    skip: bool,
    scene: &mut Scene,
    cues: &mut Vec<Cue>,
    sound: Option<&'static str>,
) -> bool {
    if first {
        if let Some(s) = sound {
            cues.push(Cue::Sfx(s));
        }
    }
    let done = b.age >= b.duration || (skip && b.age > 0.3);
    scene.banner = (!done).then(|| b.clone());
    done
}

fn show_popup(p: &mut Popup, t: f32, skip: bool, scene: &mut Scene) -> bool {
    p.age = t;
    let done = t >= POPUP_SECONDS || (skip && t > POPUP_MIN);
    scene.popup = (!done).then(|| p.clone());
    done
}

/// Sound of an effect strip.
pub fn fx_sound(key: &str) -> Option<&'static str> {
    Some(match key {
        "fire" => sfx::FIRE,
        "water" => sfx::WATER,
        "rock" => sfx::ROCK,
        "heal" => sfx::HEAL,
        "morale_up" => sfx::MORALE_UP,
        "morale_down" => sfx::MORALE_DOWN,
        "confuse" => sfx::CONFUSE,
        "arrow" => sfx::ARROW,
        "slash" => sfx::HIT,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screens::battle::testutil;
    use hero_core::battle::StrategyHit;

    #[test]
    fn knockback_grows_with_damage() {
        assert_eq!(knockback(50), 0.0);
        assert_eq!(knockback(99), 0.0);
        assert_eq!(knockback(100), 3.0);
        assert_eq!(knockback(299), 3.0);
        assert_eq!(knockback(300), 6.0);
        assert!(knockback(600) > knockback(400));
        assert_eq!(knockback(100_000), 16.0);
    }

    #[test]
    fn plan_merges_regeneration_and_exp_and_keeps_order() {
        let (pack, state) = testutil::sishui();
        let ev = vec![
            BattleEvent::PhaseStart {
                side: Side::Player,
                turn: 2,
            },
            BattleEvent::Regenerated {
                unit: 0,
                hp: 50,
                mp: 0,
                morale: 10,
            },
            BattleEvent::Regenerated {
                unit: 1,
                hp: 20,
                mp: 1,
                morale: 0,
            },
            BattleEvent::Strike {
                attacker: 1,
                defender: 4,
                damage: 120,
                morale_loss: 5,
                counter: false,
            },
            BattleEvent::ExpGained { unit: 1, amount: 6 },
            BattleEvent::ExpGained { unit: 0, amount: 8 },
            BattleEvent::Drama {
                scene: "sishui_duel".into(),
                terrain: BTreeMap::new(),
            },
            BattleEvent::Victory,
        ];
        let beats = plan(&ev, &state, &pack, &BTreeMap::new());
        let kinds: Vec<&str> = beats
            .iter()
            .map(|b| match &b.kind {
                BeatKind::Phase { .. } => "phase",
                BeatKind::Regen(l) if l.len() == 2 => "regen2",
                // Far apart in the fixture, so the strike is shown as a ranged one.
                BeatKind::Strike { ranged: true, .. } => "strike",
                BeatKind::Exp(l) if l.len() == 2 => "exp2",
                BeatKind::Drama(..) => "drama",
                BeatKind::Outcome(true) => "victory",
                _ => "other",
            })
            .collect();
        assert_eq!(
            kinds,
            ["phase", "regen2", "strike", "exp2", "drama", "victory"]
        );
    }

    /// Run the player until it is idle or blocked, collecting cues.
    fn run(player: &mut EventPlayer, scene: &mut Scene, cues: &mut Vec<Cue>, max_frames: usize) {
        let fx = BTreeMap::new();
        for _ in 0..max_frames {
            if player.is_idle() || player.is_blocked() {
                return;
            }
            player.update(1.0 / 60.0, false, scene, &fx, cues);
            scene.tick(1.0 / 60.0);
        }
        panic!("event queue did not settle");
    }

    #[test]
    fn queue_animates_in_order_and_waits_for_dramas() {
        let (pack, state) = testutil::sishui();
        let mut scene = Scene::new(&state);
        let guan_yu = state.find_unit("guan_yu").unwrap();
        let foe = state
            .units
            .iter()
            .position(|u| u.side == Side::Enemy)
            .unwrap();
        let start = state.units[guan_yu].pos;
        let path = vec![start, start.offset(0, -1), start.offset(0, -2)];
        let hp = scene.views[foe].hp;
        let ev = vec![
            BattleEvent::Moved {
                unit: guan_yu,
                path: path.clone(),
            },
            BattleEvent::Strike {
                attacker: guan_yu,
                defender: foe,
                damage: hp as i32 + 10,
                morale_loss: 50,
                counter: false,
            },
            BattleEvent::Retreated { unit: foe },
            BattleEvent::Drama {
                scene: "sishui_duel".into(),
                terrain: BTreeMap::new(),
            },
            BattleEvent::Victory,
        ];
        let mut player = EventPlayer::default();
        player.push(plan(&ev, &state, &pack, &BTreeMap::new()));
        assert!(!player.is_idle());
        let mut cues = Vec::new();
        run(&mut player, &mut scene, &mut cues, 2000);

        // Walked to the last tile, the defeated unit flew off and is gone.
        assert_eq!(scene.views[guan_yu].tile(), start.offset(0, -2));
        assert!(!scene.views[foe].visible);
        assert_eq!(scene.views[foe].hp, 0.0);
        // Blocked at the drama; nothing after it has run yet.
        assert!(player.is_blocked());
        assert_eq!(
            cues.last(),
            Some(&Cue::Drama("sishui_duel".into(), BTreeMap::new()))
        );
        assert!(cues.contains(&Cue::Sfx(sfx::HIT_HEAVY)));
        assert!(cues.contains(&Cue::Sfx(sfx::RETREAT)));
        let hit = cues.iter().position(|c| *c == Cue::Sfx(sfx::HIT_HEAVY));
        let retreat = cues.iter().position(|c| *c == Cue::Sfx(sfx::RETREAT));
        assert!(hit < retreat);
        assert!(!cues.contains(&Cue::Jingle(true)));

        assert!(!player.take_finished());

        player.resume();
        cues.clear();
        run(&mut player, &mut scene, &mut cues, 2000);
        assert!(player.is_idle());
        assert_eq!(cues, vec![Cue::Jingle(true)]);
        assert_eq!(scene.outcome, None);
        assert!(player.take_finished());
        assert!(!player.take_finished());
    }

    /// Regression: a batch whose last beat is a drama (a `reach` event running `[drama]` after
    /// a move) runs dry when the overlay closes, without another `update`. The screen must
    /// still learn that it finished, or the move never opens the command menu (soft lock in
    /// `Mode::Walking`).
    #[test]
    fn a_batch_ending_in_a_drama_finishes_when_resumed() {
        let (pack, state) = testutil::sishui();
        let mut scene = Scene::new(&state);
        let gy = state.find_unit("guan_yu").unwrap();
        let start = state.units[gy].pos;
        let ev = vec![
            BattleEvent::Moved {
                unit: gy,
                path: vec![start, start.offset(0, -1)],
            },
            BattleEvent::Drama {
                scene: "sishui_duel".into(),
                terrain: BTreeMap::new(),
            },
        ];
        let mut player = EventPlayer::default();
        assert!(!player.take_finished(), "nothing was pushed");
        player.push(plan(&ev, &state, &pack, &BTreeMap::new()));
        assert!(!player.take_finished());
        let mut cues = Vec::new();
        run(&mut player, &mut scene, &mut cues, 600);
        assert!(player.is_blocked());
        assert_eq!(
            cues.last(),
            Some(&Cue::Drama("sishui_duel".into(), BTreeMap::new()))
        );
        assert!(!player.take_finished(), "the drama is still open");

        player.resume();
        assert!(player.is_idle());
        assert!(player.take_finished());
        assert!(!player.take_finished(), "reported once");
    }

    #[test]
    fn a_batch_with_nothing_to_animate_finishes_at_once() {
        let mut player = EventPlayer::default();
        player.push(Vec::new());
        assert!(player.take_finished());
        assert!(!player.take_finished());
    }

    #[test]
    fn strategy_misses_and_heals_are_shown() {
        let (pack, state) = testutil::sishui();
        let mut scene = Scene::new(&state);
        let foe = state
            .units
            .iter()
            .position(|u| u.side == Side::Enemy)
            .unwrap();
        let hp = scene.views[1].hp;
        scene.views[1].hp = hp - 100.0;
        let ev = vec![BattleEvent::StrategyUsed {
            caster: 0,
            strategy: "scorch".into(),
            target: state.units[foe].pos,
            hits: vec![
                StrategyHit {
                    unit: foe,
                    success: false,
                    damage: 0,
                    healed: 0,
                    morale: 0,
                    status: None,
                },
                StrategyHit {
                    unit: 1,
                    success: true,
                    damage: 0,
                    healed: 60,
                    morale: 0,
                    status: None,
                },
            ],
        }];
        let mut player = EventPlayer::default();
        player.push(plan(&ev, &state, &pack, &BTreeMap::new()));
        let mut cues = Vec::new();
        let fx = BTreeMap::new();
        let mut texts = Vec::new();
        for _ in 0..600 {
            if player.is_idle() {
                break;
            }
            player.update(1.0 / 60.0, false, &mut scene, &fx, &mut cues);
            texts.extend(scene.floats.iter().map(|f| f.text.clone()));
        }
        assert!(player.is_idle());
        assert!(texts.iter().any(|t| t == "실패"));
        assert!(texts.iter().any(|t| t == "+60"));
        assert_eq!(scene.views[1].hp, hp - 40.0);
    }

    #[test]
    fn enhanced_hits_show_red_numbers_and_shake_on_heavy_blows() {
        let (pack, state) = testutil::sishui();
        let gy = state.find_unit("guan_yu").unwrap();
        let foe = state
            .units
            .iter()
            .position(|u| u.side == Side::Enemy)
            .unwrap();
        let max = state.units[foe].max_hp;
        let strike = |damage: i32| BattleEvent::Strike {
            attacker: gy,
            defender: foe,
            damage,
            morale_loss: 0,
            counter: false,
        };
        // Floats and the most shake seen while one strike plays.
        let play = |enhanced: bool, damage: i32| {
            let mut scene = Scene::new(&state);
            scene.enhanced = enhanced;
            let mut player = EventPlayer::default();
            player.push(plan(&[strike(damage)], &state, &pack, &BTreeMap::new()));
            let (mut floats, mut shake) = (Vec::new(), 0.0f32);
            for _ in 0..600 {
                if player.is_idle() {
                    break;
                }
                player.update(
                    1.0 / 60.0,
                    false,
                    &mut scene,
                    &BTreeMap::new(),
                    &mut Vec::new(),
                );
                floats.extend(scene.floats.iter().map(|f| (f.text.clone(), f.kind)));
                shake = shake.max(scene.shake);
                scene.tick(1.0 / 60.0);
            }
            floats.dedup();
            (floats, shake, scene.shake_offset())
        };
        let light = max / 10;
        let heavy = max / 3;

        // The original presentation: plain numbers, no shake, whatever the damage.
        let (floats, shake, _) = play(false, heavy);
        assert_eq!(floats, vec![(heavy.to_string(), FloatKind::Damage)]);
        assert_eq!(shake, 0.0);

        // Enhanced: a red minus number; only a heavy hit shakes, and the shake runs out.
        let (floats, shake, _) = play(true, light);
        assert_eq!(floats, vec![(format!("-{light}"), FloatKind::Hit)]);
        assert_eq!(shake, 0.0);
        let (_, shake, rest) = play(true, heavy);
        assert_eq!(shake, SHAKE_SECONDS);
        assert_eq!(rest, Vec2::ZERO);

        let mut scene = Scene::new(&state);
        scene.shake = SHAKE_SECONDS;
        let first = scene.shake_offset();
        assert_eq!(first.x.abs(), SHAKE_PX);
        assert!(first.y.abs() <= SHAKE_PX);
        scene.tick(0.04);
        assert_eq!(scene.shake_offset().x.signum(), -first.x.signum());
    }

    #[test]
    fn reinforcements_fade_in_with_a_banner() {
        let (pack, mut state) = testutil::sishui();
        // Pretend unit 5 has just been spawned: hidden in the scene, active in the state.
        let mut scene = Scene::new(&state);
        scene.views[5].visible = false;
        state.units[5].pos = Pos::new(4, 4);
        scene.views[5].pos = vec2(4.0, 4.0);
        let ev = vec![BattleEvent::Spawned { units: vec![5] }];
        let mut player = EventPlayer::default();
        player.push(plan(&ev, &state, &pack, &BTreeMap::new()));
        let mut cues = Vec::new();
        let fx = BTreeMap::new();
        player.update(0.1, false, &mut scene, &fx, &mut cues);
        assert!(scene.views[5].visible);
        assert_eq!(scene.views[5].alpha, 0.0);
        assert_eq!(
            scene.banner.as_ref().map(|b| b.title.as_str()),
            Some("원군 출현!")
        );
        assert!(cues.contains(&Cue::Center(Pos::new(4, 4))));
        let mut guard = 0;
        while !player.is_idle() && guard < 1000 {
            player.update(1.0 / 60.0, false, &mut scene, &fx, &mut cues);
            guard += 1;
        }
        assert_eq!(scene.views[5].alpha, 1.0);
        assert!(scene.banner.is_none());
    }

    #[test]
    fn level_up_popup_updates_the_view() {
        let (pack, state) = testutil::sishui();
        let mut scene = Scene::new(&state);
        let (hp, max_hp) = (scene.views[0].hp, scene.views[0].max_hp);
        let ev = vec![
            BattleEvent::LevelUp {
                unit: 0,
                level: 2,
                hp_gain: 50,
                mp_gain: 1,
            },
            BattleEvent::Learned {
                unit: 0,
                strategy: "scorch".into(),
            },
        ];
        let mut player = EventPlayer::default();
        player.push(plan(&ev, &state, &pack, &BTreeMap::new()));
        let mut cues = Vec::new();
        let fx = BTreeMap::new();
        player.update(0.05, false, &mut scene, &fx, &mut cues);
        assert_eq!(scene.views[0].level, 2);
        assert_eq!(scene.views[0].max_hp, max_hp + 50);
        assert_eq!(scene.views[0].hp, hp + 50.0);
        assert!(cues.contains(&Cue::Sfx(sfx::LEVELUP)));
        let title = scene.popup.as_ref().map(|p| p.title.clone()).unwrap();
        assert!(title.contains("레벨 업"), "{title}");
        // Confirm dismisses it after a moment; then the strategy popup follows.
        player.update(0.5, true, &mut scene, &fx, &mut cues);
        player.update(0.05, false, &mut scene, &fx, &mut cues);
        let p = scene.popup.as_ref().unwrap();
        assert_eq!(p.title, "새 책략");
        assert!(p.lines[0].contains("초열을 익혔다"), "{:?}", p.lines);
    }

    #[test]
    fn banners_can_be_dismissed() {
        let (pack, state) = testutil::sishui();
        let mut scene = Scene::new(&state);
        let ev = vec![BattleEvent::PhaseStart {
            side: Side::Enemy,
            turn: 1,
        }];
        let mut player = EventPlayer::default();
        player.push(plan(&ev, &state, &pack, &BTreeMap::new()));
        let mut cues = Vec::new();
        let fx = BTreeMap::new();
        player.update(0.1, false, &mut scene, &fx, &mut cues);
        assert!(scene.banner.is_some());
        assert_eq!(scene.hud.phase, Side::Enemy);
        assert!(cues.contains(&Cue::PhaseMusic(Side::Enemy)));
        // Too early to dismiss, then dismissed.
        player.update(0.1, true, &mut scene, &fx, &mut cues);
        assert!(!player.is_idle());
        player.update(0.2, true, &mut scene, &fx, &mut cues);
        assert!(player.is_idle());
        assert!(scene.banner.is_none());
    }
}
