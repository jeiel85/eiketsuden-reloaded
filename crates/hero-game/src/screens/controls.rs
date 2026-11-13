//! Controls: every key and pointer gesture of the game in one place. Opened from the title menu
//! (조작) and from any screen with [`HELP_KEY`] (F1), so a player who never read `README.md` can
//! still find out how to play.
//!
//! Two columns: the action on the left, the keyboard keys and the mouse / touch gesture on the
//! right, separated by `/`. The list scrolls (arrows, PageUp/PageDown, wheel, drag) on canvases
//! too short for it; confirm, cancel or F1 again closes it.
//!
//! Keys are named as plain text; the constants they stand for live in
//! [`crate::quicksave`] (F5 / F6 / F9) and [`crate::screens::battle`] (Tab / Q / E), and the
//! tests of this screen keep the list honest.

use super::backdrop::draw_backdrop;
use crate::app::{Ctx, Enter, Screen, Transition};
use crate::gfx::{fill_rect, Align, FontId, TextStyle};
use crate::input::Dir;
use crate::ui::theme;
use crate::ui::window::{draw_small_arrow, draw_window};
use macroquad::prelude::*;

/// Opens this screen from anywhere (`F1`).
pub const HELP_KEY: KeyCode = KeyCode::F1;
/// Toggles the frame rate readout (`F3`).
pub const FPS_KEY: KeyCode = KeyCode::F3;
/// Toggles fullscreen (with `Alt` + `Enter`).
pub const FULLSCREEN_KEY: KeyCode = KeyCode::F11;

/// One line of the list: the action and the keys / gesture that do it.
pub struct Row {
    pub action: &'static str,
    pub input: &'static str,
}

pub struct Section {
    pub title: &'static str,
    pub rows: &'static [Row],
}

const fn row(action: &'static str, input: &'static str) -> Row {
    Row { action, input }
}

const ALWAYS: &[Row] = &[
    row("조작 안내", "F1"),
    row("순간 저장", "F5 / 대사·전투·캠프 메뉴의 '순간 저장'"),
    row(
        "순간 불러오기",
        "F9 / 대사·전투·캠프 메뉴의 '순간 불러오기'",
    ),
    row("순간 저장 칸 (1~4)", "F6 / 설정의 '순간 저장 칸'"),
    row("전체 화면", "F11 · Alt+Enter"),
    row("화면 표시 (FPS)", "F3"),
];

const COMMON: &[Row] = &[
    row("결정", "Z · Enter · Space / 클릭 · 탭"),
    row("취소 · 메뉴", "X · Esc · Backspace / 오른쪽 클릭"),
    row("목록 이동", "방향키 · WASD / 마우스 휠"),
    row("기록 삭제", "Delete (불러오기 화면)"),
];

const BATTLE: &[Row] = &[
    row("커서 이동", "방향키 · WASD / 마우스 이동"),
    row(
        "지도 이동",
        "Shift+휠 (가로) / 화면 가장자리 · 드래그 · 휠 · 오른쪽 드래그",
    ),
    row("행동 가능한 부대", "Tab · E (다음) · Q (이전)"),
    row("연출 빨리 감기", "Z · Enter · Space 누르고 있기"),
];

const DRAMA: &[Row] = &[
    row("빨리 넘기기", "Ctrl · Tab · X 길게 누르기 / 오른쪽 위 버튼"),
    row("최근 대사", "L · PageUp / 휠 위로 · 오른쪽 위 버튼"),
    row("장면 메뉴", "X 짧게 / 오른쪽 클릭"),
];

const ORIGINAL: &[Row] = &[
    row("폴더 열기", "Enter · Z / 클릭"),
    row("상위 폴더", "Backspace · .."),
    row("글자로 찾기", "폴더 이름의 첫 글자"),
    row("한 쪽씩 넘기기", "PageUp · PageDown"),
    row("경로 붙여 넣기", "Ctrl+V · Cmd+V"),
];

