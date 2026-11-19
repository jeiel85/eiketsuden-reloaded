//! Settings overlay: volumes, text speed, battle animation speed, fullscreen (native), the quick
//! save slot, whether the player's phase ends by itself, and the view-only choices beyond the
//! original (`docs/DECISIONS.md` D25: portraits, danger range, battle presentation; D27: map and
//! unit art), which all start at the original's look.
//!
//! Values change with left/right, the ◀ ▶ arrows or confirm (steps forward) and apply
//! immediately (the effect volume plays a sample); leaving the screen persists them through
//! the storage layer.
//!
//! A row explains itself when the pointer rests on it ([`crate::ui::tooltip`]): the label fits on
//! the row, the meaning of the setting does not.

use crate::app::{Ctx, RemakeMaps, Screen, Transition};
use crate::audio::sfx;
use crate::gfx::{fill_rect, Align, TextStyle};
use crate::settings::{cycle, ArtStyle, BattleFx, BattleSpeed, PortraitStyle, Settings, TextSpeed};
use crate::ui::format;
use crate::ui::menu::{Menu, MenuEvent, MenuItem};
use crate::ui::theme;
use crate::ui::tooltip::{draw_tooltip, HoverTimer};
use crate::ui::window::draw_window;
use macroquad::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    Master,
    Bgm,
    Sfx,
    TextSpeed,
    BattleSpeed,
    Fullscreen,
    /// The quick save slot of F5 / F9 and the menus' 순간 저장 (also F6).
    QuickSlot,
    Portraits,
    /// The original's or the new map and unit art (D27).
    Art,
    DangerRange,
    BattleFx,
    /// End the phase by itself once every unit has acted (see [`Settings::auto_end_turn`]).
    AutoEndTurn,
    Defaults,
    Back,
}

const WIDTH: f32 = 260.0;

impl Row {
    /// What the row changes, for its tooltip. `None` for 돌아가기, whose label says everything
    /// there is to know.
    fn explain(self) -> Option<&'static str> {
        match self {
            Row::Master => Some("모든 소리를 한 번에 조절합니다."),
            Row::Bgm => Some("배경 음악의 음량입니다."),
            Row::Sfx => Some("효과음의 음량입니다. 바꾸면 미리 들려 줍니다."),
            Row::TextSpeed => Some("대사가 한 글자씩 나타나는 속도입니다."),
            Row::BattleSpeed => Some("부대 이동·공격 연출이 재생되는 속도입니다."),
            Row::Fullscreen => Some("창을 화면 가득 채웁니다. Alt+Enter·F11로도 바꿀 수 있습니다."),
            Row::QuickSlot => {
                Some("F5·F9와 메뉴의 순간 저장이 쓰는 칸입니다. F6으로도 바꿀 수 있습니다.")
            }
            Row::Portraits => Some("대사창의 얼굴을 원작의 것과 공개 초상화 중에서 고릅니다."),
            Row::Art => Some("전투 지도와 부대 그림을 원작의 것과 새 그림 중에서 고릅니다."),
            Row::DangerRange => {
                Some("아군 차례에 적이 다음 차례에 닿는 칸을 표시합니다 (원작에 없는 표시).")
            }
            Row::BattleFx => Some("타격에 숫자 피해량과 흔들림을 더합니다."),
            Row::AutoEndTurn => Some(
                "끄면 부대가 모두 행동해도 '턴 종료'를 누를 때까지 기다립니다. 켜면 원작처럼 저절로 넘어갑니다.",
            ),
            Row::Defaults => Some(
                "화면 표시 설정을 기본값으로 되돌립니다. 음량·창 모드·원작 폴더는 그대로 둡니다.",
            ),
            Row::Back => None,
        }
    }
}

pub struct SettingsScreen {
    rows: Vec<Row>,
    menu: Menu,
    changed: bool,
    /// Which row the pointer has rested on, for its explanation.
    hover: HoverTimer,
}

impl SettingsScreen {
    pub fn new() -> SettingsScreen {
        let mut rows = vec![
            Row::Master,
            Row::Bgm,
            Row::Sfx,
            Row::TextSpeed,
            Row::BattleSpeed,
        ];
        if crate::platform::can_toggle_fullscreen() {
            rows.push(Row::Fullscreen);
        }
        rows.extend([
            Row::QuickSlot,
            Row::Portraits,
            Row::Art,
            Row::DangerRange,
            Row::BattleFx,
            Row::AutoEndTurn,
        ]);
        rows.push(Row::Defaults);
        rows.push(Row::Back);
        SettingsScreen {
            rows,
            menu: Menu::new(Vec::new()),
            changed: false,
            hover: HoverTimer::default(),
        }
    }

