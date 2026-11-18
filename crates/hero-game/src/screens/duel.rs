//! The duel scene of dramas (`@duel`, `@duel_act`, `@duel_end`, docs/MODDING.md): two mounted
//! officers on a stage 26 cells of 16 pixels wide (the original's duel screen), the left one
//! facing right and the right one mirrored, over the background `gfx/duel/<bg>.png`.
//!
//! A fighter is drawn from a sheet of fifteen 96×96 frames in a row (docs/ASSETS.md): 0–3
//! galloping, 4–11 attacking (pairs), 12 falling, 13 and 14 lying next to the horse (two poses).
//! The sheet is `gfx/duel/<officer>.png` for the officer, else `gfx/duel/left.png` /
//! `right.png` for the side.
//! Without a sheet the fighter is not drawn (the scene still runs its timing).
//!
//! Each fighter's name is shown over their side of the stage ([`name_rects`]), so a player who
//! does not recognise the two sheets still knows who is fighting; an officer the pack does not
//! know (a scene naming someone outside `officers/`) shows the id the scene gave.

use crate::app::Ctx;
use crate::assets::AssetState;
use crate::audio::sfx;
use crate::gfx::{fill_rect, Align, FontId, TextStyle};
use crate::ui::theme;
use crate::ui::window::{draw_window_ex, WindowStyle};
use hero_core::script::{DuelAct, DuelSide, DUEL_TERRAIN};
use macroquad::prelude::*;
use std::collections::BTreeMap;

/// Width of the stage in cells, and a cell's size in stage pixels.
pub const STAGE_CELLS: i32 = 26;
pub const CELL: f32 = 16.0;
/// Height of the stage: the background band (5 cells of sky, 8 of ground).
pub const STAGE_H: f32 = 13.0 * CELL;
/// A fighter's frame.
pub const FRAME: f32 = 96.0;
/// Where the fighters start (cells from the stage's left) and stand (cells from its top).
pub const START: [i32; 2] = [7, 13];
pub const ROW: f32 = 3.0;
/// Seconds per animation step.
pub const STEP: f32 = 0.07;
/// How close a charge brings a fighter to the other, in cells.
pub const CLOSE: i32 = 4;

/// One step of a fighter's move: where they are and which frame shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Pose {
    x: i32,
    frame: u8,
}

/// A duel fighter.
#[derive(Debug, Clone)]
struct Fighter {
    officer: String,
    x: i32,
    frame: u8,
    /// Facing away from the other (fleeing).
    turned: bool,
    gone: bool,
}

/// The duel scene.
#[derive(Debug, Clone)]
pub struct DuelView {
    fighters: [Fighter; 2],
    bg: Option<String>,
    /// Steps still to show: the fighter, their pose and whether they face away.
    queue: Vec<(usize, Pose, bool)>,
    /// Time since the last step was shown.
    timer: f32,
}

fn index(side: DuelSide) -> usize {
    match side {
        DuelSide::Left => 0,
        DuelSide::Right => 1,
    }
}

/// The background key of an `@duel` background `bg` between `left` and `right`:
/// [`DUEL_TERRAIN`] becomes `terrain_<id>` of the terrain `terrain` gives for the left officer
/// (or the right one); `None` (a plain stage) when neither is on it.
pub fn background(
    bg: Option<&str>,
    left: &str,
    right: &str,
    terrain: &BTreeMap<String, String>,
) -> Option<String> {
    match bg {
        Some(DUEL_TERRAIN) => terrain
            .get(left)
            .or_else(|| terrain.get(right))
            .map(|t| format!("terrain_{t}")),
        other => other.map(str::to_string),
    }
}

impl DuelView {
    pub fn new(left: &str, right: &str, bg: Option<String>) -> DuelView {
        let fighter = |officer: &str, x: i32| Fighter {
            officer: officer.to_string(),
            x,
            frame: 0,
            turned: false,
            gone: false,
        };
        DuelView {
            fighters: [fighter(left, START[0]), fighter(right, START[1])],
            bg,
            queue: Vec::new(),
            timer: 0.0,
        }
    }

