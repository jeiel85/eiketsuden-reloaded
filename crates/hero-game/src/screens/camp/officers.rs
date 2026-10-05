//! 무장 정보: a table of the army (class, level and battle values) and a detail page per officer
//! with portrait, names, class, level and EXP, battle values, 무력/지력/통솔, known strategies,
//! equipment and biography. Tapping the lord's portrait many times leads to the original's hidden
//! command ([`crate::secret`]).
//!
//! A pack with a status window (`[presentation.status_frame]`, the original's) shows the army on
//! it instead of the table: a page of officers in its slots (unit icon, level, troops) and the
//! chosen one on the side; the detail page opens from there.
//!
//! A campaign started with 능력치 자유 조정 (DECISIONS D25) edits 무력/지력/통솔 on the detail page:
//! confirm (or a tap on the abilities) starts editing, up and down pick one, left and right (or
//! pressing its left or right half) change it by 1, held by 5 after a second and by 10 after
//! two ([`edit_step`]), confirm or cancel ends.

use super::stats::officer_stats;
use super::widgets::{back_button, back_tapped, content_rect, draw_back_button, help_y};
use super::widgets::{
    class_name, draw_camp_backdrop, draw_caption, draw_header, draw_help, draw_list_frame,
    draw_officer_sprite, draw_stats_block, item_icon, officer_name, portrait_key, slot_icon,
    slot_name, visible_rows,
};
use crate::app::{Ctx, Enter, Screen, Transition};
use crate::audio::sfx;
use crate::gfx::{fill_rect, Align, FontId, TextStyle};
use crate::input::{Dir, KeyRepeat};
use crate::secret::SecretStep;
use crate::ui::art::draw_portrait_card;
use crate::ui::bars::{draw_gauge, draw_gauge_labeled, GaugeKind};
use crate::ui::dialog::{ConfirmDialog, ConfirmEvent};
use crate::ui::format;
use crate::ui::menu::{Menu, MenuEvent, MenuItem};
use crate::ui::theme;
use crate::ui::window::{
    draw_divider, draw_highlight, draw_icon, draw_side_arrow, draw_window_ex, inset, WindowStyle,
};
use hero_core::campaign::{Ability, OfficerState};
use hero_core::data::ItemKind;
use hero_core::pack::{Pack, StatusFrame};
use macroquad::prelude::*;

const ROW_H: f32 = 22.0;
/// Column x offsets from the row's left edge, after the sprite and name.
const COLUMNS: [(&str, f32); 8] = [
    ("병과", 104.0),
    ("Lv", 160.0),
    ("병력", 200.0),
    ("책략치", 250.0),
    ("공격력", 300.0),
    ("방어력", 350.0),
    ("이동력", 400.0),
    ("", 440.0),
];

/// Names of the strategies an officer knows, with their MP cost.
pub fn strategy_list(pack: &Pack, officer: &OfficerState) -> Vec<(String, i32)> {
    pack.known_strategies(&officer.class, officer.level)
        .iter()
        .map(|id| match pack.strategy(id) {
            Some(s) => (s.name.clone(), s.mp),
            None => (id.clone(), 0),
        })
        .collect()
}

/// Row `i` (무력, 지력, 통솔) of the abilities on the detail page.
fn ability_rect(canvas: Vec2, i: usize) -> Rect {
    let panel = content_rect(canvas);
    let cx = panel.x + 8.0 + 108.0;
    let by = panel.y + 8.0 + 120.0;
    // Wide enough for the ▶ drawn just right of the value.
    Rect::new(cx - 2.0, by - 2.0 + i as f32 * 14.0, 162.0, 14.0)
}

/// The ability row under `p` and the half of it: left lowers the value, right raises it.
fn ability_side(canvas: Vec2, p: Vec2) -> Option<(usize, Dir)> {
    (0..3)
        .find(|&k| ability_rect(canvas, k).contains(p))
        .map(|k| {
            let side = if p.x < ability_rect(canvas, k).center().x {
                Dir::Left
            } else {
                Dir::Right
            };
            (k, side)
        })
}

/// How much one step changes an ability that has been stepped the same way for `held` seconds.
///
/// Input: seconds the key or press has been held. Output: 1, then 5 after 1 s, then 10 after
/// 2 s.
///
/// Why grow the step instead of only repeating: the key repeat alone takes about 8 seconds from
/// 1 to 100. Growing it keeps a short press exact and takes a held one there in about 2.
fn edit_step(held: f32) -> i32 {
    if held < 1.0 {
        1
    } else if held < 2.0 {
        5
    } else {
        10
    }
}

/// Where the detail page draws the officer's portrait.
fn portrait_rect(canvas: Vec2) -> Rect {
    let panel = content_rect(canvas);
    Rect::new(panel.x + 8.0, panel.y + 8.0, 96.0, 120.0)
}

/// The orb of the hidden command, in the top left corner as in the original (left of the title,
/// which starts at x 10).
fn orb_rect() -> Rect {
    Rect::new(0.0, 1.0, 10.0, 10.0)
}

/// The prompt of the hidden command (our wording; the original's warning is not reproduced).
const SECRET_PROMPT: &str =
    "금단의 비법\n이 명령은 게임의 균형을 무너뜨립니다. 그래도 쓰시겠습니까?";

/// The status window's picture placed in the camp screens' `area`: centred, whole pixels.
struct StatusLayout<'a> {
    frame: &'a StatusFrame,
    origin: Vec2,
}

