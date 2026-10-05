//! 출진 준비: the camp screen of a campaign `Camp` node and its sub-screens.
//!
//! The hub ([`CampScreen`]) shows the camp's title and the army's gold, a command menu and, when
//! the camp prepares a battle, the battle's objective and the current deployment:
//!
//! | command | screen |
//! |---|---|
//! | 출진 (다음으로 without a battle) | deployment confirmation → `Flow::Advance` |
//! | 부대 편성 (battle only) | [`deploy::DeployScreen`] — `CampaignState::deployed` |
//! | 장비 | [`equip::EquipScreen`] — weapons, war manuals, treasures |
//! | 상점 | [`shop::ShopScreen`] — buy the node's `shop` list, sell at half price |
//! | 도구 | [`tools::ToolsScreen`] — class-up and class-change items |
//! | 무장 정보 | [`officers::OfficersScreen`] — roster table and detail pages |
//! | 기록 / 불러오기 / 설정 | the save/load and settings screens |
//! | 타이틀로 | back to the title screen (after a confirmation) |
//!
//! Every change goes through `hero_core::campaign::CampaignState` (buy, sell, equip, unequip,
//! use_item), so the camp obeys exactly the rules the engine checks; stats are computed by the
//! battle engine ([`stats`]).

pub mod deploy;
pub mod equip;
pub mod frame;
pub mod officers;
pub mod shop;
pub mod stats;
pub mod tools;
pub mod widgets;

use self::deploy::{deploy_max, initial_selection, normalize_deployment, DeployScreen};
use self::widgets::{
    class_name, draw_camp_backdrop, draw_caption, draw_header, draw_help, draw_officer_sprite,
    help_y, officer_name, TwoButtons, TOP,
};
use super::saveload::SaveLoadScreen;
use super::settings::SettingsScreen;
use crate::app::{Ctx, Enter, Screen, Transition};
use crate::audio::{bgm, sfx};
use crate::flow::Flow;
use crate::gfx::{fill_rect, Align, FontId, Gfx, TextStyle};
use crate::ui::dialog::{ConfirmDialog, ConfirmEvent};
use crate::ui::format;
use crate::ui::menu::{Menu, MenuEvent, MenuItem};
use crate::ui::theme;
use crate::ui::window::{draw_divider, draw_window, draw_window_ex, WindowStyle};
use hero_core::battledef::BattleDef;
use hero_core::campaign::CampaignState;
use hero_core::data::Id;
use hero_core::pack::Pack;
use macroquad::prelude::*;

/// Commands of the camp menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command {
    Sortie,
    Deploy,
    Equip,
    Shop,
    Tools,
    Officers,
    Save,
    Load,
    /// 순간 저장 (F5): for touch screens, which have no keys (the dialogue and battle menus have
    /// it too).
    QuickSave,
    /// 순간 불러오기 (F9).
    QuickLoad,
    Settings,
    Title,
}

impl Command {
    fn label(self, has_battle: bool) -> &'static str {
        match self {
            Command::Sortie if has_battle => "출진",
            Command::Sortie => "다음으로",
            Command::Deploy => "부대 편성",
            Command::Equip => "장비",
            Command::Shop => "상점",
            Command::Tools => "도구",
            Command::Officers => "무장 정보",
            Command::Save => "기록",
            Command::Load => "불러오기",
            Command::QuickSave => "순간 저장",
            Command::QuickLoad => "순간 불러오기",
            Command::Settings => "설정",
            Command::Title => "타이틀로",
        }
    }
}

/// Commands shown for a camp (부대 편성 only when it prepares a battle).
fn commands(has_battle: bool) -> Vec<Command> {
    let mut v = vec![Command::Sortie];
    if has_battle {
        v.push(Command::Deploy);
    }
    v.extend([
        Command::Equip,
        Command::Shop,
        Command::Tools,
        Command::Officers,
        Command::Save,
        Command::Load,
        Command::QuickSave,
        Command::QuickLoad,
        Command::Settings,
        Command::Title,
    ]);
    v
}

/// The deployment confirmation (출진): battle, objective and the deployed officers with their
/// unit sprites.
struct SortieDialog {
    selection: Vec<Id>,
    objective: Vec<String>,
    buttons: TwoButtons,
    rect: Rect,
}

const SORTIE_W: f32 = 300.0;
/// One deployed officer: sprite above, name below.
const CELL: Vec2 = Vec2::new(46.0, 40.0);