    /// Queue a move of `side`; returns the sound it starts with.
    pub fn act(&mut self, side: DuelSide, act: DuelAct) -> Option<&'static str> {
        let i = index(side);
        let me = self.fighters[i].x;
        let other = self.fighters[1 - i].x;
        // The left fighter moves right towards the other, the right one left.
        let toward = if i == 0 { 1 } else { -1 };
        let mut steps: Vec<(Pose, bool)> = Vec::new();
        let sound = match act {
            DuelAct::Charge => {
                let target = other - toward * CLOSE;
                let mut x = me;
                let mut n = 0u8;
                while (target - x) * toward > 0 {
                    x += toward;
                    steps.push((
                        Pose {
                            x,
                            frame: 1 + n % 3,
                        },
                        false,
                    ));
                    n += 1;
                }
                steps.push((Pose { x, frame: 0 }, false));
                Some(sfx::STEP)
            }
            DuelAct::Strike(frame) => {
                steps.push((Pose { x: me, frame }, false));
                steps.push((
                    Pose {
                        x: me,
                        frame: frame + 1,
                    },
                    false,
                ));
                steps.push((
                    Pose {
                        x: me,
                        frame: frame + 1,
                    },
                    false,
                ));
                steps.push((Pose { x: me, frame: 0 }, false));
                Some(sfx::HIT_HEAVY)
            }
            DuelAct::Fall => {
                steps.push((Pose { x: me, frame: 12 }, false));
                steps.push((Pose { x: me, frame: 12 }, false));
                steps.push((Pose { x: me, frame: 13 }, false));
                Some(sfx::RETREAT)
            }
            DuelAct::Flee => {
                // Off the stage, however far that is.
                let mut x = me;
                let mut n = 0u8;
                while !off_stage(x) {
                    x -= toward;
                    steps.push((
                        Pose {
                            x,
                            frame: 1 + n % 3,
                        },
                        true,
                    ));
                    n = n.wrapping_add(1);
                }
                Some(sfx::STEP)
            }
            DuelAct::Back => {
                let start = START[i];
                // Riding back to the start faces away from the other fighter.
                let away = (start - me).signum() == -toward;
                let mut x = me;
                let mut n = 0u8;
                while x != start {
                    x += (start - x).signum();
                    steps.push((
                        Pose {
                            x,
                            frame: 1 + n % 3,
                        },
                        away,
                    ));
                    n += 1;
                }
                steps.push((Pose { x, frame: 0 }, false));
                Some(sfx::STEP)
            }
        };
        self.queue
            .extend(steps.into_iter().map(|(pose, turned)| (i, pose, turned)));
        sound
    }

    /// Whether a move is still being shown.
    pub fn busy(&self) -> bool {
        !self.queue.is_empty()
    }

    /// Advance by `dt` seconds (`fast`: the whole queue at once).
    pub fn update(&mut self, dt: f32, fast: bool) {
        if fast {
            for (i, pose, turned) in std::mem::take(&mut self.queue) {
                self.apply(i, pose, turned);
            }
            return;
        }
        self.timer += dt;
        while self.timer >= STEP && !self.queue.is_empty() {
            self.timer -= STEP;
            let (i, pose, turned) = self.queue.remove(0);
            self.apply(i, pose, turned);
        }
        if self.queue.is_empty() {
            self.timer = 0.0;
        }
    }

    fn apply(&mut self, i: usize, pose: Pose, turned: bool) {
        let f = &mut self.fighters[i];
        f.x = pose.x;
        f.frame = pose.frame;
        f.turned = turned;
        f.gone = off_stage(pose.x);
    }

    /// Textures the scene needs (for preloading).
    pub fn textures(&self) -> Vec<String> {
        let mut keys: Vec<String> = self
            .fighters
            .iter()
            .map(|f| format!("duel/{}", f.officer))
            .collect();
        keys.extend(["duel/left".to_string(), "duel/right".to_string()]);
        keys.extend(self.bg.iter().map(|b| format!("duel/{b}")));
        keys
    }

    /// Draw the scene centred in `area`, as large as fits (whole-number scales from 1 up).
    pub fn draw(&self, ctx: &Ctx, area: Rect) {
        let stage = stage_rect(area);
        let scale = stage.w / STAGE_W;
        match self
            .bg
            .as_ref()
            .and_then(|b| ctx.media.texture(&format!("duel/{b}")))
        {
            Some(tex) => draw_texture_ex(
                &tex,
                stage.x,
                stage.y,
                WHITE,
                DrawTextureParams {
                    dest_size: Some(stage.size()),
                    ..Default::default()
                },
            ),
            None => fill_rect(stage, Color::from_hex(0x2a3a22)),
        }
        for (i, f) in self.fighters.iter().enumerate() {
            if f.gone {
                continue;
            }
            let Some(sheet) = sheet(ctx, &f.officer, i) else {
                continue;
            };
            let size = FRAME * scale;
            let x = (stage.x + f.x as f32 * CELL * scale - size / 2.0 + CELL * scale).round();
            let y = (stage.y + ROW * CELL * scale).round();
            // The sheets face right: the right fighter is mirrored, and so is one turning away.
            let mirrored = (i == 1) != f.turned;
            // Only the part on the stage is drawn (a rider comes and goes at its edges).
            let (cut_left, cut_right) =
                ((stage.x - x).max(0.0), (x + size - stage.right()).max(0.0));
            if cut_left + cut_right >= size {
                continue;
            }
            // In sheet pixels; a mirrored frame is cut on the other side of the sheet.
            let (src_left, src_right) = if mirrored {
                (cut_right / scale, cut_left / scale)
            } else {
                (cut_left / scale, cut_right / scale)
            };
            draw_texture_ex(
                &sheet,
                x + cut_left,
                y,
                WHITE,
                DrawTextureParams {
                    dest_size: Some(vec2(size - cut_left - cut_right, size)),
                    source: Some(Rect::new(
                        f32::from(f.frame) * FRAME + src_left,
                        0.0,
                        FRAME - src_left - src_right,
                        FRAME,
                    )),
                    flip_x: mirrored,
                    ..Default::default()
                },
            );
        }
        self.draw_names(ctx, stage);
    }

    /// Draw each fighter's name in a small window at the top of their half of `stage`.
    ///
    /// Why the name is not left to the sheet: the art says nothing about who the rider is, and a
    /// duel is often the first time an officer appears. The windows sit on the sky band (the
    /// riders start at [`ROW`]), so they cover no fighting. A fighter who has left the stage
    /// takes their name with them.
    fn draw_names(&self, ctx: &Ctx, stage: Rect) {
        let gfx = &ctx.gfx;
        let small = TextStyle::small(theme::TEXT).shadow(theme::TEXT_SHADOW);
        for (i, r) in name_rects(stage).into_iter().enumerate() {
            let f = &self.fighters[i];
            if f.gone {
                continue;
            }
            draw_window_ex(r, WindowStyle::Panel, 0.85);
            // A pack may name an officer longer than the window: a cut name still identifies the
            // fighter better than one spilling over the stage.
            let name = gfx.elide(fighter_name(ctx, f), FontId::Small, 1, r.w - 2.0 * NAME_PAD);
            gfx.text_aligned(&name, r.x, r.y + 2.0, r.w, Align::Center, small);
        }
    }
}