impl<'a> StatusLayout<'a> {
    fn new(frame: &'a StatusFrame, area: Vec2) -> StatusLayout<'a> {
        let [w, h] = frame.size;
        StatusLayout {
            frame,
            origin: vec2(
                ((area.x - w as f32) / 2.0).floor(),
                ((area.y - h as f32) / 2.0).floor(),
            ),
        }
    }

    /// An area of the picture on screen.
    fn rect(&self, [x, y, w, h]: [u32; 4]) -> Rect {
        Rect::new(
            self.origin.x + x as f32,
            self.origin.y + y as f32,
            w as f32,
            h as f32,
        )
    }

    fn per_page(&self) -> usize {
        self.frame.slots.len()
    }

    /// Slots in a row (the ones level with the first).
    fn columns(&self) -> usize {
        let top = self.frame.slots[0].icon[1];
        self.frame
            .slots
            .iter()
            .take_while(|s| s.icon[1] == top)
            .count()
            .max(1)
    }

    /// The slot tapped at `p`: its icon, level or troops.
    fn slot_at(&self, p: Vec2) -> Option<usize> {
        self.frame.slots.iter().position(|s| {
            [s.icon, s.level, s.troops]
                .into_iter()
                .any(|a| self.rect(a).contains(p))
        })
    }
}

/// Where a tap counts towards the hidden command now ([`crate::secret`]): the lord's portrait,
/// on their detail page, or on the status window while the lord is the chosen officer.
///
/// Input: the canvas, the pack's status window, the detail page open (`detail`), the chosen
/// officer (`cursor`) and whether an officer of the army is the lord. Output: the portrait's
/// rectangle, or `None` when no lord's portrait is on screen.
///
/// Why the status window too: the original counts taps on the lord's portrait of its status
/// window, and the original mode shows that window, so that is where its players tap; before,
/// only the detail page's portrait counted and those taps did nothing.
fn secret_portrait(
    canvas: Vec2,
    status: Option<&StatusFrame>,
    detail: Option<usize>,
    cursor: usize,
    is_lord: impl Fn(usize) -> bool,
) -> Option<Rect> {
    match (detail, status) {
        (Some(i), _) => is_lord(i).then(|| portrait_rect(canvas)),
        (None, Some(frame)) => {
            is_lord(cursor).then(|| StatusLayout::new(frame, canvas).rect(frame.portrait))
        }
        (None, None) => None,
    }
}

/// What a tap or key on the status window does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StatusAction {
    /// Move the cursor to this officer.
    Choose(usize),
    /// Open the chosen officer's detail page.
    Open,
    Close,
    None,
}

/// The status window's answer to a tap at `p` with `cursor` on officer `cursor` of `count`.
fn status_tap(layout: &StatusLayout, p: Vec2, cursor: usize, count: usize) -> StatusAction {
    let per = layout.per_page();
    let page = cursor / per;
    if layout.rect(layout.frame.close).contains(p) {
        return StatusAction::Close;
    }
    let pager = layout.rect(layout.frame.pager);
    if pager.contains(p) {
        let pages = count.div_ceil(per).max(1);
        let to = if p.y < pager.center().y {
            (page + pages - 1) % pages
        } else {
            (page + 1) % pages
        };
        return StatusAction::Choose(to * per);
    }
    match layout.slot_at(p).map(|k| page * per + k) {
        Some(i) if i == cursor && i < count => StatusAction::Open,
        Some(i) if i < count => StatusAction::Choose(i),
        _ => StatusAction::None,
    }
}

/// The officer table and detail pages.
pub struct OfficersScreen {
    menu: Menu,
    /// Index of the officer whose detail page is open.
    detail: Option<usize>,
    /// The prompt of the hidden command, while it is open.
    prompt: Option<ConfirmDialog>,
    /// 능력치 자유 조정: the ability row being edited on the detail page.
    edit: Option<usize>,
    /// The way the ability is being stepped (key or press held) and for how long, in seconds:
    /// the step grows with it ([`edit_step`]).
    edit_hold: (Option<Dir>, f32),
    /// Repeat of a press held on an ability row: touch has no key repeat.
    press_repeat: KeyRepeat,
}

impl Default for OfficersScreen {
    fn default() -> Self {
        OfficersScreen::new()
    }
}

impl OfficersScreen {
    pub fn new() -> OfficersScreen {
        OfficersScreen {
            menu: Menu::new(Vec::new()),
            detail: None,
            prompt: None,
            edit: None,
            edit_hold: (None, 0.0),
            press_repeat: KeyRepeat::default(),
        }
    }

    fn rebuild(&mut self, ctx: &Ctx) {
        let (Some(pack), Some(session)) = (ctx.pack.as_deref(), ctx.session.as_ref()) else {
            return;
        };
        let items = session
            .campaign
            .roster
            .iter()
            .map(|o| MenuItem::new(officer_name(pack, &o.id)))
            .collect();
        // The table (and the detail page) fill the camp's content area.
        let table = content_rect(ctx.gfx.size());
        let rows = ((table.h - 36.0) / ROW_H).floor() as usize;
        let cursor = self.menu.cursor();
        let mut menu = Menu::new(items)
            .rows(rows)
            .at(table.x + 2.0, table.y + 30.0, table.w - 4.0);
        menu.framed = false;
        menu.row_height = ROW_H;
        menu.tag_width = 28.0;
        menu.wrap = false;
        menu.set_cursor(cursor);
        self.menu = menu;
    }