impl SortieDialog {
    fn new(gfx: &Gfx, def: &BattleDef, selection: Vec<Id>) -> SortieDialog {
        let inner_w = SORTIE_W - 24.0;
        let mut objective = gfx.wrap(
            &format!("승리 조건: {}", def.objective),
            FontId::Main,
            1,
            inner_w,
        );
        objective.truncate(2);
        let h = 8.0
            + 20.0
            + 17.0
            + 16.0 * objective.len() as f32
            + 8.0
            + Self::cell_rows(selection.len()) as f32 * CELL.y
            + 8.0
            + widgets::BUTTON_H
            + 12.0;
        let canvas = gfx.size();
        SortieDialog {
            selection,
            objective,
            buttons: TwoButtons::new("출진", "취소"),
            rect: Rect::new(
                ((canvas.x - SORTIE_W) / 2.0).round(),
                ((canvas.y - h) / 2.0).round(),
                SORTIE_W,
                h.round(),
            ),
        }
    }

    fn per_row() -> usize {
        ((SORTIE_W - 24.0) / CELL.x).floor().max(1.0) as usize
    }

    fn cell_rows(n: usize) -> usize {
        n.div_ceil(Self::per_row()).max(1)
    }

    /// Centre x and top y of the button row.
    fn buttons_at(&self) -> (f32, f32) {
        (
            self.rect.x + self.rect.w / 2.0,
            self.rect.bottom() - 10.0 - widgets::BUTTON_H,
        )
    }
}

enum Popup {
    None,
    Sortie(SortieDialog),
    Confirm(Command, ConfirmDialog),
}

const MENU_X: f32 = 10.0;
const MENU_W: f32 = 112.0;

/// The battle / army panel right of the command menu on a `canvas` sized canvas: it takes the
/// rest of the width (10 pixels from the right edge) and ends 9 pixels above the help bar.
fn panel_rect(canvas: Vec2) -> Rect {
    let (x, y) = (130.0, TOP + 4.0);
    Rect::new(x, y, canvas.x - 10.0 - x, help_y(canvas.y) - 9.0 - y)
}

/// The camp hub. See the module docs.
pub struct CampScreen {
    title: String,
    shop: Vec<Id>,
    battle: Option<Id>,
    commands: Vec<Command>,
    menu: Menu,
    popup: Popup,
}

impl CampScreen {
    /// Camp node with heading `title`, shop list `shop` and the battle it prepares.
    pub fn new(title: &str, shop: &[Id], battle: Option<&str>) -> CampScreen {
        let commands = commands(battle.is_some());
        CampScreen {
            title: title.to_string(),
            shop: shop.to_vec(),
            battle: battle.map(str::to_string),
            commands,
            menu: Menu::new(Vec::new()),
            popup: Popup::None,
        }
    }

    fn heading(&self, pack: &Pack) -> String {
        if !self.title.is_empty() {
            return self.title.clone();
        }
        match self.battle_def(pack) {
            Some(def) => format!("{} 준비", def.name),
            None => "출진 준비".to_string(),
        }
    }

