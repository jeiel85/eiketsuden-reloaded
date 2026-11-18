//! Drawing helpers and small widgets shared by the camp screens: backdrop, header with gold, help
//! bar, officer rows with unit sprites, stat comparisons, item descriptions, a two-button row and
//! the quantity dialog of the shop.

use super::stats::OfficerStats;
use crate::app::Ctx;
use crate::audio::sfx;
use crate::gfx::{fill_gradient_v, fill_rect, Align, FontId, Gfx, TextStyle};
use crate::input::Dir;
use crate::ui::art::{draw_background, draw_unit};
use crate::ui::dialog::ConfirmEvent;
use crate::ui::format;
use crate::ui::menu::Menu;
use crate::ui::theme;
use crate::ui::window::{
    draw_highlight, draw_icon, draw_side_arrow, draw_title_bar, draw_window, draw_window_ex, inset,
    WindowStyle,
};
use hero_core::campaign::OfficerState;
use hero_core::data::{Effect, ItemDef, ItemKind};
use hero_core::pack::Pack;
use macroquad::prelude::*;

/// Top of the content below the header.
pub const TOP: f32 = 26.0;
/// Offset of the first row below the caption strip of a list window.
pub const LIST_TOP: f32 = 18.0;
/// Horizontal gap between the list column and the panel of a two-column screen.
pub const COLUMN_GAP: f32 = 6.0;

/// Top of the help bar at the bottom of a canvas `canvas_h` pixels high.
pub fn help_y(canvas_h: f32) -> f32 {
    canvas_h - 17.0
}

/// Where the windows of a camp sub-screen go on a `canvas` sized canvas: below the header,
/// above the help bar, 8 pixels in from the sides.
pub fn content_rect(canvas: Vec2) -> Rect {
    let top = TOP + 2.0;
    Rect::new(8.0, top, canvas.x - 16.0, help_y(canvas.y) - 3.0 - top)
}

/// [`content_rect`] split into a list column `left_w` pixels wide and a panel right of it that
/// takes the rest of the width.
pub fn columns(canvas: Vec2, left_w: f32) -> (Rect, Rect) {
    let c = content_rect(canvas);
    let right_x = c.x + left_w + COLUMN_GAP;
    (
        Rect::new(c.x, c.y, left_w, c.h),
        Rect::new(right_x, c.y, c.right() - right_x, c.h),
    )
}

/// Background of every camp screen: the `camp` drama background (or its painted stand-in),
/// darkened by `dim` (0..1) so windows stay readable.
pub fn draw_camp_backdrop(ctx: &Ctx, dim: f32) {
    fill_rect(ctx.gfx.screen(), theme::BACKGROUND);
    draw_background(ctx, "camp", 1.0);
    fill_rect(
        ctx.gfx.screen(),
        Color::new(0.01, 0.015, 0.05, dim.clamp(0.0, 1.0)),
    );
}

/// Title strip with `title` on the left and the army's gold on the right.
pub fn draw_header(ctx: &Ctx, title: &str, gold: i64) {
    draw_title_bar(ctx, title);
    let gfx = &ctx.gfx;
    let amount = format::thousands(gold);
    let style = TextStyle::main(theme::TEXT).shadow(theme::TEXT_SHADOW);
    let w = gfx.text_width(&amount, FontId::Main, 1);
    let x = gfx.size().x - 10.0 - w;
    gfx.text(&amount, x, 2.0, style);
    draw_icon(ctx, "gold", vec2(x - 19.0, 2.0));
    gfx.text_aligned(
        "군자금",
        0.0,
        4.0,
        x - 23.0,
        Align::Right,
        TextStyle::small(theme::TEXT_DIM).shadow(theme::TEXT_SHADOW),
    );
}

/// Help line at the bottom of the screen.
pub fn draw_help(ctx: &Ctx, text: &str) {
    draw_help_colored(ctx, text, theme::TEXT_DIM);
}