    fn draw_table(&self, ctx: &Ctx, pack: &Pack, roster: &[OfficerState]) {
        let gfx = &ctx.gfx;
        let table = content_rect(gfx.size());
        draw_list_frame(ctx, table, "무장 일람", true);
        let head = TextStyle::small(theme::TEXT_DIM);
        let base = self.menu.row_rect(0).x;
        let hy = table.y + 16.0;
        gfx.text("이름", base + 40.0, hy, head);
        gfx.text(COLUMNS[0].0, base + COLUMNS[0].1, hy, head);
        // Number columns: headers right-aligned over their numbers.
        for k in 1..COLUMNS.len() - 1 {
            let (label, x0) = COLUMNS[k];
            let x1 = COLUMNS[k + 1].1 - 10.0;
            gfx.text_aligned(label, base + x0, hy, x1 - x0, Align::Right, head);
        }
        self.menu.draw(ctx);
        let value = TextStyle::main(theme::TEXT).shadow(theme::TEXT_SHADOW);
        for (i, row) in visible_rows(&self.menu) {
            let Some(o) = roster.get(i) else {
                continue;
            };
            draw_officer_sprite(
                ctx,
                pack,
                o,
                vec2(row.x + 26.0, row.bottom() - 1.0),
                i == self.menu.cursor(),
            );
            let y = row.y + (row.h - 16.0) / 2.0;
            gfx.text(class_name(pack, &o.class), row.x + COLUMNS[0].1, y, value);
            let s = officer_stats(pack, o);
            let cols = [
                i64::from(o.level),
                s.map_or(0, |s| i64::from(s.hp)),
                s.map_or(0, |s| i64::from(s.mp)),
                s.map_or(0, |s| i64::from(s.atk)),
                s.map_or(0, |s| i64::from(s.def)),
                s.map_or(0, |s| i64::from(s.mov)),
            ];
            for (k, v) in cols.into_iter().enumerate() {
                // Right-align each number under its header.
                let x0 = row.x + COLUMNS[k + 1].1;
                let x1 = row.x + COLUMNS[k + 2].1 - 10.0;
                gfx.text_aligned(&format::thousands(v), x0, y, x1 - x0, Align::Right, value);
            }
        }
    }

    fn draw_detail(&self, ctx: &Ctx, pack: &Pack, o: &OfficerState) {
        let gfx = &ctx.gfx;
        let Some(def) = pack.officer(&o.id) else {
            return;
        };
        let panel = content_rect(gfx.size());
        draw_window_ex(panel, WindowStyle::Panel, 1.0);
        let x = panel.x + 8.0;
        let y = panel.y + 8.0;
        // Portrait and names.
        draw_portrait_card(
            ctx,
            Some(portrait_key(pack, &o.id)),
            portrait_rect(gfx.size()),
            1.0,
            1.0,
        );
        gfx.text(
            &def.name,
            x,
            y + 124.0,
            TextStyle::main(theme::TEXT_NAME)
                .size(2)
                .shadow(theme::TEXT_SHADOW),
        );
        let small = TextStyle::small(theme::TEXT_DIM);
        let mut names = Vec::new();
        if !def.hanja.is_empty() {
            names.push(def.hanja.clone());
        }
        if !def.courtesy.is_empty() {
            names.push(format!("자 {}", def.courtesy));
        }
        gfx.text(
            &names.join("  "),
            x,
            y + 158.0,
            TextStyle::main(theme::TEXT),
        );
        if def.lord {
            draw_icon(ctx, "lord", vec2(x + 80.0, y + 128.0));
        }

        // Class, level, EXP and battle values.
        let cx = x + 108.0;
        let cw = 150.0;
        gfx.text(
            &format!("{}  Lv{}", class_name(pack, &o.class), o.level),
            cx,
            y,
            TextStyle::main(theme::TEXT).shadow(theme::TEXT_SHADOW),
        );
        if let Some(c) = pack.class(&o.class) {
            if !c.hanja.is_empty() {
                gfx.text_aligned(&c.hanja, cx, y + 2.0, cw, Align::Right, small);
            }
        }
        let per_level = pack.rules.exp_per_level.max(1);
        if o.level >= pack.rules.level_cap {
            gfx.text("경험치", cx, y + 18.0, small);
            gfx.text_aligned("최고 레벨", cx, y + 18.0, cw, Align::Right, small);
        } else {
            draw_gauge_labeled(
                gfx,
                vec2(cx, y + 18.0),
                cw,
                "경험치",
                i64::from(o.exp),
                i64::from(per_level),
                GaugeKind::Exp,
            );
        }
        if let Some(stats) = officer_stats(pack, o) {
            draw_stats_block(gfx, &stats, None, cx, y + 40.0, cw);
        }
        // 무력 / 지력 / 통솔 (the row being edited lit up behind it).
        if let Some(row) = self.edit {
            draw_highlight(ability_rect(gfx.size(), row), true, ctx.time);
        }
        let by = y + 120.0;
        for (i, (label, v)) in [("무력", o.strength), ("지력", o.int), ("통솔", o.lead)]
            .into_iter()
            .enumerate()
        {
            let ry = by + i as f32 * 14.0;
            gfx.text(label, cx, ry, small);
            gfx.text_aligned(
                &v.to_string(),
                cx,
                ry - 1.0,
                cw,
                Align::Right,
                TextStyle::main(theme::TEXT).shadow(theme::TEXT_SHADOW),
            );
            draw_gauge(
                Rect::new(cx + 28.0, ry + 4.0, cw - 56.0, 5.0),
                v as f32,
                100.0,
                GaugeKind::Custom(theme::TEXT_ACCENT),
            );
            if self.edit == Some(i) {
                draw_side_arrow(cx + 26.0, ry + 6.0, false, theme::TEXT_ACCENT);
                draw_side_arrow(cx + cw + 4.0, ry + 6.0, true, theme::TEXT_ACCENT);
            }
        }

        // Equipment and strategies.
        let rx = cx + cw + 14.0;
        let rw = panel.right() - 8.0 - rx;
        draw_caption(gfx, "장비", rx, y);
        for (i, slot) in [ItemKind::Weapon, ItemKind::Armor, ItemKind::Accessory]
            .into_iter()
            .enumerate()
        {
            let id = match slot {
                ItemKind::Weapon => o.equip.weapon.as_ref(),
                ItemKind::Armor => o.equip.armor.as_ref(),
                _ => o.equip.accessory.as_ref(),
            };
            let item = id.and_then(|id| pack.item(id));
            let ry = y + 14.0 + i as f32 * 17.0;
            draw_icon(ctx, item.map_or(slot_icon(slot), item_icon), vec2(rx, ry));
            gfx.text(slot_name(slot), rx + 20.0, ry + 2.0, small);
            gfx.text(
                item.map_or("—", |i| i.name.as_str()),
                rx + 62.0,
                ry,
                TextStyle::main(theme::TEXT).shadow(theme::TEXT_SHADOW),
            );
        }
        let sy = y + 70.0;
        draw_divider(rx, sy - 4.0, rw);
        draw_caption(gfx, "책략", rx, sy);
        let strategies = strategy_list(pack, o);
        if strategies.is_empty() {
            gfx.text("없음", rx, sy + 14.0, TextStyle::main(theme::TEXT_DIM));
        } else {
            let col_w = (rw / 2.0).floor();
            let rows = 6;
            for (i, (name, mp)) in strategies.iter().take(rows * 2).enumerate() {
                let (col, row) = (i / rows, i % rows);
                let px = rx + col as f32 * col_w;
                let py = sy + 14.0 + row as f32 * 14.0;
                gfx.text(name, px, py, TextStyle::small(theme::TEXT));
                gfx.text_aligned(
                    &mp.to_string(),
                    px,
                    py,
                    col_w - 8.0,
                    Align::Right,
                    TextStyle::small(theme::MP),
                );
            }
            if strategies.len() > rows * 2 {
                gfx.text_aligned(
                    &format!("외 {}개", strategies.len() - rows * 2),
                    rx,
                    sy,
                    rw,
                    Align::Right,
                    small,
                );
            }
        }

        // Biography.
        let bio_y = panel.bottom() - 46.0;
        draw_divider(cx, bio_y - 5.0, panel.right() - 8.0 - cx);
        let lines = gfx.wrap(&def.bio, FontId::Main, 1, panel.right() - 12.0 - cx);
        gfx.text_lines(
            &lines[..lines.len().min(3)],
            cx,
            bio_y,
            TextStyle::main(theme::TEXT_DIM).shadow(theme::TEXT_SHADOW),
        );
    }
}