    fn battle_def<'a>(&self, pack: &'a Pack) -> Option<&'a BattleDef> {
        self.battle.as_deref().and_then(|b| pack.battles.get(b))
    }

    fn rebuild(&mut self) {
        let has_battle = self.battle.is_some();
        let items = self
            .commands
            .iter()
            .map(|c| MenuItem::new(c.label(has_battle)))
            .collect();
        let cursor = self.menu.cursor();
        let mut menu = Menu::new(items)
            .at(MENU_X, TOP + 4.0, MENU_W)
            .cancellable(false);
        menu.set_cursor(cursor);
        self.menu = menu;
    }

    /// Current deployment for this camp's battle, normalised to its rules.
    fn deployment(&self, pack: &Pack, campaign: &CampaignState) -> Vec<Id> {
        match self.battle_def(pack) {
            Some(def) => normalize_deployment(pack, def, campaign, &campaign.deployed),
            None => Vec::new(),
        }
    }

    fn help(&self, command: Command) -> String {
        match command {
            Command::Sortie if self.battle.is_some() => {
                "편성을 확인하고 전투에 나섭니다.".to_string()
            }
            Command::Sortie => "준비를 마치고 다음으로 진행합니다.".to_string(),
            Command::Deploy => "이번 전투에 출진할 무장을 고릅니다.".to_string(),
            Command::Equip => "무기·병법서·보물을 장비하거나 해제합니다.".to_string(),
            Command::Shop if self.shop.is_empty() => {
                "가진 물건을 팝니다. 이곳에서는 파는 물건이 없습니다.".to_string()
            }
            Command::Shop => "물건을 사고팝니다.".to_string(),
            Command::Tools => "승급·병과 변경 도구를 무장에게 사용합니다.".to_string(),
            Command::Officers => "무장의 능력·책략·장비를 봅니다.".to_string(),
            Command::Save => "지금까지의 진행을 기록합니다.".to_string(),
            Command::Load => "기록을 불러옵니다.".to_string(),
            Command::QuickSave => "지금 진행을 순간 저장 칸에 기록합니다 (F5).".to_string(),
            Command::QuickLoad => "순간 저장을 불러옵니다 (F9).".to_string(),
            Command::Settings => "음량, 글자 속도 등을 바꿉니다.".to_string(),
            Command::Title => "타이틀 화면으로 돌아갑니다.".to_string(),
        }
    }

    fn activate(&mut self, ctx: &mut Ctx, command: Command) -> Transition {
        let Some(pack) = ctx.pack.clone() else {
            return Transition::None;
        };
        match command {
            Command::Sortie => {
                let Some(session) = ctx.session.as_ref() else {
                    return Transition::None;
                };
                if self.battle.is_some() {
                    let selection = self.deployment(&pack, &session.campaign);
                    if selection.is_empty() {
                        ctx.sfx(sfx::ERROR);
                        ctx.toast("출진할 무장이 없습니다. 부대를 편성하세요.");
                        return Transition::None;
                    }
                    if let Some(def) = self.battle_def(&pack) {
                        self.popup = Popup::Sortie(SortieDialog::new(&ctx.gfx, def, selection));
                    }
                } else {
                    self.popup = Popup::Confirm(
                        Command::Sortie,
                        ConfirmDialog::new(&ctx.gfx, "준비를 마치고 다음으로 진행할까요?"),
                    );
                }
                Transition::None
            }
            Command::Deploy => match &self.battle {
                Some(b) => Transition::push(DeployScreen::new(b)),
                None => Transition::None,
            },
            Command::Equip => Transition::push(equip::EquipScreen::new()),
            Command::Shop => Transition::push(shop::ShopScreen::new(&self.shop)),
            Command::Tools => Transition::push(tools::ToolsScreen::new()),
            Command::Officers => Transition::push(officers::OfficersScreen::new()),
            Command::Save => match ctx.session.as_ref() {
                Some(session) => Transition::push(SaveLoadScreen::save(session.to_save(&pack))),
                None => Transition::None,
            },
            Command::Load => Transition::push(SaveLoadScreen::load(&pack.manifest.id)),
            Command::QuickSave => Transition::QuickSave,
            Command::QuickLoad => Transition::QuickLoad,
            Command::Settings => Transition::push(SettingsScreen::new()),
            Command::Title => {
                self.popup = Popup::Confirm(
                    Command::Title,
                    ConfirmDialog::new(
                        &ctx.gfx,
                        "타이틀 화면으로 돌아갈까요?\n기록하지 않은 진행은 사라집니다.",
                    )
                    .default_no(),
                );
                Transition::None
            }
        }
    }

    fn update_popup(&mut self, ctx: &mut Ctx) -> Option<Transition> {
        match std::mem::replace(&mut self.popup, Popup::None) {
            Popup::None => None,
            Popup::Sortie(mut dialog) => {
                let (cx, y) = dialog.buttons_at();
                match dialog.buttons.update(ctx, cx, y, true) {
                    ConfirmEvent::Yes => {
                        if let Some(session) = ctx.session.as_mut() {
                            session.campaign.deployed = dialog.selection;
                        }
                        return Some(Transition::Flow(Flow::Advance));
                    }
                    ConfirmEvent::No => {}
                    ConfirmEvent::None => self.popup = Popup::Sortie(dialog),
                }
                Some(Transition::None)
            }
            Popup::Confirm(command, mut dialog) => {
                match dialog.update(ctx) {
                    ConfirmEvent::Yes => match command {
                        Command::Title => return Some(Transition::Flow(Flow::Title)),
                        _ => return Some(Transition::Flow(Flow::Advance)),
                    },
                    ConfirmEvent::No => {}
                    ConfirmEvent::None => self.popup = Popup::Confirm(command, dialog),
                }
                Some(Transition::None)
            }
        }
    }

    fn draw_battle_panel(&self, ctx: &Ctx, pack: &Pack, campaign: &CampaignState, def: &BattleDef) {
        let gfx = &ctx.gfx;
        let panel = panel_rect(gfx.size());
        let x = panel.x + 10.0;
        let w = panel.w - 20.0;
        let mut y = panel.y + 6.0;
        draw_caption(gfx, "다음 전투", x, y);
        y += 13.0;
        gfx.text(
            &def.name,
            x,
            y,
            TextStyle::main(theme::TEXT_NAME)
                .size(2)
                .shadow(theme::TEXT_SHADOW),
        );
        if !def.location.is_empty() {
            gfx.text_aligned(
                &def.location,
                x,
                y + 12.0,
                w,
                Align::Right,
                TextStyle::small(theme::TEXT_DIM),
            );
        }
        y += 34.0;
        draw_divider(x, y, w);
        y += 6.0;
        let label = TextStyle::small(theme::TEXT_DIM);
        let text = TextStyle::main(theme::TEXT).shadow(theme::TEXT_SHADOW);
        // The lord's retreat (unless the battle is fought without them), the battle's own
        // defeat conditions, running out of turns.
        let lord = campaign
            .roster
            .iter()
            .find(|o| pack.officer(&o.id).is_some_and(|d| d.lord))
            .filter(|o| !def.deploy.forbidden.contains(&o.id))
            .map(|o| format!("{} 퇴각", officer_name(pack, &o.id)))
            // Without the lord the troop is lost when it has retreated.
            .or_else(|| Some("아군 전멸".to_string()));
        let own = def.defeat.iter().map(|c| {
            crate::screens::battle::text::condition_text(c, |id| officer_name(pack, id).to_string())
        });
        let defeat = lord
            .into_iter()
            .chain(own)
            .chain([format!("{}턴 경과", def.turn_limit)])
            .collect::<Vec<_>>()
            .join(" · ");
        let mut rows = vec![("승리 조건", def.objective.clone()), ("패배 조건", defeat)];
        if def.reward_gold > 0 {
            rows.push((
                "승리 보상",
                format!("금 {}", format::thousands(def.reward_gold)),
            ));
        }
        for (k, v) in rows {
            gfx.text(k, x, y + 2.0, label);
            let lines = gfx.wrap(&v, FontId::Main, 1, w - 56.0);
            gfx.text_lines(&lines[..lines.len().min(2)], x + 56.0, y, text);
            y += 16.0 * lines.len().clamp(1, 2) as f32;
        }
        y += 2.0;
        draw_divider(x, y, w);
        y += 5.0;
        let selection = self.deployment(pack, campaign);
        draw_caption(
            gfx,
            &format!("출진 부대 {}/{}", selection.len(), deploy_max(def)),
            x,
            y,
        );
        y += 12.0;
        self.draw_officer_grid(ctx, pack, campaign, &selection, x, y, w);
    }

    /// Officers in two columns of 26-pixel rows, as many as fit above the panel's bottom.
    #[allow(clippy::too_many_arguments)]
    fn draw_officer_grid(
        &self,
        ctx: &Ctx,
        pack: &Pack,
        campaign: &CampaignState,
        ids: &[Id],
        x: f32,
        y: f32,
        w: f32,
    ) {
        let gfx = &ctx.gfx;
        let panel = panel_rect(gfx.size());
        let col_w = (w / 2.0).floor();
        let row_h = 26.0;
        let rows = ((panel.bottom() - 4.0 - y) / row_h).floor().max(1.0) as usize;
        let shown = ids.len().min(rows * 2);
        for (i, id) in ids.iter().take(shown).enumerate() {
            let Some(o) = campaign.officer(id) else {
                continue;
            };
            let (col, row) = (i / rows, i % rows);
            let px = x + col as f32 * col_w;
            let py = y + row as f32 * row_h;
            draw_officer_sprite(ctx, pack, o, vec2(px + 12.0, py + row_h - 2.0), false);
            gfx.text(
                officer_name(pack, id),
                px + 28.0,
                py + 5.0,
                TextStyle::main(theme::TEXT).shadow(theme::TEXT_SHADOW),
            );
            gfx.text_aligned(
                &format!("{} Lv{}", class_name(pack, &o.class), o.level),
                px,
                py + 7.0,
                col_w - 10.0,
                Align::Right,
                TextStyle::small(theme::TEXT_DIM),
            );
        }
        if ids.len() > shown {
            gfx.text_aligned(
                &format!("외 {}명", ids.len() - shown),
                x,
                panel.bottom() - 14.0,
                w,
                Align::Right,
                TextStyle::small(theme::TEXT_DIM),
            );
        }
    }

    fn draw_sortie(&self, ctx: &Ctx, pack: &Pack, campaign: &CampaignState, dialog: &SortieDialog) {
        let gfx = &ctx.gfx;
        let r = dialog.rect;
        fill_rect(gfx.screen(), Color::new(0.0, 0.0, 0.0, 0.45));
        draw_window(r);
        let x = r.x + 12.0;
        let w = r.w - 24.0;
        let mut y = r.y + 8.0;
        gfx.text_aligned(
            "출진하시겠습니까?",
            r.x,
            y,
            r.w,
            Align::Center,
            TextStyle::main(theme::TEXT_ACCENT).shadow(theme::TEXT_SHADOW),
        );
        y += 20.0;
        if let Some(def) = self.battle_def(pack) {
            gfx.text(
                &def.name,
                x,
                y,
                TextStyle::main(theme::TEXT_NAME).shadow(theme::TEXT_SHADOW),
            );
            gfx.text_aligned(
                &format!("출진 {}/{}명", dialog.selection.len(), deploy_max(def)),
                x,
                y + 2.0,
                w,
                Align::Right,
                TextStyle::small(theme::TEXT_DIM),
            );
        }
        y += 17.0;
        gfx.text_lines(
            &dialog.objective,
            x,
            y,
            TextStyle::main(theme::TEXT).shadow(theme::TEXT_SHADOW),
        );
        y += 16.0 * dialog.objective.len() as f32 + 3.0;
        draw_divider(x, y, w);
        y += 5.0;
        // Deployed officers, in slot order, centred row by row.
        let per_row = SortieDialog::per_row();
        for (row, chunk) in dialog.selection.chunks(per_row).enumerate() {
            let row_w = chunk.len() as f32 * CELL.x;
            let x0 = r.x + ((r.w - row_w) / 2.0).round();
            let top = y + row as f32 * CELL.y;
            for (i, id) in chunk.iter().enumerate() {
                let cx = x0 + i as f32 * CELL.x + CELL.x / 2.0;
                if let Some(o) = campaign.officer(id) {
                    draw_officer_sprite(ctx, pack, o, vec2(cx, top + 24.0), true);
                }
                gfx.text_aligned(
                    officer_name(pack, id),
                    cx - CELL.x / 2.0,
                    top + 25.0,
                    CELL.x,
                    Align::Center,
                    TextStyle::small(theme::TEXT).shadow(theme::TEXT_SHADOW),
                );
            }
        }
        let (cx, by) = dialog.buttons_at();
        dialog.buttons.draw(ctx, cx, by);
    }
}