/// The name to show for `f`: the pack's officer name, else the id the scene gave (a duel may
/// name someone outside the pack's officers).
fn fighter_name<'a>(ctx: &'a Ctx, f: &'a Fighter) -> &'a str {
    ctx.pack
        .as_deref()
        .and_then(|p| p.officer(&f.officer))
        .map_or(f.officer.as_str(), |o| o.name.as_str())
}

/// Whether a rider at `x` is entirely off the stage (a frame reaches 2 cells left and 4 right
/// of its cell).
fn off_stage(x: i32) -> bool {
    x + 4 <= 0 || x - 2 >= STAGE_CELLS
}

/// Height of the name windows over the stage.
const NAME_H: f32 = 14.0;
/// Gap between a name window and the stage's edge.
const NAME_MARGIN: f32 = 4.0;
/// Narrowest a name window gets, so a one-character name still reads as a window.
const NAME_MIN_W: f32 = 24.0;
/// Room left inside a name window on each side of the text.
const NAME_PAD: f32 = 3.0;

/// The name windows of the two fighters on `stage`: the left one along the left edge and the
/// right one along the right edge, both at the top (the sky band the riders do not reach).
fn name_rects(stage: Rect) -> [Rect; 2] {
    let y = stage.y + NAME_MARGIN;
    // A quarter of the stage each: enough for a three-character name at 1:1, and they never meet
    // on the narrowest stage.
    let w = (stage.w / 4.0).max(NAME_MIN_W).round();
    [
        Rect::new(stage.x + NAME_MARGIN, y, w, NAME_H),
        Rect::new(stage.right() - NAME_MARGIN - w, y, w, NAME_H),
    ]
}