impl OfficersScreen {
    /// The officer list on the status window: taps, direction keys (left and right step, up and
    /// down move by a row), confirm opens the detail page, cancel or the close button leaves.
    fn update_status(&mut self, ctx: &mut Ctx, frame: &StatusFrame, count: usize) -> Transition {
        let layout = StatusLayout::new(frame, ctx.gfx.size());
        let cursor = self.menu.cursor().min(count.saturating_sub(1));
        let mut action = StatusAction::None;
        if let Some(p) = ctx.input.tap() {
            action = status_tap(&layout, p, cursor, count);
            if action != StatusAction::None {
                ctx.input.consume();
            }
        } else if let Some(dir) = ctx.input.nav() {
            let step = match dir {
                Dir::Left => -1,
                Dir::Right => 1,
                Dir::Up => -(layout.columns() as i32),
                Dir::Down => layout.columns() as i32,
            };
            // A step off the army (or off the top or bottom row) goes nowhere.
            let to = cursor as i32 + step;
            if (0..count as i32).contains(&to) {
                action = StatusAction::Choose(to as usize);
            }
        } else if ctx.input.confirm_key() && count > 0 {
            action = StatusAction::Open;
        } else if ctx.input.cancel() {
            action = StatusAction::Close;
        }
        match action {
            StatusAction::Choose(i) => {
                ctx.sfx(sfx::CURSOR);
                self.menu.set_cursor(i.min(count.saturating_sub(1)));
            }
            StatusAction::Open => {
                ctx.sfx(sfx::CONFIRM);
                self.detail = Some(cursor);
            }
            StatusAction::Close => {
                ctx.sfx(sfx::CANCEL);
                return Transition::Pop;
            }
            StatusAction::None => {}
        }
        Transition::None
    }