impl Screen for CampScreen {
    fn in_camp_frame(&self) -> bool {
        true
    }

    fn name(&self) -> &'static str {
        "camp"
    }

    fn on_enter(&mut self, ctx: &mut Ctx, how: Enter) {
        if how == Enter::Fresh {
            ctx.audio.play_bgm(bgm::CAMP);
            // Show (and store) the deployment normalised to this battle's rules, so a list left
            // over from an earlier battle never reaches the battle engine.
            if let (Some(pack), Some(session)) = (ctx.pack.clone(), ctx.session.as_mut()) {
                if let Some(def) = self.battle_def(&pack) {
                    session.campaign.deployed = initial_selection(&pack, def, &session.campaign);
                }
            }
        }
        self.rebuild();
    }

    fn update(&mut self, ctx: &mut Ctx) -> Transition {
        if let Some(t) = self.update_popup(ctx) {
            return t;
        }
        match self.menu.update(ctx) {
            MenuEvent::Selected(i) => {
                let command = self.commands[i];
                self.activate(ctx, command)
            }
            _ => Transition::None,
        }
    }

    fn draw(&self, ctx: &Ctx) {
        let (Some(pack), Some(session)) = (ctx.pack.as_deref(), ctx.session.as_ref()) else {
            fill_rect(ctx.gfx.screen(), theme::BACKGROUND);
            return;
        };
        let campaign = &session.campaign;
        let panel = panel_rect(ctx.gfx.size());
        draw_camp_backdrop(ctx, 0.45);
        draw_header(ctx, &self.heading(pack), campaign.gold);
        self.menu.draw(ctx);

        draw_window_ex(panel, WindowStyle::Panel, 0.94);
        match self.battle_def(pack) {
            Some(def) => self.draw_battle_panel(ctx, pack, campaign, def),
            None => {
                let x = panel.x + 10.0;
                draw_caption(
                    &ctx.gfx,
                    &format!("아군 {}명", campaign.roster.len()),
                    x,
                    panel.y + 6.0,
                );
                let ids: Vec<Id> = campaign.roster.iter().map(|o| o.id.clone()).collect();
                self.draw_officer_grid(
                    ctx,
                    pack,
                    campaign,
                    &ids,
                    x,
                    panel.y + 20.0,
                    panel.w - 20.0,
                );
            }
        }
        // Play time under the menu.
        let m = self.menu.rect();
        ctx.gfx.text_aligned(
            &format!("플레이 {}", format::play_time(campaign.play_seconds)),
            m.x,
            m.bottom() + 6.0,
            m.w,
            Align::Center,
            TextStyle::small(theme::TEXT_DIM).shadow(theme::TEXT_SHADOW),
        );
        if let Some(command) = self.commands.get(self.menu.cursor()) {
            draw_help(ctx, &self.help(*command));
        }
        match &self.popup {
            Popup::None => {}
            Popup::Sortie(dialog) => self.draw_sortie(ctx, pack, campaign, dialog),
            Popup::Confirm(_, dialog) => {
                fill_rect(ctx.gfx.screen(), Color::new(0.0, 0.0, 0.0, 0.4));
                dialog.draw(ctx);
            }
        }
    }
}