/// The whole list, in the order it is shown.
pub const SECTIONS: &[Section] = &[
    Section {
        title: "언제나",
        rows: ALWAYS,
    },
    Section {
        title: "공통",
        rows: COMMON,
    },
    Section {
        title: "전투",
        rows: BATTLE,
    },
    Section {
        title: "대사 · 장면",
        rows: DRAMA,
    },
    Section {
        title: "원작 데이터 (폴더 고르기)",
        rows: ORIGINAL,
    },
];

/// The whole list, in the order it is shown.
pub fn sections() -> &'static [Section] {
    SECTIONS
}

/// Width of the action column.
const ACTION_COL: f32 = 100.0;
/// Border between the columns and the window edge.
const MARGIN: f32 = 10.0;
/// Room at the right of the key column for the scroll arrows.
const GUTTER: f32 = 10.0;
/// Height of the heading above the list.
const HEADER: f32 = 24.0;
/// Height of the closing hint below the list.
const FOOTER: f32 = 18.0;
/// Height of a line of the small font.
const LINE: f32 = 12.0;
/// Height of a section title line, gap included. Two lines tall (the title sits on the lower
/// one), so every line of the list is a whole number of [`LINE`]s and scrolling by whole lines
/// can bring any line fully into view.
const SECTION: f32 = 2.0 * LINE;

/// A laid-out line of the list.
enum Line {
    Section(usize),
    /// Index into the depth-first run of rows of [`sections`], and the wrapped right column.
    Row(usize, Vec<String>),
}

pub struct ControlsScreen {
    /// Laid out on the canvas when the screen is entered.
    lines: Vec<Line>,
    /// Height of each line of `lines`.
    heights: Vec<f32>,
    total: f32,
    scroll: f32,
    /// The list's rectangle on the canvas (clipping and hit tests).
    area: Rect,
    /// Opened over a running game (F1): the screen below stays visible.
    overlay: bool,
}

impl ControlsScreen {
    /// The screen as the title menu opens it: a normal screen with the backdrop.
    pub fn new() -> ControlsScreen {
        ControlsScreen {
            lines: Vec::new(),
            heights: Vec::new(),
            total: 0.0,
            scroll: 0.0,
            area: Rect::new(0.0, 0.0, 0.0, 0.0),
            overlay: false,
        }
    }

    /// The screen as F1 opens it: an overlay, so the game below keeps its state and stays
    /// visible behind it.
    pub fn overlay() -> ControlsScreen {
        ControlsScreen {
            overlay: true,
            ..ControlsScreen::new()
        }
    }

    fn layout(&mut self, ctx: &Ctx) {
        let canvas = ctx.gfx.size();
        let width = (canvas.x - MARGIN - ACTION_COL - MARGIN - GUTTER).max(40.0);
        self.lines.clear();
        self.heights.clear();
        self.total = 0.0;
        let mut row_index = 0;
        for (s, section) in sections().iter().enumerate() {
            self.lines.push(Line::Section(s));
            self.heights.push(SECTION);
            self.total += SECTION;
            for row in section.rows {
                let wrapped = ctx.gfx.wrap(row.input, FontId::Small, 1, width);
                let h = wrapped.len().max(1) as f32 * LINE;
                self.lines.push(Line::Row(row_index, wrapped));
                self.heights.push(h);
                self.total += h;
                row_index += 1;
            }
        }
        self.scroll = 0.0;
        self.area = Rect::new(
            MARGIN,
            HEADER,
            canvas.x - 2.0 * MARGIN,
            (canvas.y - FOOTER - HEADER).max(1.0),
        );
    }

    fn max_scroll(&self) -> f32 {
        // Whole lines, matching the scroll step and the whole-line drawing below: the list shows
        // the top `view` of its area, so that its last line ends on a line boundary.
        let view = ((self.area.h / LINE).floor() * LINE).max(LINE);
        (self.total - view).max(0.0)
    }

    /// The action of the `n`th row of the list (see [`sections`]).
    fn action_of(n: usize) -> &'static str {
        sections()
            .iter()
            .flat_map(|s| s.rows)
            .nth(n)
            .map_or("", |r| r.action)
    }
}