    fn draw_status(&self, ctx: &Ctx, pack: &Pack, roster: &[OfficerState], frame: &StatusFrame) {
        let gfx = &ctx.gfx;
        let layout = StatusLayout::new(frame, gfx.size());
        let whole = layout.rect([0, 0, frame.size[0], frame.size[1]]);
        // A picture of another size is not stretched: the window is filled plainly (as validate
        // warns), like the other frames.
        let [w, h] = frame.size;
        let picture = ctx
            .media
            .texture(&frame.image)
            .filter(|t| t.size() == vec2(w as f32, h as f32));
        match picture {
            Some(tex) => draw_texture_ex(
                &tex,
                whole.x,
                whole.y,
                WHITE,
                DrawTextureParams {
                    dest_size: Some(whole.size()),
                    ..Default::default()
                },
            ),
            None => draw_window_ex(whole, WindowStyle::Panel, 1.0),
        }
        let value = TextStyle::main(theme::TEXT).shadow(theme::TEXT_SHADOW);
        let line = gfx.line_height(FontId::Main, 1);
        // Text centred on a box's height, up to 3 px in from its sides as far as it fits;
        // text wider than the box is cut short.
        let put = |text: &str, r: Rect, align: Align, style: TextStyle| {
            let mut shown = text.to_string();
            while !shown.is_empty() && gfx.text_width(&shown, FontId::Main, 1) > r.w {
                shown.pop();
            }
            let pad = ((r.w - gfx.text_width(&shown, FontId::Main, 1)) / 2.0).clamp(0.0, 3.0);
            let y = (r.y + (r.h - line) / 2.0).round();
            gfx.text_aligned(&shown, r.x + pad, y, r.w - 2.0 * pad, align, style);
        };
        put(
            "무장 정보",
            layout.rect(frame.title),
            Align::Left,
            TextStyle::main(theme::TEXT_NAME).shadow(theme::TEXT_SHADOW),
        );
        let per = layout.per_page();
        let count = roster.len();
        let cursor = self.menu.cursor().min(count.saturating_sub(1));
        let page = cursor / per;
        for (k, slot) in frame.slots.iter().enumerate() {
            let i = page * per + k;
            let Some(o) = roster.get(i) else {
                continue;
            };
            let icon = layout.rect(slot.icon);
            // The chosen officer's box lights up behind the unit.
            if i == cursor {
                draw_highlight(icon, true, ctx.time);
            }
            draw_officer_sprite(
                ctx,
                pack,
                o,
                vec2(icon.center().x, icon.bottom()),
                i == cursor,
            );
            put(
                &o.level.to_string(),
                layout.rect(slot.level),
                Align::Right,
                value,
            );
            let troops = officer_stats(pack, o).map_or(0, |s| i64::from(s.hp));
            put(
                &format::thousands(troops),
                layout.rect(slot.troops),
                Align::Right,
                value,
            );
        }
        let pages = count.div_ceil(per).max(1);
        put(
            &format!("{}/{pages}", page + 1),
            layout.rect(frame.page),
            Align::Center,
            value,
        );
        let rest = count.saturating_sub((page + 1) * per);
        put(
            &rest.to_string(),
            layout.rect(frame.rest),
            Align::Center,
            value,
        );

        // The chosen officer.
        let Some(o) = roster.get(cursor) else {
            return;
        };
        draw_portrait_card(
            ctx,
            Some(portrait_key(pack, &o.id)),
            layout.rect(frame.portrait),
            1.0,
            1.0,
        );
        put(
            officer_name(pack, &o.id),
            layout.rect(frame.name),
            Align::Center,
            value,
        );
        put(
            &o.level.to_string(),
            layout.rect(frame.level),
            Align::Right,
            value,
        );
        for (area, v) in [
            (frame.lead, o.lead),
            (frame.strength, o.strength),
            (frame.intellect, o.int),
        ] {
            put(&v.to_string(), layout.rect(area), Align::Right, value);
        }
        put(
            class_name(pack, &o.class),
            layout.rect(frame.class),
            Align::Center,
            value,
        );
        // Equipment, then the strategies with their MP, as far as the box holds them.
        let info = inset(layout.rect(frame.info), 4.0);
        let small = gfx.line_height(FontId::Small, 1);
        // Each line with its colour and whether it is an entry (not a heading).
        let mut lines: Vec<(String, Color, bool)> = Vec::new();
        for (slot, id) in [
            (ItemKind::Weapon, o.equip.weapon.as_ref()),
            (ItemKind::Armor, o.equip.armor.as_ref()),
            (ItemKind::Accessory, o.equip.accessory.as_ref()),
        ] {
            let item = id
                .and_then(|id| pack.item(id))
                .map_or("—", |i| i.name.as_str());
            lines.push((format!("{} {item}", slot_name(slot)), theme::TEXT, true));
        }
        let strategies = strategy_list(pack, o);
        lines.push(("책략".to_string(), theme::TEXT_DIM, false));
        if strategies.is_empty() {
            lines.push(("없음".to_string(), theme::TEXT_DIM, false));
        }
        for (name, mp) in strategies {
            lines.push((format!("{name} {mp}"), theme::TEXT, true));
        }
        let fit = ((info.h / small).floor() as usize).max(1);
        let overflow = lines.len() > fit;
        for (n, (text, color, _)) in lines.iter().take(fit).enumerate() {
            let text = if overflow && n + 1 == fit {
                let hidden = lines[n..].iter().filter(|(_, _, entry)| *entry).count();
                format!("외 {hidden}개")
            } else {
                text.clone()
            };
            gfx.text(
                &text,
                info.x,
                info.y + n as f32 * small,
                TextStyle::small(*color),
            );
        }
    }