/// The base data pack, for tests of the camp and drama screens.
#[cfg(test)]
pub(crate) fn test_pack() -> Pack {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/base");
    Pack::load(&hero_core::pack::DirSource { root }).expect("the base pack loads")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_depends_on_the_battle() {
        let with = commands(true);
        assert_eq!(with[0], Command::Sortie);
        assert!(with.contains(&Command::Deploy));
        assert_eq!(Command::Sortie.label(true), "출진");
        let without = commands(false);
        assert!(!without.contains(&Command::Deploy));
        assert_eq!(Command::Sortie.label(false), "다음으로");
        assert_eq!(with.len(), without.len() + 1);
    }

    /// With a battle the menu has 12 commands; on the smallest canvas they and the play time
    /// under them still end above the help bar (ROADMAP M7-1).
    #[test]
    fn the_whole_menu_fits_the_smallest_canvas() {
        let mut camp = CampScreen::new("", &[], Some("p1_sishui"));
        assert_eq!(camp.commands.len(), 12);
        assert!(camp.commands.contains(&Command::QuickSave));
        assert!(camp.commands.contains(&Command::QuickLoad));
        camp.rebuild();
        let m = camp.menu.rect();
        let play_time_bottom = m.bottom() + 6.0 + FontId::Small.line_height();
        assert!(
            play_time_bottom <= help_y(crate::gfx::DEFAULT_CANVAS.y),
            "{play_time_bottom}"
        );
    }

    #[test]
    fn panel_fills_the_canvas_right_of_the_menu() {
        // The base pack's layout.
        assert_eq!(
            panel_rect(crate::gfx::DEFAULT_CANVAS),
            Rect::new(130.0, 30.0, 340.0, 214.0)
        );
        let r = panel_rect(vec2(640.0, 480.0));
        assert_eq!((r.x, r.right()), (130.0, 630.0));
        assert_eq!(r.bottom(), help_y(480.0) - 9.0);
    }

    #[test]
    fn headings() {
        let pack = test_pack();
        let camp = CampScreen::new("", &[], Some("p1_sishui"));
        assert_eq!(camp.heading(&pack), "사수관 전투 준비");
        let camp = CampScreen::new("진류", &[], None);
        assert_eq!(camp.heading(&pack), "진류");
        assert_eq!(CampScreen::new("", &[], None).heading(&pack), "출진 준비");
    }
}