/// Help line in a given colour; text wider than the screen is cut with `…`.
pub fn draw_help_colored(ctx: &Ctx, text: &str, color: Color) {
    let gfx = &ctx.gfx;
    let canvas = gfx.size();
    let y = help_y(canvas.y);
    fill_gradient_v(
        Rect::new(0.0, y - 3.0, canvas.x, canvas.y - y + 3.0),
        Color::new(0.0, 0.0, 0.05, 0.0),
        Color::new(0.0, 0.0, 0.05, 0.75),
    );
    let fitted = gfx.elide(text, FontId::Small, 1, back_button(canvas).x - 16.0);
    gfx.text(
        &fitted,
        10.0,
        y + 1.0,
        TextStyle::small(color).shadow(theme::TEXT_SHADOW),
    );
}

/// The 돌아가기 button at the right end of the help bar of a `canvas` sized canvas: mouse and
/// touch have no cancel key, so every camp sub-screen offers this button for it.
pub fn back_button(canvas: Vec2) -> Rect {
    Rect::new(canvas.x - 70.0, help_y(canvas.y) - 2.0, 62.0, 16.0)
}

/// Whether the 돌아가기 button was tapped this frame (the tap is consumed and the cancel sound
/// played).
pub fn back_tapped(ctx: &mut Ctx) -> bool {
    if ctx.input.tapped(back_button(ctx.gfx.size())) {
        ctx.input.consume();
        ctx.sfx(sfx::CANCEL);
        true
    } else {
        false
    }
}

/// Draw the 돌아가기 button.
pub fn draw_back_button(ctx: &Ctx) {
    let r = back_button(ctx.gfx.size());
    let hover = ctx.input.hovering(r);
    draw_window_ex(r, WindowStyle::Panel, if hover { 1.0 } else { 0.85 });
    if hover {
        draw_highlight(inset(r, 2.0), true, ctx.time);
    }
    let color = if hover { theme::TEXT } else { theme::TEXT_DIM };
    draw_side_arrow(r.x + 9.0, r.center().y, false, color);
    ctx.gfx.text_aligned(
        "돌아가기",
        r.x + 12.0,
        r.y + 2.0,
        r.w - 14.0,
        Align::Center,
        TextStyle::small(color),
    );
}

/// Class display name.
pub fn class_name<'a>(pack: &'a Pack, class: &'a str) -> &'a str {
    pack.class(class).map_or(class, |c| c.name.as_str())
}

/// Officer display name.
pub fn officer_name<'a>(pack: &'a Pack, id: &'a str) -> &'a str {
    pack.officer(id).map_or(id, |o| o.name.as_str())
}

/// Portrait key of an officer.
pub fn portrait_key<'a>(pack: &'a Pack, id: &'a str) -> &'a str {
    pack.officer(id).map_or(id, |o| o.portrait_key())
}

/// Sprite key of a class (`units/<sprite>_<side>`).
pub fn class_sprite<'a>(pack: &'a Pack, class: &'a str) -> &'a str {
    pack.class(class).map_or(class, |c| c.sprite.as_str())
}

/// Draw an officer's unit sprite with its feet at `feet`; `walking` animates the walk cycle.
pub fn draw_officer_sprite(
    ctx: &Ctx,
    pack: &Pack,
    officer: &OfficerState,
    feet: Vec2,
    walking: bool,
) {
    let step = if walking {
        ((ctx.time * 5.0) as u32) % 4
    } else {
        0
    };
    draw_unit(
        ctx,
        class_sprite(pack, &officer.class),
        "player",
        feet,
        step,
    );
}

/// Name of an item slot.
pub fn slot_name(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::Weapon => "무기",
        ItemKind::Armor => "병법서",
        ItemKind::Accessory => "보물",
        ItemKind::Consumable => "도구",
    }
}

/// Engine icon for an item slot.
pub fn slot_icon(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::Weapon => "weapon",
        ItemKind::Armor => "armor",
        ItemKind::Accessory => "accessory",
        ItemKind::Consumable => "consumable",
    }
}

/// Icon key of an item (its own, or the icon of its slot).
pub fn item_icon(item: &ItemDef) -> &str {
    if item.icon.is_empty() {
        slot_icon(item.kind)
    } else {
        &item.icon
    }
}