    /// The hidden command: its prompt, its orb and taps on the lord's portrait. Returns `true`
    /// when it took the input.
    fn update_secret(&mut self, ctx: &mut Ctx) -> bool {
        let Some(pack) = ctx.pack.clone() else {
            return false;
        };
        if let Some(mut prompt) = self.prompt.take() {
            match prompt.update(ctx) {
                ConfirmEvent::None => self.prompt = Some(prompt),
                answer => {
                    ctx.input.consume();
                    let yes = answer == ConfirmEvent::Yes;
                    if let Some(session) = ctx.session.as_mut() {
                        session.secret.answer(yes);
                    }
                    if yes {
                        ctx.sfx(sfx::TREASURE);
                    }
                }
            }
            return true;
        }
        let enabled = ctx.session.as_ref().is_some_and(|s| s.secret.enabled());
        if enabled && ctx.input.tapped(orb_rect()) {
            ctx.input.consume();
            let Some(session) = ctx.session.as_mut() else {
                return true;
            };
            let gold = session.campaign.gold;
            if let Some(lord) = session.campaign.forbidden_secret(&pack) {
                let level = session.campaign.officer(&lord).map_or(0, |o| o.level);
                // The gold the army actually got (the cap may cut it).
                let gained = session.campaign.gold - gold;
                ctx.sfx(sfx::LEVELUP);
                ctx.toast(format!(
                    "금단의 비법: {} Lv {level} · 무력·지력·통솔 {} · 군자금 +{}",
                    officer_name(&pack, &lord),
                    hero_core::campaign::FORBIDDEN_SECRET_ABILITY,
                    format::thousands(gained),
                ));
            }
            return true;
        }
        // A tap on the lord's portrait counts, until the command is enabled.
        let is_lord = |i: usize| {
            ctx.session
                .as_ref()
                .and_then(|s| s.campaign.roster.get(i))
                .and_then(|o| pack.officer(&o.id))
                .is_some_and(|d| d.lord)
        };
        let target = secret_portrait(
            ctx.gfx.size(),
            pack.manifest.presentation.status_frame.as_ref(),
            self.detail,
            self.menu.cursor(),
            is_lord,
        );
        if enabled || !target.is_some_and(|r| ctx.input.tapped(r)) {
            return false;
        }
        ctx.input.consume();
        let step = ctx
            .session
            .as_mut()
            .map_or(SecretStep::None, |s| s.secret.tap());
        match step {
            SecretStep::None => {}
            SecretStep::Chime => ctx.sfx(sfx::PHASE),
            SecretStep::Ask => {
                self.prompt = Some(
                    ConfirmDialog::new(&ctx.gfx, SECRET_PROMPT)
                        .labels("예", "아니오")
                        .default_no(),
                );
            }
        }
        true
    }

    fn draw_secret(&self, ctx: &Ctx) {
        if ctx.session.as_ref().is_some_and(|s| s.secret.enabled()) {
            let c = orb_rect().center();
            draw_circle(c.x, c.y, 4.0, Color::from_hex(0x10267a));
            draw_circle(c.x, c.y, 3.0, Color::from_hex(0x3f7bff));
            draw_circle(c.x - 1.0, c.y - 1.0, 1.0, Color::from_hex(0xcfe0ff));
        }
        if let Some(prompt) = &self.prompt {
            fill_rect(ctx.gfx.screen(), Color::new(0.0, 0.0, 0.0, 0.4));
            prompt.draw(ctx);
        }
    }
}

impl OfficersScreen {
    /// 능력치 자유 조정 on the detail page of officer `i`. Returns `true` when it took the input.
    fn update_edit(&mut self, ctx: &mut Ctx, i: usize, back: bool) -> bool {
        let canvas = ctx.gfx.size();
        let tapped_row = ctx.input.tap().and_then(|p| {
            (0..3)
                .find(|&k| ability_rect(canvas, k).contains(p))
                .map(|k| (k, p))
        });
        let Some(row) = self.edit else {
            // Not editing: confirm or a tap on the abilities starts.
            if back {
                return false;
            }
            if let Some((k, _)) = tapped_row {
                ctx.input.consume();
                ctx.sfx(sfx::CONFIRM);
                self.edit = Some(k);
                return true;
            }
            if ctx.input.confirm_key() {
                ctx.input.consume();
                ctx.sfx(sfx::CONFIRM);
                self.edit = Some(0);
                return true;
            }
            return false;
        };
        let mut delta = 0;
        if back || ctx.input.cancel() || ctx.input.confirm_key() {
            ctx.input.consume();
            if !back {
                ctx.sfx(sfx::CANCEL);
            }
            self.edit = None;
            self.edit_hold = (None, 0.0);
            return true;
        }
        // A press on a row steps it at once and again while held, as a held key does; the tap
        // that ends the press then does nothing more.
        let pressed = ctx
            .input
            .pressed()
            .then(|| ctx.input.pointer())
            .flatten()
            .and_then(|p| ability_side(canvas, p));
        if let Some((k, _)) = pressed {
            self.edit = Some(k);
        }
        let row = self.edit.unwrap_or(row);
        let input = &ctx.input;
        let pressing = |d: Dir| {
            input.down()
                && input
                    .pointer()
                    .and_then(|p| ability_side(canvas, p))
                    .is_some_and(|(k, side)| k == row && side == d)
        };
        let press_step = self
            .press_repeat
            .step(ctx.dt, pressed.map(|(_, d)| d), pressing);
        let stepping = [Dir::Left, Dir::Right]
            .into_iter()
            .find(|&d| input.held(d) || pressing(d));
        self.edit_hold = match self.edit_hold {
            (way, t) if way == stepping && stepping.is_some() => (way, t + ctx.dt),
            _ => (stepping, 0.0),
        };
        let step = edit_step(self.edit_hold.1);
        let by = |d: Dir| if d == Dir::Left { -step } else { step };
        if let Some(d) = press_step {
            ctx.input.consume();
            delta = by(d);
        } else if tapped_row.is_some() {
            ctx.input.consume();
            return true;
        } else if ctx.input.tap().is_some() {
            // A tap anywhere else ends editing.
            ctx.input.consume();
            ctx.sfx(sfx::CANCEL);
            self.edit = None;
            return true;
        } else {
            match ctx.input.nav() {
                Some(Dir::Up) => self.edit = Some((row + 2) % 3),
                Some(Dir::Down) => self.edit = Some((row + 1) % 3),
                Some(d @ (Dir::Left | Dir::Right)) => delta = by(d),
                None => return true,
            }
            if delta == 0 {
                ctx.sfx(sfx::CURSOR);
            }
        }
        if delta != 0 {
            let row = self.edit.unwrap_or(row);
            let ability = Ability::ALL[row];
            if let Some(session) = ctx.session.as_mut() {
                let campaign = &mut session.campaign;
                let Some(o) = campaign.roster.get(i) else {
                    return true;
                };
                let id = o.id.clone();
                let now = match ability {
                    Ability::Strength => o.strength,
                    Ability::Int => o.int,
                    Ability::Lead => o.lead,
                };
                let set = campaign.set_ability(&id, ability, now + delta);
                if set == Some(now) {
                    ctx.sfx(sfx::ERROR);
                } else {
                    ctx.sfx(sfx::CURSOR);
                }
            }
        }
        true
    }
}

