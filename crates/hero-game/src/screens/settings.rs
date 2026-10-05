//! Settings overlay: volumes, text speed, battle animation speed, fullscreen (native), and the
//! view-only choices beyond the original (`docs/DECISIONS.md` D25: portraits, danger range,
//! battle presentation), which all start at the original's look.
//!
//! Values change with left/right, the ◀ ▶ arrows or confirm (steps forward) and apply
//! immediately (the effect volume plays a sample); leaving the screen persists them through
//! the storage layer.

use crate::app::{Ctx, Screen, Transition};
use crate::audio::sfx;
use crate::gfx::{fill_rect, Align, TextStyle};
use crate::settings::{cycle, BattleFx, BattleSpeed, PortraitStyle, Settings, TextSpeed};
use crate::ui::format;
use crate::ui::menu::{Menu, MenuEvent, MenuItem};
use crate::ui::theme;
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
    DangerRange,
    BattleFx,
    Defaults,
    Back,
}

const WIDTH: f32 = 260.0;

pub struct SettingsScreen {
    rows: Vec<Row>,
    menu: Menu,
    changed: bool,
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
            Row::DangerRange,
            Row::BattleFx,
        ]);
        rows.push(Row::Defaults);
        rows.push(Row::Back);
        SettingsScreen {
            rows,
            menu: Menu::new(Vec::new()),
            changed: false,
        }
    }

    /// Input: the settings and whether the chain holds the original mode's pack below the top
    /// ([`crate::platform::DataRoot::has_original_layer`]). Without one (the web build, the
    /// base pack alone) the face choice changes nothing, so its row is shown disabled.
    fn items(&self, s: &Settings, faces: bool) -> Vec<MenuItem> {
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
                Row::DangerRange => MenuItem::new("위험 범위")
                    .detail(if s.danger_range { "켬" } else { "끔" })
                    .adjustable(),
                Row::BattleFx => MenuItem::new("전투 연출")
                    .detail(s.battle_fx.label())
                    .adjustable(),
                Row::Defaults => MenuItem::new("기본값으로"),
                Row::Back => MenuItem::new("돌아가기"),
            })
            .collect()
    }

    fn refresh(&mut self, ctx: &Ctx) {
        let items = self.items(&ctx.settings, ctx.data_root.has_original_layer());
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
            Row::DangerRange => s.danger_range = !s.danger_range,
            Row::BattleFx => s.battle_fx = cycle(&BattleFx::ALL, s.battle_fx, delta),
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
        let items = self.items(&ctx.settings, ctx.data_root.has_original_layer());
        self.menu = placed(items, ctx.gfx.size());
    }

    fn update(&mut self, ctx: &mut Ctx) -> Transition {
        match self.menu.update(ctx) {
            MenuEvent::Adjust(i, d) => {
                let row = self.rows[i];
                self.adjust(ctx, row, d);
            }
            MenuEvent::Selected(i) => match self.rows[i] {
                Row::Defaults => {
                    ctx.settings = Settings {
                        // Keep the window mode; resetting it unexpectedly is jarring.
                        fullscreen: ctx.settings.fullscreen,
                        ..Settings::default()
                    };
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
        let where_ = format!("저장 위치: {}", ctx.storage.location());
        let lines = ctx
            .gfx
            .wrap(&where_, crate::gfx::FontId::Small, 1, canvas.x - 20.0);
        ctx.gfx.text_lines(
            &lines,
            10.0,
            canvas.y - 4.0 - 12.0 * lines.len() as f32,
            TextStyle::small(theme::TEXT_DISABLED),
        );
    }
}

/// The settings menu of `items` on a `canvas`: the window (menu plus its heading, see
/// [`window_rect`]) centred on it.
fn placed(items: Vec<MenuItem>, canvas: Vec2) -> Menu {
    let mut menu = Menu::new(items);
    menu.framed = false;
    let h = menu.rect().h;
    menu.set_position(
        ((canvas.x - WIDTH) / 2.0).round() + 6.0,
        ((canvas.y - h) / 2.0).round() + 10.0,
    );
    menu.set_width(WIDTH - 12.0);
    menu
}

/// The window drawn around the settings menu `m`, with its heading.
fn window_rect(m: Rect) -> Rect {
    Rect::new(m.x - 6.0, m.y - 26.0, WIDTH, m.h + 32.0)
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
        let menu = placed(screen.items(&Settings::default(), true), canvas);
        let window = window_rect(menu.rect());
        assert!(window.y >= 0.0, "{window:?}");
        // The storage line takes the last 16 pixels.
        assert!(window.bottom() <= canvas.y - 16.0, "{window:?}");
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
        assert_eq!(screen.items(&s, true)[at].detail.as_deref(), Some("1 / 4"));
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
        let off = &screen.items(&s, false)[at];
        assert!(!off.enabled && !off.adjustable);
        assert_eq!(off.detail.as_deref(), Some("원작 데이터 없음"));
        let on = &screen.items(&s, true)[at];
        assert!(on.enabled && on.adjustable);
        assert_eq!(on.detail.as_deref(), Some(s.portraits.label()));
    }
}