/// Short summary of what an item does, e.g. `공격 +12%`, `병력 400 회복`, `이동 +3`.
pub fn item_effect(pack: &Pack, item: &ItemDef) -> String {
    let mut parts: Vec<String> = Vec::new();
    if item.atk_pct > 0 {
        parts.push(format!("공격 {:+}%", item.atk_pct - 100));
    }
    if item.def_pct > 0 {
        parts.push(format!("방어 {:+}%", item.def_pct - 100));
    }
    if item.move_bonus != 0 {
        parts.push(format!("이동 {:+}", item.move_bonus));
    }
    if item.regen_hp > 0 {
        parts.push(format!("매 턴 병력 {}% 회복", item.regen_hp));
    }
    if item.regen_morale > 0 {
        parts.push(format!("매 턴 사기 {} 회복", item.regen_morale));
    }
    if let Some(s) = &item.strategy {
        let name = pack.strategy(s).map_or(s.as_str(), |d| d.name.as_str());
        parts.push(format!("책략 「{name}」"));
    }
    for e in &item.effects {
        match e {
            Effect::Heal { power } => parts.push(format!("병력 {power} 회복")),
            Effect::Morale { amount } if *amount >= 0 => parts.push(format!("사기 {amount} 회복")),
            Effect::Morale { amount } => parts.push(format!("사기 {} 감소", -amount)),
            Effect::Promote => parts.push("병과 향상".to_string()),
            Effect::ChangeClass { to } => {
                parts.push(format!("병과 → {}", class_name(pack, to)));
            }
            Effect::Damage { power } => parts.push(format!("피해 {power}")),
            Effect::Status { .. } => parts.push("혼란".to_string()),
        }
    }
    parts.join(" · ")
}

/// Colour for a stat change.
pub fn delta_color(before: i32, after: i32) -> Color {
    match after.cmp(&before) {
        std::cmp::Ordering::Greater => theme::TEXT_GOOD,
        std::cmp::Ordering::Less => theme::TEXT_BAD,
        std::cmp::Ordering::Equal => theme::TEXT,
    }
}

/// One stat row: `label` at `x`, the value right-aligned in `[x, x + w]`; with `after`, the
/// row reads `before → after` with the new value coloured by the change.
pub fn draw_stat(gfx: &Gfx, label: &str, before: i32, after: Option<i32>, x: f32, y: f32, w: f32) {
    let dim = TextStyle::main(theme::TEXT_DIM).shadow(theme::TEXT_SHADOW);
    let norm = TextStyle::main(theme::TEXT).shadow(theme::TEXT_SHADOW);
    gfx.text(label, x, y, dim);
    match after {
        Some(a) if a != before => {
            let new = format::thousands(i64::from(a));
            let nw = gfx.text_width(&new, FontId::Main, 1);
            gfx.text(
                &new,
                x + w - nw,
                y,
                TextStyle::main(delta_color(before, a)).shadow(theme::TEXT_SHADOW),
            );
            let arrow_x = x + w - nw - 14.0;
            gfx.text("→", arrow_x, y, dim);
            let old = format::thousands(i64::from(before));
            let ow = gfx.text_width(&old, FontId::Main, 1);
            gfx.text(&old, arrow_x - 4.0 - ow, y, norm);
        }
        _ => {
            gfx.text_aligned(
                &format::thousands(i64::from(before)),
                x,
                y,
                w,
                Align::Right,
                norm,
            );
        }
    }
}

/// The battle values block: 병력 / 책략치 / 공격 / 방어 / 이동, optionally compared with `after`.
/// Returns the height used.
pub fn draw_stats_block(
    gfx: &Gfx,
    stats: &OfficerStats,
    after: Option<&OfficerStats>,
    x: f32,
    y: f32,
    w: f32,
) -> f32 {
    let rows = [
        ("병력", stats.hp, after.map(|a| a.hp)),
        ("책략치", stats.mp, after.map(|a| a.mp)),
        ("공격력", stats.atk, after.map(|a| a.atk)),
        ("방어력", stats.def, after.map(|a| a.def)),
        ("이동력", stats.mov, after.map(|a| a.mov)),
    ];
    for (i, (label, before, after)) in rows.into_iter().enumerate() {
        draw_stat(gfx, label, before, after, x, y + i as f32 * 15.0, w);
    }
    rows.len() as f32 * 15.0
}