    /// Input: the settings, whether the chain holds the original mode's pack below the top
    /// ([`crate::platform::DataRoot::has_original_layer`]) and whether this launch's conversion
    /// drew the new art's maps ([`RemakeMaps`]). Without an original pack (the web build, the
    /// base pack alone) the face and art choices change nothing, so their rows are shown
    /// disabled.
    fn items(&self, s: &Settings, faces: bool, maps: RemakeMaps) -> Vec<MenuItem> {
        self.rows
            .iter()
            .map(|row| match row {
                Row::Master => MenuItem::new("전체 음량")
                    .detail(format::percent(s.master_volume))
                    .adjustable(),
                Row::Bgm => MenuItem::new("배경음")
                    .detail(format::percent(s.bgm_volume))
                    .adjustable(),
                Row::Sfx => MenuItem::new("효과음")
                    .detail(format::percent(s.sfx_volume))
                    .adjustable(),
                Row::TextSpeed => MenuItem::new("글자 속도")
                    .detail(s.text_speed.label())
                    .adjustable(),
                Row::BattleSpeed => MenuItem::new("전투 속도")
                    .detail(s.battle_speed.label())
                    .adjustable(),
                Row::Fullscreen => MenuItem::new("전체 화면")
                    .detail(if s.fullscreen { "켬" } else { "끔" })
                    .adjustable(),
                Row::QuickSlot => MenuItem::new("순간 저장 칸")
                    .detail(format!("{} / {}", s.quick_slot, crate::saves::QUICK_SLOTS))
                    .adjustable(),
                Row::Portraits if !faces => MenuItem::new("얼굴")
                    .detail("원작 데이터 없음")
                    .enabled(false),
                Row::Portraits => MenuItem::new("얼굴")
                    .detail(s.portraits.label())
                    .adjustable(),
                Row::Art if !faces => MenuItem::new("그림")
                    .detail("원작 데이터 없음")
                    .enabled(false),
                Row::Art if s.art == ArtStyle::Remake && maps == RemakeMaps::NotDrawn => {
                    MenuItem::new("그림")
                        .detail("새 그림 · 맵은 재시작 후")
                        .adjustable()
                }
                Row::Art if s.art == ArtStyle::Remake && maps == RemakeMaps::Failed => {
                    MenuItem::new("그림")
                        .detail("새 그림 · 맵 변환 실패")
                        .adjustable()
                }
                Row::Art => MenuItem::new("그림").detail(s.art.label()).adjustable(),
                Row::DangerRange => MenuItem::new("위험 범위")
                    .detail(if s.danger_range { "켬" } else { "끔" })
                    .adjustable(),
                Row::BattleFx => MenuItem::new("전투 연출")
                    .detail(s.battle_fx.label())
                    .adjustable(),
                Row::AutoEndTurn => MenuItem::new("턴 자동 종료")
                    .detail(if s.auto_end_turn { "켬" } else { "끔" })
                    .adjustable(),
                Row::Defaults => MenuItem::new("기본값으로"),
                Row::Back => MenuItem::new("돌아가기"),
            })
            .collect()
    }

    fn refresh(&mut self, ctx: &Ctx) {
        let items = self.items(
            &ctx.settings,
            ctx.data_root.has_original_layer(),
            ctx.remake_maps,
        );
        self.menu.set_items(items);
    }