impl Screen for OfficersScreen {
    fn in_camp_frame(&self) -> bool {
        true
    }

    fn name(&self) -> &'static str {
        "camp-officers"
    }

    fn on_enter(&mut self, ctx: &mut Ctx, _how: Enter) {
        self.rebuild(ctx);
    }

    fn update(&mut self, ctx: &mut Ctx) -> Transition {
        if self.update_secret(ctx) {
            return Transition::None;
        }
        let count = ctx.session.as_ref().map_or(0, |s| s.campaign.roster.len());
        let pack = ctx.pack.clone();
        let status = pack
            .as_deref()
            .and_then(|p| p.manifest.presentation.status_frame.as_ref());
        // The back button is not drawn on the status window: it takes no taps there.
        let back = (self.detail.is_some() || status.is_none()) && back_tapped(ctx);
        if let Some(i) = self.detail {
            let free_edit = ctx.session.as_ref().is_some_and(|s| s.campaign.free_edit);
            if free_edit && self.update_edit(ctx, i, back) {
                return Transition::None;
            }
            // Keys page through the army; a tap on the left or right half does the same.
            let mut step = match ctx.input.nav() {
                Some(Dir::Left | Dir::Up) => -1,
                Some(Dir::Right | Dir::Down) => 1,
                _ => 0,
            };
            if let Some(p) = ctx.input.tap() {
                step = if p.x < ctx.gfx.size().x / 2.0 { -1 } else { 1 };
            }
            if back || ctx.input.cancel() || ctx.input.confirm_key() {
                ctx.input.consume();
                if !back {
                    ctx.sfx(sfx::CANCEL);
                }
                self.detail = None;
            } else if step != 0 && count > 1 {
                let next = (i as i32 + step).rem_euclid(count as i32) as usize;
                self.detail = Some(next);
                self.menu.set_cursor(next);
                ctx.sfx(sfx::CURSOR);
            }
            return Transition::None;
        }
        if let Some(frame) = status {
            return self.update_status(ctx, frame, count);
        }
        if back {
            return Transition::Pop;
        }
        match self.menu.update(ctx) {
            MenuEvent::Selected(i) if i < count => self.detail = Some(i),
            MenuEvent::Cancelled => return Transition::Pop,
            _ => {}
        }
        Transition::None
    }

    fn draw(&self, ctx: &Ctx) {
        let (Some(pack), Some(session)) = (ctx.pack.as_deref(), ctx.session.as_ref()) else {
            return;
        };
        let campaign = &session.campaign;
        draw_camp_backdrop(ctx, 0.8);
        match self
            .detail
            .and_then(|i| campaign.roster.get(i).map(|o| (i, o)))
        {
            Some((i, o)) => {
                draw_header(
                    ctx,
                    &format!("무장 정보 — {}", officer_name(pack, &o.id)),
                    campaign.gold,
                );
                self.draw_detail(ctx, pack, o);
                draw_help(
                    ctx,
                    if self.edit.is_some() {
                        "↑↓ 능력치 · ←→ 조정(길게: 빠르게) · Z/X 끝"
                    } else if campaign.free_edit {
                        "←→ 다른 무장 · Z 능력치 조정 · X 목록으로"
                    } else {
                        "←→ 다른 무장 · X 목록으로"
                    },
                );
                ctx.gfx.text_aligned(
                    &format!("{} / {}", i + 1, campaign.roster.len()),
                    0.0,
                    help_y(ctx.gfx.size().y) + 1.0,
                    back_button(ctx.gfx.size()).x - 8.0,
                    Align::Right,
                    TextStyle::small(theme::TEXT_DIM),
                );
            }
            None => {
                if let Some(frame) = &pack.manifest.presentation.status_frame {
                    self.draw_status(ctx, pack, &campaign.roster, frame);
                    self.draw_secret(ctx);
                    return;
                }
                draw_header(ctx, "무장 정보", campaign.gold);
                self.draw_table(ctx, pack, &campaign.roster);
                draw_help(ctx, "Z 자세히 · X 돌아가기");
            }
        }
        draw_back_button(ctx);
        self.draw_secret(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screens::camp::test_pack;
    use hero_core::campaign::CampaignState;

    /// The original's status window: 512×320, six slots in two columns.
    fn status_frame() -> StatusFrame {
        let slot = |x: u32, y: u32| hero_core::pack::StatusSlot {
            icon: [x, y, 32, 32],
            level: [x, y + 32, 32, 16],
            troops: [x + 88, y + 8, 40, 16],
        };
        StatusFrame {
            image: "ui/status".into(),
            size: [512, 320],
            title: [8, 8, 288, 32],
            slots: [
                (16, 64),
                (160, 64),
                (16, 128),
                (160, 128),
                (16, 192),
                (160, 192),
            ]
            .into_iter()
            .map(|(x, y)| slot(x, y))
            .collect(),
            portrait: [320, 16, 64, 80],
            name: [400, 8, 80, 16],
            level: [464, 32, 16, 16],
            lead: [456, 64, 24, 16],
            strength: [456, 96, 24, 16],
            intellect: [456, 128, 24, 16],
            class: [320, 112, 64, 32],
            info: [314, 170, 188, 140],
            page: [48, 272, 48, 32],
            pager: [96, 272, 32, 32],
            rest: [176, 272, 48, 32],
            close: [240, 272, 48, 32],
        }
    }

    #[test]
    fn the_status_window_sits_in_the_camp_view_and_pages_the_army() {
        let frame = status_frame();
        // In the original's camp view (511×322) it lands where the original draws it.
        let layout = StatusLayout::new(&frame, vec2(511.0, 322.0));
        assert_eq!(layout.origin, vec2(-1.0, 1.0));
        assert_eq!((layout.per_page(), layout.columns()), (6, 2));
        let at = |x: f32, y: f32| layout.origin + vec2(x, y);
        // A tap on another officer's slot moves to them; on the chosen one opens its page.
        assert_eq!(
            status_tap(&layout, at(170.0, 70.0), 0, 10),
            StatusAction::Choose(1)
        );
        assert_eq!(
            status_tap(&layout, at(20.0, 70.0), 0, 10),
            StatusAction::Open
        );
        assert_eq!(
            status_tap(&layout, at(120.0, 80.0), 0, 10),
            StatusAction::Open
        );
        // Empty slots of the last page do nothing.
        assert_eq!(
            status_tap(&layout, at(170.0, 200.0), 6, 8),
            StatusAction::None
        );
        // The pager: bottom half the next page, top half the previous (wrapping).
        assert_eq!(
            status_tap(&layout, at(100.0, 300.0), 0, 10),
            StatusAction::Choose(6)
        );
        assert_eq!(
            status_tap(&layout, at(100.0, 275.0), 0, 10),
            StatusAction::Choose(6)
        );
        assert_eq!(
            status_tap(&layout, at(100.0, 300.0), 7, 10),
            StatusAction::Choose(0)
        );
        assert_eq!(
            status_tap(&layout, at(260.0, 280.0), 3, 10),
            StatusAction::Close
        );
        assert_eq!(
            status_tap(&layout, at(400.0, 250.0), 3, 10),
            StatusAction::None
        );
        // Without officers nothing opens.
        assert_eq!(
            status_tap(&layout, at(20.0, 70.0), 0, 0),
            StatusAction::None
        );
    }

    /// A short press steps by 1; a held one by 5 after a second and by 10 after two, so 1 to
    /// 100 takes about two seconds of holding at the key repeat's rate.
    #[test]
    fn a_held_ability_steps_faster() {
        assert_eq!(edit_step(0.0), 1);
        assert_eq!(edit_step(0.99), 1);
        assert_eq!(edit_step(1.0), 5);
        assert_eq!(edit_step(2.0), 10);
        // Stepped at the key repeat's timing from 1 upward.
        let (mut value, mut t, mut steps) = (1, 0.0f32, 0);
        let mut repeat = KeyRepeat::default();
        let mut pressed = Some(Dir::Right);
        while value < 100 {
            if repeat.step(0.016, pressed.take(), |_| true).is_some() {
                value = (value + edit_step(t)).min(100);
                steps += 1;
            }
            t += 0.016;
            assert!(t < 3.0, "still at {value} after {t} s");
        }
        assert!(steps < 30, "{steps} steps");
    }

    #[test]
    fn a_press_on_an_ability_row_picks_the_way_by_its_half() {
        let canvas = vec2(640.0, 400.0);
        let r = ability_rect(canvas, 1);
        assert_eq!(
            ability_side(canvas, vec2(r.x + 2.0, r.center().y)),
            Some((1, Dir::Left))
        );
        assert_eq!(
            ability_side(canvas, vec2(r.right() - 2.0, r.center().y)),
            Some((1, Dir::Right))
        );
        assert_eq!(ability_side(canvas, vec2(r.x - 5.0, r.center().y)), None);
    }

    /// The lord's portrait counts for the hidden command on the detail page and, in a pack
    /// with the original's status window, on that window while the lord is chosen.
    #[test]
    fn the_lords_portrait_counts_on_the_status_window_too() {
        let canvas = vec2(511.0, 322.0);
        let lord = |i: usize| i == 0;
        // The detail pages: the lord's only.
        assert_eq!(
            secret_portrait(canvas, None, Some(0), 0, lord),
            Some(portrait_rect(canvas))
        );
        assert_eq!(secret_portrait(canvas, None, Some(1), 0, lord), None);
        // The table of a pack without a status window shows no portrait.
        assert_eq!(secret_portrait(canvas, None, None, 0, lord), None);
        // The status window: its portrait, while the lord is the chosen officer.
        let frame = status_frame();
        let layout = StatusLayout::new(&frame, canvas);
        let side = layout.rect(frame.portrait);
        assert_eq!(
            secret_portrait(canvas, Some(&frame), None, 0, lord),
            Some(side)
        );
        assert_eq!(secret_portrait(canvas, Some(&frame), None, 2, lord), None);
        // The portrait is no slot: a tap there did nothing on the window before.
        assert_eq!(
            status_tap(&layout, side.center(), 0, 10),
            StatusAction::None
        );
    }

    #[test]
    fn strategies_follow_class_and_level() {
        let pack = test_pack();
        let mut campaign = CampaignState::new_game(&pack);
        let liu = campaign.officer("liu_bei").unwrap().clone();
        let known = strategy_list(&pack, &liu);
        assert_eq!(
            known.len(),
            pack.known_strategies(&liu.class, liu.level).len()
        );
        campaign.officer_mut("liu_bei").unwrap().level = 50;
        let later = strategy_list(&pack, campaign.officer("liu_bei").unwrap());
        assert!(later.len() >= known.len());
        assert!(later.iter().all(|(name, mp)| !name.is_empty() && *mp >= 0));
    }
}