/// Visible rows of a list menu: `(index, row rectangle)`.
pub fn visible_rows(menu: &Menu) -> impl Iterator<Item = (usize, Rect)> + '_ {
    let r = menu.rect();
    (0..menu.items.len())
        .map(move |i| (i, menu.row_rect(i)))
        .filter(move |(_, row)| row.y >= r.y - 0.5 && row.bottom() <= r.bottom() + 0.5)
}

/// A small caption at the top of a panel.
pub fn draw_caption(gfx: &Gfx, text: &str, x: f32, y: f32) {
    gfx.text(
        text,
        x,
        y,
        TextStyle::small(theme::TEXT_ACCENT).shadow(theme::TEXT_SHADOW),
    );
}

/// Draw a list window with a caption strip above its rows (for lists built on [`Menu`] with
/// `framed = false`): the window is `rect`, the caption sits in its first 14 pixels.
pub fn draw_list_frame(ctx: &Ctx, rect: Rect, caption: &str, active: bool) {
    draw_window_ex(
        rect,
        if active {
            WindowStyle::Normal
        } else {
            WindowStyle::Panel
        },
        1.0,
    );
    draw_caption(&ctx.gfx, caption, rect.x + 8.0, rect.y + 4.0);
}

// ----- two buttons ---------------------------------------------------------------------------

/// A row of two buttons (yes / no) inside a dialog.
#[derive(Debug, Clone)]
pub struct TwoButtons {
    pub yes: String,
    pub no: String,
    pub yes_selected: bool,
}

pub const BUTTON_W: f32 = 64.0;
pub const BUTTON_H: f32 = 18.0;

impl TwoButtons {
    pub fn new(yes: &str, no: &str) -> TwoButtons {
        TwoButtons {
            yes: yes.to_string(),
            no: no.to_string(),
            yes_selected: true,
        }
    }

    /// Button rectangles centred on `cx` with their top at `y`.
    pub fn rects(cx: f32, y: f32) -> (Rect, Rect) {
        (
            Rect::new((cx - BUTTON_W - 6.0).round(), y, BUTTON_W, BUTTON_H),
            Rect::new((cx + 6.0).round(), y, BUTTON_W, BUTTON_H),
        )
    }

    /// Handle input. `nav_toggles` lets up/down/left/right switch buttons (dialogs that use
    /// left/right for something else pass `false`; up/down still switch).
    pub fn update(&mut self, ctx: &mut Ctx, cx: f32, y: f32, nav_toggles: bool) -> ConfirmEvent {
        let (yes_r, no_r) = Self::rects(cx, y);
        let input = &ctx.input;
        if let Some(p) = input.tap() {
            if yes_r.contains(p) {
                ctx.sfx(sfx::CONFIRM);
                return ConfirmEvent::Yes;
            }
            if no_r.contains(p) {
                ctx.sfx(sfx::CANCEL);
                return ConfirmEvent::No;
            }
        }
        if input.pointer_moved() {
            if let Some(p) = input.pointer() {
                if yes_r.contains(p) {
                    self.yes_selected = true;
                } else if no_r.contains(p) {
                    self.yes_selected = false;
                }
            }
        }
        match input.nav() {
            Some(Dir::Up | Dir::Down) => {
                self.yes_selected = !self.yes_selected;
                ctx.sfx(sfx::CURSOR);
                return ConfirmEvent::None;
            }
            Some(Dir::Left | Dir::Right) if nav_toggles => {
                self.yes_selected = !self.yes_selected;
                ctx.sfx(sfx::CURSOR);
                return ConfirmEvent::None;
            }
            _ => {}
        }
        if input.confirm_key() {
            return if self.yes_selected {
                ctx.sfx(sfx::CONFIRM);
                ConfirmEvent::Yes
            } else {
                ctx.sfx(sfx::CANCEL);
                ConfirmEvent::No
            };
        }
        if input.cancel() {
            ctx.sfx(sfx::CANCEL);
            return ConfirmEvent::No;
        }
        ConfirmEvent::None
    }