    fn adjust(&mut self, ctx: &mut Ctx, row: Row, delta: i32) {
        let s = &mut ctx.settings;
        let step = |v: u8| (i32::from(v) + delta * 10).clamp(0, 100) as u8;
        match row {
            Row::Master => s.master_volume = step(s.master_volume),
            Row::Bgm => s.bgm_volume = step(s.bgm_volume),
            Row::Sfx => s.sfx_volume = step(s.sfx_volume),
            Row::TextSpeed => s.text_speed = cycle(&TextSpeed::ALL, s.text_speed, delta),
            Row::BattleSpeed => s.battle_speed = cycle(&BattleSpeed::ALL, s.battle_speed, delta),
            Row::Fullscreen => {
                s.fullscreen = !s.fullscreen;
                // Fullscreen switches right away; the rest is persisted on leaving.
                ctx.commit_settings();
            }
            Row::QuickSlot => {
                let slots: Vec<u8> = (1..=crate::saves::QUICK_SLOTS).collect();
                s.quick_slot = cycle(&slots, s.quick_slot, delta);
            }
            Row::Portraits => s.portraits = cycle(&PortraitStyle::ALL, s.portraits, delta),
            Row::Art => s.art = cycle(&ArtStyle::ALL, s.art, delta),
            Row::DangerRange => s.danger_range = !s.danger_range,
            Row::BattleFx => s.battle_fx = cycle(&BattleFx::ALL, s.battle_fx, delta),
            Row::AutoEndTurn => s.auto_end_turn = !s.auto_end_turn,
            Row::Defaults | Row::Back => return,
        }
        ctx.audio.apply_settings(&ctx.settings);
        if matches!(row, Row::Master | Row::Sfx) {
            ctx.sfx(sfx::CONFIRM);
        }
        self.changed = true;
        self.refresh(ctx);
    }

    fn leave(&mut self, ctx: &mut Ctx) -> Transition {
        if self.changed {
            ctx.commit_settings();
            self.changed = false;
        }
        Transition::Pop
    }
}

impl Default for SettingsScreen {
    fn default() -> Self {
        SettingsScreen::new()
    }
}

impl Screen for SettingsScreen {
    fn name(&self) -> &'static str {
        "settings"
    }

    fn is_overlay(&self) -> bool {
        true
    }

    fn on_enter(&mut self, ctx: &mut Ctx, _how: crate::app::Enter) {
        let items = self.items(
            &ctx.settings,
            ctx.data_root.has_original_layer(),
            ctx.remake_maps,
        );
        self.menu = placed(items, ctx.gfx.size());
        self.hover = HoverTimer::default();
    }

    fn update(&mut self, ctx: &mut Ctx) -> Transition {
        // Which row the pointer rests on, for its explanation (drawn by `draw`).
        let row = ctx
            .input
            .pointer()
            .and_then(|p| self.menu.row_at(p))
            .map(|i| i as u64);
        self.hover.update(row, ctx.dt);
        match self.menu.update(ctx) {
            MenuEvent::Adjust(i, d) => {
                let row = self.rows[i];
                self.adjust(ctx, row, d);
            }
            MenuEvent::Selected(i) => match self.rows[i] {
                Row::Defaults => {
                    // Keeps the window mode and the original-mode folder (see the settings).
                    ctx.settings.reset_to_defaults();
                    ctx.audio.apply_settings(&ctx.settings);
                    self.changed = true;
                    self.refresh(ctx);
                    ctx.toast("기본 설정으로 되돌렸습니다.");
                }
                Row::Back => return self.leave(ctx),
                row => self.adjust(ctx, row, 1),
            },
            MenuEvent::Cancelled => return self.leave(ctx),
            _ => {}
        }
        Transition::None
    }

    fn draw(&self, ctx: &Ctx) {
        let canvas = ctx.gfx.size();
        fill_rect(ctx.gfx.screen(), Color::new(0.0, 0.0, 0.02, 0.55));
        let frame = window_rect(self.menu.rect());
        draw_window(frame);
        ctx.gfx.text_aligned(
            "설정",
            frame.x,
            frame.y + 6.0,
            frame.w,
            Align::Center,
            TextStyle::main(theme::TEXT_ACCENT).shadow(theme::TEXT_SHADOW),
        );
        self.menu.draw(ctx);
        // The storage location line, when the window leaves room for it: on a canvas too short
        // for both, the rows win (a line drawn over the last row would hide a setting).
        let window = window_rect(self.menu.rect());
        let where_ = format!("저장 위치: {}", ctx.storage.location());
        let lines = ctx
            .gfx
            .wrap(&where_, crate::gfx::FontId::Small, 1, canvas.x - 20.0);
        if storage_line_fits(window, canvas, lines.len()) {
            ctx.gfx.text_lines(
                &lines,
                10.0,
                canvas.y - 4.0 - 12.0 * lines.len() as f32,
                TextStyle::small(theme::TEXT_DISABLED),
            );
        }
        // The explanation of the row under the pointer, once it has rested there.
        if self.hover.visible() {
            if let (Some(i), Some(p)) = (self.hover.target(), ctx.input.pointer()) {
                if let Some(why) = self.rows.get(i as usize).and_then(|r| r.explain()) {
                    draw_tooltip(&ctx.gfx, why, p);
                }
            }
        }
    }
}