/// `offset` moved to the nearest whole line and clamped to `max`. The list draws only lines
/// fully inside its area (text is not clipped), so a fractional offset would make rows vanish
/// instead of sliding.
fn snap_to_line(offset: f32, max: f32) -> f32 {
    ((offset / LINE).round() * LINE).clamp(0.0, max)
}

impl Default for ControlsScreen {
    fn default() -> Self {
        ControlsScreen::new()
    }
}

impl Screen for ControlsScreen {
    fn name(&self) -> &'static str {
        "controls"
    }

    fn is_overlay(&self) -> bool {
        self.overlay
    }

    fn on_enter(&mut self, ctx: &mut Ctx, _how: Enter) {
        self.layout(ctx);
    }

    fn update(&mut self, ctx: &mut Ctx) -> Transition {
        let max_scroll = self.max_scroll();
        // Whole lines, like the arrow step: the list draws only lines fully inside its area.
        let page = ((self.area.h / LINE).floor() * LINE).max(LINE);
        let mut step = 0.0;
        match ctx.input.nav() {
            Some(Dir::Up) => step -= LINE,
            Some(Dir::Down) => step += LINE,
            _ => {}
        }
        if ctx.input.key_pressed(KeyCode::PageUp) {
            step -= page;
        }
        if ctx.input.key_pressed(KeyCode::PageDown) {
            step += page;
        }
        if ctx.input.hovering(self.area) {
            step += ctx.input.wheel() as f32 * 3.0 * LINE;
        }
        if let Some(d) = ctx.input.drag() {
            if self.area.contains(d.origin) {
                step -= d.delta.y;
            }
        }
        self.scroll = snap_to_line(self.scroll + step, max_scroll);
        // F1 also closes it (the same key opens it from any screen).
        if ctx.input.confirm() || ctx.input.cancel() || ctx.input.key_pressed(HELP_KEY) {
            return Transition::Pop;
        }
        Transition::None
    }

    fn draw(&self, ctx: &Ctx) {
        let gfx = &ctx.gfx;
        let canvas = gfx.size();
        if self.overlay {
            fill_rect(gfx.screen(), Color::new(0.0, 0.0, 0.02, 0.72));
        } else {
            draw_backdrop(canvas, ctx.time);
        }
        let frame = Rect::new(
            MARGIN - 6.0,
            2.0,
            canvas.x - 2.0 * (MARGIN - 6.0),
            canvas.y - 4.0,
        );
        draw_window(frame);
        gfx.text_aligned(
            "조작",
            frame.x,
            frame.y + 5.0,
            frame.w,
            Align::Center,
            TextStyle::main(theme::TEXT_ACCENT).shadow(theme::TEXT_SHADOW),
        );

        let text_x = MARGIN + ACTION_COL;
        let section_style = TextStyle::small(theme::TEXT_ACCENT).shadow(theme::TEXT_SHADOW);
        let key_style = TextStyle::small(theme::TEXT);
        let action_style = TextStyle::small(theme::TEXT_DIM);
        let mut y = self.area.y - self.scroll;
        for (i, line) in self.lines.iter().enumerate() {
            let h = self.heights[i];
            // Only lines fully inside the list area are drawn: text is not clipped by anything
            // below this, so a half-scrolled line would spill over the heading or the hint.
            let visible = y >= self.area.y - 0.5 && y + h <= self.area.bottom() + 0.5;
            match line {
                Line::Section(s) => {
                    if visible {
                        // On the lower of its two lines: the upper one is the gap above it.
                        gfx.text(
                            &format!("[{}]", sections()[*s].title),
                            MARGIN,
                            y + LINE,
                            section_style,
                        );
                    }
                }
                Line::Row(n, wrapped) => {
                    if visible {
                        let action = Self::action_of(*n);
                        // Centre the action against its (possibly several) key lines.
                        gfx.text(action, MARGIN, y + (h - LINE) / 2.0, action_style);
                        gfx.text_lines(wrapped, text_x, y, key_style);
                    }
                }
            }
            y += h;
        }
        // Arrows drawn as shapes: the small font may have no triangle glyphs.
        let arrow_x = frame.right() - 8.0;
        if self.scroll > 0.0 {
            draw_small_arrow(arrow_x, self.area.y - 5.0, false, theme::TEXT_ACCENT);
        }
        if self.scroll < self.max_scroll() {
            draw_small_arrow(arrow_x, self.area.bottom() + 4.0, true, theme::TEXT_ACCENT);
        }
        gfx.text_aligned(
            "Z · X · F1 · 클릭 — 닫기   방향키 · 휠 — 넘기기",
            frame.x,
            canvas.y - FOOTER + 2.0,
            frame.w,
            Align::Center,
            TextStyle::small(theme::TEXT_DIM).shadow(theme::TEXT_SHADOW),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every section has rows, every row names both an action and keys, and the keys the game
    /// binds are in the list (F1, F5, F6, F9, F3, F11, Tab / Q / E, PageUp, Delete).
    #[test]
    fn the_list_covers_every_binding() {
        let sections = sections();
        assert!(!sections.is_empty());
        let all: Vec<&Row> = sections.iter().flat_map(|s| s.rows.iter()).collect();
        assert!(all
            .iter()
            .all(|r| !r.action.is_empty() && !r.input.is_empty()));
        let text: String = all.iter().map(|r| r.input).collect::<Vec<_>>().join(" ");
        for key in [
            "F1",
            "F3",
            "F5",
            "F6",
            "F9",
            "F11",
            "Alt+Enter",
            "Tab",
            "Q",
            "E",
            "PageUp",
            "Delete",
            "Esc",
            "Backspace",
            "WASD",
            "Ctrl",
        ] {
            assert!(text.contains(key), "{key} missing from the list: {text}");
        }
        // The constants and the list agree.
        assert_eq!(HELP_KEY, KeyCode::F1);
        assert_eq!(FPS_KEY, KeyCode::F3);
        assert_eq!(FULLSCREEN_KEY, KeyCode::F11);
        assert_eq!(crate::quicksave::SAVE_KEY, KeyCode::F5);
        assert_eq!(crate::quicksave::SLOT_KEY, KeyCode::F6);
        assert_eq!(crate::quicksave::LOAD_KEY, KeyCode::F9);
    }

    /// The list is longer than the space under its heading on the default canvas, so it
    /// scrolls; and `action_of` walks the rows in the same order `layout` numbers them.
    #[test]
    fn the_list_is_numbered_in_the_drawn_order() {
        let rows: Vec<&Row> = sections().iter().flat_map(|s| s.rows.iter()).collect();
        for (n, row) in rows.iter().enumerate() {
            assert_eq!(ControlsScreen::action_of(n), row.action);
        }
        assert_eq!(ControlsScreen::action_of(rows.len()), "");
        let area = crate::gfx::DEFAULT_CANVAS.y - FOOTER - HEADER;
        let sections_h = sections().len() as f32 * SECTION;
        assert!(
            rows.len() as f32 * LINE + sections_h > area,
            "the list fits without scrolling; the scroll code is untested"
        );
    }

    /// The scroll limit lands on a line boundary that leaves the view filled: the last line of
    /// the list ends exactly on the bottom of the area, so nothing is left permanently hidden.
    #[test]
    fn scrolling_moves_in_whole_lines() {
        let area = Rect::new(0.0, 24.0, 480.0, 200.0);
        let mut screen = ControlsScreen {
            lines: Vec::new(),
            heights: Vec::new(),
            total: 0.0,
            scroll: 0.0,
            area,
            overlay: false,
        };
        // 200 is not a whole number of 12-pixel lines, and rows wrap to different heights.
        screen.total = 272.0;
        let view = (area.h / LINE).floor() * LINE;
        let max = screen.max_scroll();
        assert_eq!(view, 192.0);
        assert_eq!(max, 80.0);
        assert_eq!(screen.total - max, view, "the last line stays visible");
        assert_eq!(snap_to_line(13.0, max), 12.0);
        assert_eq!(snap_to_line(-5.0, max), 0.0);
        assert_eq!(snap_to_line(1e6, max), max);
    }
}