    pub fn draw(&self, ctx: &Ctx, cx: f32, y: f32) {
        let (yes_r, no_r) = Self::rects(cx, y);
        for (r, label, selected) in [
            (yes_r, &self.yes, self.yes_selected),
            (no_r, &self.no, !self.yes_selected),
        ] {
            draw_window_ex(r, WindowStyle::Panel, 1.0);
            if selected {
                draw_highlight(inset(r, 3.0), true, ctx.time);
            }
            let color = if selected {
                theme::TEXT
            } else {
                theme::TEXT_DIM
            };
            ctx.gfx.text_aligned(
                label,
                r.x,
                r.y + 1.0,
                r.w,
                Align::Center,
                TextStyle::main(color).shadow(theme::TEXT_SHADOW),
            );
        }
    }
}

// ----- quantity dialog -----------------------------------------------------------------------

/// Quantity bounds: `1..=max` (with `max >= 1`), stepping by `delta` and clamping.
pub fn step_quantity(qty: u32, delta: i32, max: u32) -> u32 {
    let max = max.max(1);
    (i64::from(qty) + i64::from(delta)).clamp(1, i64::from(max)) as u32
}

/// "How many?" dialog of the shop: ◀ quantity ▶ (left/right ±1, PageUp/PageDown ±10), the total
/// price, and two buttons.
#[derive(Debug, Clone)]
pub struct QuantityDialog {
    pub title: String,
    /// Extra line under the title (e.g. what the item does), may be empty.
    pub note: String,
    pub unit_price: i64,
    pub max: u32,
    pub qty: u32,
    /// Label of the total, e.g. `합계` / `매각액`.
    pub total_label: String,
    buttons: TwoButtons,
    rect: Rect,
}

impl QuantityDialog {
    /// A dialog centred on the canvas of `gfx`.
    pub fn new(
        gfx: &Gfx,
        title: &str,
        note: &str,
        unit_price: i64,
        max: u32,
        total_label: &str,
        yes: &str,
    ) -> QuantityDialog {
        let w = 260.0;
        let h = 110.0;
        let canvas = gfx.size();
        QuantityDialog {
            title: title.to_string(),
            note: note.to_string(),
            unit_price,
            max: max.max(1),
            qty: 1,
            total_label: total_label.to_string(),
            buttons: TwoButtons::new(yes, "취소"),
            rect: Rect::new(
                ((canvas.x - w) / 2.0).round(),
                ((canvas.y - h) / 2.0).round(),
                w,
                h,
            ),
        }
    }

    pub fn total(&self) -> i64 {
        self.unit_price * i64::from(self.qty)
    }

    fn arrows(&self) -> (Rect, Rect) {
        let y = self.rect.y + 44.0;
        let cx = self.rect.x + self.rect.w / 2.0;
        (
            Rect::new(cx - 44.0, y, 16.0, 16.0),
            Rect::new(cx + 28.0, y, 16.0, 16.0),
        )
    }

    fn buttons_y(&self) -> f32 {
        self.rect.bottom() - theme::PADDING - BUTTON_H - 2.0
    }

    pub fn update(&mut self, ctx: &mut Ctx) -> ConfirmEvent {
        let (l, r) = self.arrows();
        let mut delta = 0;
        if let Some(p) = ctx.input.tap() {
            if l.contains(p) {
                delta = -1;
            } else if r.contains(p) {
                delta = 1;
            }
        }
        match ctx.input.nav() {
            Some(Dir::Left) => delta = -1,
            Some(Dir::Right) => delta = 1,
            _ => {}
        }
        if ctx.input.key_pressed(KeyCode::PageUp) {
            delta = 10;
        }
        if ctx.input.key_pressed(KeyCode::PageDown) {
            delta = -10;
        }
        if delta != 0 {
            let q = step_quantity(self.qty, delta, self.max);
            if q != self.qty {
                self.qty = q;
                ctx.sfx(sfx::CURSOR);
            } else {
                ctx.sfx(sfx::ERROR);
            }
            ctx.input.consume();
            return ConfirmEvent::None;
        }
        let cx = self.rect.x + self.rect.w / 2.0;
        let y = self.buttons_y();
        self.buttons.update(ctx, cx, y, false)
    }