/// Width of the stage in stage pixels.
const STAGE_W: f32 = STAGE_CELLS as f32 * CELL;

/// Where the stage is drawn in `area`: centred, as large as fits, at a whole-number scale when
/// it fits at 1 or more (pixel art stays sharp).
fn stage_rect(area: Rect) -> Rect {
    let fit = (area.w / STAGE_W).min(area.h / STAGE_H);
    let scale = if fit >= 1.0 { fit.floor() } else { fit };
    let (w, h) = (STAGE_W * scale, STAGE_H * scale);
    Rect::new(
        (area.x + (area.w - w) / 2.0).round(),
        (area.y + (area.h - h) / 2.0).round(),
        w,
        h,
    )
}

/// The sheet of a fighter: the officer's own, else the side's.
fn sheet(ctx: &Ctx, officer: &str, side: usize) -> Option<Texture2D> {
    let own = format!("duel/{officer}");
    if ctx.media.texture_state(&own) == AssetState::Ready {
        return ctx.media.texture(&own);
    }
    ctx.media
        .texture(if side == 0 { "duel/left" } else { "duel/right" })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_terrain_background_is_the_one_under_the_left_fighter() {
        let terrain: BTreeMap<String, String> = [("guan_yu", "forest"), ("hua_xiong", "plain")]
            .iter()
            .map(|&(o, t)| (o.to_string(), t.to_string()))
            .collect();
        let bg = |bg: Option<&str>, left: &str, right: &str| background(bg, left, right, &terrain);
        assert_eq!(
            bg(Some(DUEL_TERRAIN), "guan_yu", "hua_xiong").as_deref(),
            Some("terrain_forest")
        );
        // The left one not on the field: the right one's.
        assert_eq!(
            bg(Some(DUEL_TERRAIN), "zhang_fei", "hua_xiong").as_deref(),
            Some("terrain_plain")
        );
        // Neither (outside a battle): a plain stage. A named background stays.
        assert_eq!(bg(Some(DUEL_TERRAIN), "zhang_fei", "lu_bu"), None);
        assert_eq!(
            bg(Some("field"), "guan_yu", "hua_xiong").as_deref(),
            Some("field")
        );
        assert_eq!(bg(None, "guan_yu", "hua_xiong"), None);
    }

    fn xs(v: &DuelView) -> [i32; 2] {
        [v.fighters[0].x, v.fighters[1].x]
    }

    /// The name windows stay on the stage, over its sky band, and never meet each other — on the
    /// smallest stage (1:1) and on a large one.
    #[test]
    fn the_name_windows_stay_on_the_stage_and_apart() {
        for area in [
            Rect::new(16.0, 32.0, 416.0, 352.0),
            Rect::new(0.0, 0.0, 1280.0, 600.0),
            Rect::new(0.0, 0.0, 208.0, 400.0),
        ] {
            let stage = stage_rect(area);
            let [left, right] = name_rects(stage);
            for r in [left, right] {
                assert!(
                    r.x >= stage.x && r.right() <= stage.right(),
                    "{r:?} {stage:?}"
                );
                assert!(r.y >= stage.y && r.bottom() <= stage.bottom(), "{r:?}");
                // Above the riders: they start at ROW cells from the top.
                assert!(r.bottom() <= stage.y + ROW * CELL * (stage.w / STAGE_W));
            }
            assert!(left.right() < right.x, "{left:?} {right:?}");
            assert_eq!(left.y, right.y);
        }
    }

    #[test]
    fn a_charge_stops_close_to_the_other_fighter() {
        let mut v = DuelView::new("guan_yu", "hua_xiong", None);
        assert_eq!(v.act(DuelSide::Left, DuelAct::Charge), Some(sfx::STEP));
        assert!(v.busy());
        v.update(0.0, true);
        assert_eq!(xs(&v), [START[1] - CLOSE, START[1]]);
        assert_eq!(v.fighters[0].frame, 0);
        // The right one charging closes in from the other side.
        let mut v = DuelView::new("a", "b", None);
        v.act(DuelSide::Right, DuelAct::Charge);
        v.update(0.0, true);
        assert_eq!(xs(&v), [START[0], START[0] + CLOSE]);
    }

    #[test]
    fn moves_play_step_by_step_and_end_standing_or_lying() {
        let mut v = DuelView::new("a", "b", None);
        v.act(DuelSide::Left, DuelAct::Strike(6));
        v.update(STEP, false);
        assert_eq!(v.fighters[0].frame, 6);
        v.update(STEP, false);
        assert_eq!(v.fighters[0].frame, 7);
        v.update(STEP * 5.0, false);
        assert!(!v.busy());
        assert_eq!(v.fighters[0].frame, 0);
        v.act(DuelSide::Right, DuelAct::Fall);
        v.update(1.0, false);
        assert_eq!(v.fighters[1].frame, 13);
    }

    #[test]
    fn the_stage_keeps_its_shape_and_sharp_scales() {
        // The original's map hole shows it at 1:1.
        assert_eq!(
            stage_rect(Rect::new(16.0, 32.0, 416.0, 352.0)),
            Rect::new(16.0, 104.0, 416.0, 208.0)
        );
        let big = stage_rect(Rect::new(0.0, 0.0, 1280.0, 600.0));
        assert_eq!((big.w, big.h), (832.0, 416.0));
        let small = stage_rect(Rect::new(0.0, 0.0, 208.0, 400.0));
        assert_eq!((small.w, small.h), (208.0, 104.0));
    }

    #[test]
    fn a_fleeing_fighter_leaves_the_stage_and_back_returns() {
        let mut v = DuelView::new("a", "b", None);
        v.act(DuelSide::Right, DuelAct::Flee);
        v.update(0.0, true);
        assert!(v.fighters[1].gone);
        assert!(v.fighters[1].turned);
        // From wherever the flight starts (here after a charge to the right).
        let mut v = DuelView::new("a", "b", None);
        v.act(DuelSide::Left, DuelAct::Charge);
        v.act(DuelSide::Left, DuelAct::Flee);
        v.update(STEP, false);
        assert!(!v.fighters[0].gone);
        v.update(0.0, true);
        assert!(v.fighters[0].gone);
        assert!(off_stage(v.fighters[0].x) && !off_stage(v.fighters[0].x + 1));
        let mut v = DuelView::new("a", "b", None);
        v.act(DuelSide::Left, DuelAct::Charge);
        v.act(DuelSide::Left, DuelAct::Back);
        v.update(0.0, true);
        assert_eq!(xs(&v), START);
    }
}