/// Height of the storage location line at the bottom of the canvas.
const STORAGE_LINE: f32 = 16.0;

/// Space the window keeps above the first row for its heading, and below the last row.
const HEADING: f32 = 24.0;
const FOOTING: f32 = 6.0;

/// The settings menu of `items` on a `canvas`: the window (menu plus its heading, see
/// [`window_rect`]) centred in the space above the storage location line. When the rows do not
/// fit that space (a small canvas, or settings that grew), the list shows as many as fit and
/// scrolls (arrows, wheel, drag — [`Menu`] handles it).
fn placed(items: Vec<MenuItem>, canvas: Vec2) -> Menu {
    let fits = canvas.y - STORAGE_LINE - HEADING - FOOTING;
    // At least one row, so the menu is never empty; on a canvas too small even for that the
    // window takes the top of the canvas and [`storage_line_fits`] drops the line below it.
    let max_rows = (fits / theme::ROW_HEIGHT).floor().max(1.0) as usize;
    let mut menu = Menu::new(items);
    menu.framed = false;
    if menu.items.len() > max_rows {
        menu = menu.rows(max_rows);
    }
    let window_h = menu.rect().h + HEADING + FOOTING;
    let top = ((canvas.y - STORAGE_LINE - window_h) / 2.0)
        .round()
        .max(0.0);
    menu.set_position(((canvas.x - WIDTH) / 2.0).round() + 6.0, top + HEADING);
    menu.set_width(WIDTH - 12.0);
    menu
}

/// Whether the storage location line (`lines` of them) still fits under the settings `window` on
/// a `canvas`. On a canvas too short for the window and the line, the line is dropped rather than
/// drawn over the last rows.
fn storage_line_fits(window: Rect, canvas: Vec2, lines: usize) -> bool {
    window.bottom() <= canvas.y - 4.0 - 12.0 * lines as f32
}