    pub fn draw(&self, ctx: &Ctx) {
        let gfx = &ctx.gfx;
        fill_rect(gfx.screen(), Color::new(0.0, 0.0, 0.0, 0.4));
        draw_window(self.rect);
        let r = self.rect;
        gfx.text_aligned(
            &self.title,
            r.x,
            r.y + 7.0,
            r.w,
            Align::Center,
            TextStyle::main(theme::TEXT_ACCENT).shadow(theme::TEXT_SHADOW),
        );
        if !self.note.is_empty() {
            gfx.text_aligned(
                &self.note,
                r.x,
                r.y + 25.0,
                r.w,
                Align::Center,
                TextStyle::small(theme::TEXT_DIM),
            );
        }
        let (la, ra) = self.arrows();
        let can_less = self.qty > 1;
        let can_more = self.qty < self.max;
        let arrow = |on: bool| {
            if on {
                theme::CURSOR_ARROW
            } else {
                theme::TEXT_DISABLED
            }
        };
        draw_side_arrow(la.center().x, la.center().y, false, arrow(can_less));
        draw_side_arrow(ra.center().x, ra.center().y, true, arrow(can_more));
        gfx.text_aligned(
            &format!("{} 개", self.qty),
            la.right(),
            la.y,
            ra.x - la.right(),
            Align::Center,
            TextStyle::main(theme::TEXT).shadow(theme::TEXT_SHADOW),
        );
        gfx.text_aligned(
            &format!(
                "{} {}  (최대 {}개)",
                self.total_label,
                format::thousands(self.total()),
                self.max
            ),
            r.x,
            r.y + 62.0,
            r.w,
            Align::Center,
            TextStyle::small(theme::TEXT_NAME),
        );
        let cx = r.x + r.w / 2.0;
        self.buttons.draw(ctx, cx, self.buttons_y());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screens::camp::test_pack;

    #[test]
    fn content_and_columns_follow_the_canvas() {
        let default = crate::gfx::DEFAULT_CANVAS;
        // The base pack's layout.
        assert_eq!(content_rect(default), Rect::new(8.0, 28.0, 464.0, 222.0));
        assert_eq!(help_y(default.y), 253.0);
        assert_eq!(back_button(default), Rect::new(410.0, 251.0, 62.0, 16.0));
        let (list, panel) = columns(default, 262.0);
        assert_eq!(list, Rect::new(8.0, 28.0, 262.0, 222.0));
        assert_eq!(panel, Rect::new(276.0, 28.0, 196.0, 222.0));
        // A bigger canvas gives the panel the extra width and both the extra height.
        let vga = vec2(640.0, 480.0);
        let (list, panel) = columns(vga, 262.0);
        assert_eq!((list.w, list.h), (262.0, 432.0));
        assert_eq!(panel.right(), 632.0);
        assert!(panel.bottom() < help_y(vga.y) && back_button(vga).right() < vga.x);
    }

    #[test]
    fn quantities_stay_in_range() {
        assert_eq!(step_quantity(1, -1, 5), 1);
        assert_eq!(step_quantity(1, 1, 5), 2);
        assert_eq!(step_quantity(4, 10, 5), 5);
        assert_eq!(step_quantity(3, 0, 0), 1);
        assert_eq!(step_quantity(u32::MAX, 1, u32::MAX), u32::MAX);
    }

    #[test]
    fn item_effect_summaries() {
        let pack = test_pack();
        let text = |id: &str| item_effect(&pack, pack.item(id).unwrap());
        assert_eq!(text("green_dragon_blade"), "공격 +12%");
        assert_eq!(text("sunzi"), "방어 +22%");
        assert_eq!(text("red_hare"), "이동 +3");
        assert_eq!(text("bean"), "병력 400 회복");
        assert_eq!(text("salve"), "병력 400 회복 · 사기 20 회복");
        assert_eq!(
            text("imperial_seal"),
            "매 턴 병력 10% 회복 · 매 턴 사기 10 회복"
        );
        assert_eq!(text("long_spear"), "병과 향상");
        assert_eq!(text("archery_guide"), "병과 → 궁병");
        assert!(text("scroll_scorch").starts_with("책략 「"));
        assert_eq!(delta_color(5, 7), theme::TEXT_GOOD);
        assert_eq!(delta_color(5, 3), theme::TEXT_BAD);
        assert_eq!(delta_color(5, 5), theme::TEXT);
    }
}