/// The window drawn around the settings menu `m`, with its heading.
fn window_rect(m: Rect) -> Rect {
    Rect::new(m.x - 6.0, m.y - HEADING, WIDTH, m.h + HEADING + FOOTING)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// With every row (the native build's fullscreen and the quick save slot), the window and
    /// the storage line under it still fit the smallest canvas.
    #[test]
    fn every_row_fits_the_smallest_canvas() {
        let mut screen = SettingsScreen::new();
        if !screen.rows.contains(&Row::Fullscreen) {
            screen.rows.insert(5, Row::Fullscreen);
        }
        assert!(screen.rows.contains(&Row::QuickSlot));
        let canvas = crate::gfx::DEFAULT_CANVAS;
        let menu = placed(
            screen.items(&Settings::default(), true, RemakeMaps::Ready),
            canvas,
        );
        let window = window_rect(menu.rect());
        assert!(window.y >= 0.0, "{window:?}");
        // The storage line takes the last 16 pixels.
        assert!(window.bottom() <= canvas.y - 16.0, "{window:?}");
    }

    /// On a canvas too short for every row the window stays inside it and the list scrolls
    /// instead (the cursor row is always drawn).
    #[test]
    fn a_short_canvas_scrolls_the_list_instead_of_overflowing() {
        let screen = SettingsScreen::new();
        let canvas = Vec2::new(480.0, 200.0);
        let mut menu = placed(
            screen.items(&Settings::default(), true, RemakeMaps::Ready),
            canvas,
        );
        let window = window_rect(menu.rect());
        assert!(window.y >= 0.0, "{window:?}");
        assert!(window.bottom() <= canvas.y - STORAGE_LINE, "{window:?}");
        assert!(menu.visible_rows < menu.items.len(), "nothing to scroll");
        menu.set_cursor(menu.items.len() - 1);
        assert!(menu.cursor() >= menu.visible_rows);
        assert!(
            menu.row_rect(menu.cursor()).bottom() <= menu.rect().bottom() + 0.5,
            "the cursor row is out of view"
        );
    }

    /// The quick save slot row steps through the slots, wrapping.
    #[test]
    fn the_quick_slot_row_shows_and_steps_the_slot() {
        let screen = SettingsScreen::new();
        let at = screen
            .rows
            .iter()
            .position(|&r| r == Row::QuickSlot)
            .unwrap();
        let mut s = Settings::default();
        assert_eq!(
            screen.items(&s, true, RemakeMaps::Ready)[at]
                .detail
                .as_deref(),
            Some("1 / 4")
        );
        let slots: Vec<u8> = (1..=crate::saves::QUICK_SLOTS).collect();
        assert_eq!(cycle(&slots, 1, -1), 4);
        s.next_quick_slot();
        assert_eq!(s.quick_save_slot(), crate::saves::SaveSlot::Quick(2));
        s.quick_slot = 4;
        s.next_quick_slot();
        assert_eq!(s.quick_slot, 1);
    }

    #[test]
    fn the_face_row_is_disabled_without_an_original_layer() {
        let screen = SettingsScreen::new();
        let at = screen
            .rows
            .iter()
            .position(|&r| r == Row::Portraits)
            .unwrap();
        let s = Settings::default();
        let off = &screen.items(&s, false, RemakeMaps::Ready)[at];
        assert!(!off.enabled && !off.adjustable);
        assert_eq!(off.detail.as_deref(), Some("원작 데이터 없음"));
        let on = &screen.items(&s, true, RemakeMaps::Ready)[at];
        assert!(on.enabled && on.adjustable);
        assert_eq!(on.detail.as_deref(), Some(s.portraits.label()));
    }

    /// The art row (D27): disabled without an original layer; with the new art chosen after a
    /// conversion that left out its maps, it says the maps follow at the next launch — and says
    /// the conversion failed when no restart would help.
    #[test]
    fn the_art_row_says_when_the_maps_follow() {
        use RemakeMaps::{Failed, NotDrawn, Ready};
        let screen = SettingsScreen::new();
        let at = screen.rows.iter().position(|&r| r == Row::Art).unwrap();
        let mut s = Settings::default();
        let off = &screen.items(&s, false, Ready)[at];
        assert!(!off.enabled);
        assert_eq!(off.detail.as_deref(), Some("원작 데이터 없음"));
        assert_eq!(
            screen.items(&s, true, NotDrawn)[at].detail.as_deref(),
            Some("원작")
        );
        s.art = ArtStyle::Remake;
        assert_eq!(
            screen.items(&s, true, Ready)[at].detail.as_deref(),
            Some("새 그림")
        );
        let pending = &screen.items(&s, true, NotDrawn)[at];
        assert!(pending.adjustable);
        assert_eq!(pending.detail.as_deref(), Some("새 그림 · 맵은 재시작 후"));
        // A failed drawing is not something a restart fixes, so the row does not promise one.
        assert_eq!(
            screen.items(&s, true, Failed)[at].detail.as_deref(),
            Some("새 그림 · 맵 변환 실패")
        );
    }

    /// Every setting row explains itself (the tooltip text is not empty and stays short enough
    /// for the tooltip box), and the rows whose label says it all have none. The explanation
    /// exists for every row of the screen, so a new row cannot be added without one.
    #[test]
    fn every_row_explains_itself() {
        let screen = SettingsScreen::new();
        let mut action_rows = 0;
        for row in &screen.rows {
            match row.explain() {
                Some(why) => {
                    assert!(!why.is_empty(), "{row:?} has an empty explanation");
                    // The tooltip wraps at 180 pixels of the small font: about 30 characters a
                    // line, and four lines still fit above the message box.
                    assert!(why.chars().count() <= 120, "{row:?}: {why}");
                }
                None => action_rows += 1,
            }
        }
        // Only 돌아가기 does not explain itself.
        assert_eq!(action_rows, 1, "a setting row lost its explanation");
        assert!(Row::Defaults.explain().is_some());
        assert_eq!(Row::Back.explain(), None);
    }

    /// The auto end turn row shows the setting and flips it (default: on, as before).
    #[test]
    fn the_auto_end_row_flips_the_setting() {
        let screen = SettingsScreen::new();
        let at = screen
            .rows
            .iter()
            .position(|&r| r == Row::AutoEndTurn)
            .unwrap();
        let s = Settings::default();
        assert!(s.auto_end_turn);
        assert_eq!(
            screen.items(&s, true, RemakeMaps::Ready)[at]
                .detail
                .as_deref(),
            Some("켬")
        );
        let mut flipped = s;
        flipped.auto_end_turn = false;
        assert_eq!(
            screen.items(&flipped, true, RemakeMaps::Ready)[at]
                .detail
                .as_deref(),
            Some("끔")
        );
    }
}
