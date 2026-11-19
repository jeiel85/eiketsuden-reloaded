//! Save / load slot screen, reusable from the title screen (load) and the camp (save).
//!
//! * [`SaveLoadScreen::load`] lists every slot; choosing a filled slot offers
//!   불러오기 / 내보내기 / 삭제, and loading continues the game ([`Flow::Continue`]).
//! * [`SaveLoadScreen::save`] writes the given [`SaveGame`] into a manual slot (the autosave slot
//!   is read-only here), asking before overwriting, then returns to the previous screen.
//!
//! Deleting always asks for confirmation. The Delete key deletes the selected slot directly
//! (with confirmation). The screen pops itself on cancel.
//!
//! Two tools carry records between devices and tidy old ones, on the `T` key (the footer says so):
//! a slot's record goes to the clipboard as the JSON text it is stored as, and a record pasted
//! from the clipboard comes into the first free manual slot. "모두 지우기" clears every record of
//! the pack, the automatic and quick ones included (`saves::clear`); until this, the only way to
//! delete anything was this screen's Delete, slot by slot.
//!
//! The clipboard answers later (a browser asks the player, X11 waits for the owner), so the
//! screen starts the work and keeps drawing: [`crate::platform::clipboard`].

use crate::app::{Ctx, Enter, Screen, Transition};
use crate::audio::sfx;
use crate::flow::Flow;
use crate::gfx::{fill_rect, Align, TextStyle};
use crate::platform::clipboard::{Clipboard, Event as ClipboardEvent};
use crate::platform::unix_now;
use crate::quicksave;
use crate::saves::{self, SlotInfo, SlotStatus};
use crate::ui::dialog::{ChoiceBox, ChoiceEvent, ConfirmDialog, ConfirmEvent};
use crate::ui::format;
use crate::ui::menu::{Menu, MenuEvent, MenuItem};
use crate::ui::theme;
use crate::ui::window::{draw_title_bar, draw_window_ex, WindowStyle};
use hero_core::save::SaveGame;
use macroquad::prelude::*;

/// Key that opens the record tools (import / clear every record).
const TOOLS_KEY: KeyCode = KeyCode::T;

enum Mode {
    Load,
    Save(Box<SaveGame>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SlotAction {
    Load,
    Save,
    /// Put the slot's record on the clipboard ([`saves::export_text`]).
    Export,
    Delete,
    Cancel,
}

impl SlotAction {
    fn label(self) -> &'static str {
        match self {
            SlotAction::Load => "불러오기",
            SlotAction::Save => "여기에 기록",
            SlotAction::Export => "클립보드로 내보내기",
            SlotAction::Delete => "삭제",
            SlotAction::Cancel => "취소",
        }
    }
}

/// What the tools box (and its confirmations) can do beyond one slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolAction {
    /// Read a record from the clipboard and put it in a manual slot.
    Import,
    /// Remove every record of the pack.
    ClearAll,
    Cancel,
}

impl ToolAction {
    fn label(self) -> &'static str {
        match self {
            ToolAction::Import => "클립보드에서 가져오기",
            ToolAction::ClearAll => "기록 모두 지우기",
            ToolAction::Cancel => "취소",
        }
    }
}

/// A clipboard operation the screen started, waiting for its answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pending {
    /// The record of this slot is being copied.
    Export(saves::SaveSlot),
    /// A record is being read for [`ToolAction::Import`].
    Import,
}

enum Popup {
    None,
    Actions {
        slot: usize,
        actions: Vec<SlotAction>,
        choice: ChoiceBox,
    },
    Tools {
        actions: Vec<ToolAction>,
        choice: ChoiceBox,
    },
    Confirm {
        slot: usize,
        action: SlotAction,
        dialog: ConfirmDialog,
    },
    /// A pasted record, about to replace the record in `slot` (the confirmation).
    Import {
        slot: usize,
        imported: Box<saves::Imported>,
        dialog: ConfirmDialog,
    },
    /// "Remove every record?" and how many there are.
    ClearAll {
        count: usize,
        dialog: ConfirmDialog,
    },
}

/// Left edge and top of the slot list, below the title bar; it spans the canvas width with
/// this margin on both sides.
const LIST_MARGIN: f32 = 12.0;
const LIST_TOP: f32 = 28.0;
/// Space kept free below the slot details for the key help line.
const FOOTER_H: f32 = 18.0;

pub struct SaveLoadScreen {
    mode: Mode,
    pack_id: String,
    slots: Vec<SlotInfo>,
    menu: Menu,
    popup: Popup,
    now: u64,
    clipboard: Clipboard,
    pending: Option<Pending>,
    /// When the pending clipboard request started ([`get_time`]), for the timeout.
    pending_since: f64,
}

impl SaveLoadScreen {
    /// Pick a save to load (for pack `pack_id`).
    pub fn load(pack_id: &str) -> SaveLoadScreen {
        SaveLoadScreen::new(Mode::Load, pack_id)
    }

    /// Pick a slot to write `save` into.
    pub fn save(save: SaveGame) -> SaveLoadScreen {
        let pack_id = save.pack_id.clone();
        SaveLoadScreen::new(Mode::Save(Box::new(save)), &pack_id)
    }

    fn new(mode: Mode, pack_id: &str) -> SaveLoadScreen {
        SaveLoadScreen {
            mode,
            pack_id: pack_id.to_string(),
            slots: Vec::new(),
            menu: Menu::new(Vec::new()),
            popup: Popup::None,
            now: 0,
            clipboard: Clipboard::new(),
            pending: None,
            pending_since: 0.0,
        }
    }

    fn saving(&self) -> bool {
        matches!(self.mode, Mode::Save(_))
    }

    fn refresh(&mut self, ctx: &Ctx) {
        self.now = unix_now();
        self.slots = saves::list(ctx.storage.as_ref(), &self.pack_id);
        let saving = self.saving();
        let items = self
            .slots
            .iter()
            .map(|info| {
                let (text, detail) = match &info.status {
                    SlotStatus::Empty => ("— 비어 있음 —".to_string(), String::new()),
                    SlotStatus::Ready(s) => {
                        (s.label.clone(), format::relative_time(s.saved_at, self.now))
                    }
                    SlotStatus::Unreadable(_) => ("(읽을 수 없는 기록)".to_string(), String::new()),
                };
                let enabled = match (&info.status, info.slot) {
                    (_, slot) if saving && slot.is_system() => false,
                    (SlotStatus::Empty, _) => saving,
                    _ => true,
                };
                let item = MenuItem::new(text).tag(info.slot.name()).enabled(enabled);
                if detail.is_empty() {
                    item
                } else {
                    item.detail(detail)
                }
            })
            .collect();
        let first_refresh = self.menu.items.is_empty();
        let cursor = self.menu.cursor();
        // A new menu starts on the first enabled slot: the first loadable save when loading,
        // the first manual slot when saving.
        let list_w = ctx.gfx.size().x - 2.0 * LIST_MARGIN;
        let mut menu = Menu::new(items).at(LIST_MARGIN, LIST_TOP, list_w);
        menu.wrap = false;
        menu.tag_width = 64.0;
        // After an action keep the cursor on the same slot while it is still selectable.
        if !first_refresh && menu.items.get(cursor).is_some_and(|it| it.enabled) {
            menu.set_cursor(cursor);
        }
        self.menu = menu;
    }

    fn open_actions(&mut self, ctx: &mut Ctx, index: usize) {
        let info = &self.slots[index];
        let mut actions = Vec::new();
        match (&self.mode, &info.status) {
            (Mode::Load, SlotStatus::Ready(_)) => actions.push(SlotAction::Load),
            (Mode::Save(_), _) if !info.slot.is_system() => actions.push(SlotAction::Save),
            _ => {}
        }
        if info.status != SlotStatus::Empty {
            // Even a record this game cannot open can be carried to another device (or a
            // newer version of this one): it travels as the text it was stored as.
            actions.push(SlotAction::Export);
            actions.push(SlotAction::Delete);
        }
        actions.push(SlotAction::Cancel);
        let labels: Vec<&str> = actions.iter().map(|a| a.label()).collect();
        let prompt = match &info.status {
            SlotStatus::Unreadable(why) => format!("{} — {why}", info.slot.name()),
            _ => info.slot.name(),
        };
        let cancel = actions.len() - 1;
        let choice = ChoiceBox::new(&ctx.gfx, Some(&prompt), &labels, Some(cancel));
        self.popup = Popup::Actions {
            slot: index,
            actions,
            choice,
        };
    }

    fn confirm(&mut self, ctx: &Ctx, slot: usize, action: SlotAction) {
        let name = self.slots[slot].slot.name();
        let dialog = match action {
            SlotAction::Delete => {
                ConfirmDialog::new(&ctx.gfx, &format!("{name}을(를) 삭제할까요?")).default_no()
            }
            _ => ConfirmDialog::new(&ctx.gfx, &format!("{name}에 덮어쓸까요?")).default_no(),
        };
        self.popup = Popup::Confirm {
            slot,
            action,
            dialog,
        };
    }

    /// Carry out an action on slot `index`.
    fn perform(&mut self, ctx: &mut Ctx, index: usize, action: SlotAction) -> Transition {
        let slot = self.slots[index].slot;
        match action {
            SlotAction::Load => match saves::read(ctx.storage.as_ref(), slot, &self.pack_id) {
                Ok(save) => match ctx.pack.as_deref().map(|p| quicksave::playable(p, &save)) {
                    Some(Err(why)) => {
                        ctx.sfx(sfx::ERROR);
                        ctx.toast(why);
                    }
                    _ => return Transition::Flow(Flow::Continue(Box::new(save))),
                },
                Err(e) => {
                    ctx.sfx(sfx::ERROR);
                    ctx.toast(e.to_string());
                }
            },
            SlotAction::Save => {
                if let Mode::Save(save) = &self.mode {
                    let mut save = (**save).clone();
                    save.saved_at = unix_now();
                    match saves::write(ctx.storage.as_mut(), slot, &save) {
                        Ok(()) => {
                            ctx.toast(format!("{}에 기록했습니다.", slot.name()));
                            return Transition::Pop;
                        }
                        Err(e) => {
                            ctx.sfx(sfx::ERROR);
                            ctx.toast(format!("기록하지 못했습니다: {e}"));
                        }
                    }
                }
            }
            SlotAction::Export => {
                match saves::export_text(ctx.storage.as_ref(), slot, &self.pack_id) {
                    Ok(text) => match self.clipboard.copy(&text) {
                        Ok(()) => self.expect(Pending::Export(slot)),
                        Err(why) => {
                            ctx.sfx(sfx::ERROR);
                            ctx.toast(format!("내보내지 못했습니다: {why}"));
                        }
                    },
                    Err(e) => {
                        ctx.sfx(sfx::ERROR);
                        ctx.toast(format!("내보내지 못했습니다: {e}"));
                    }
                }
            }
            SlotAction::Delete => match saves::delete(ctx.storage.as_mut(), slot, &self.pack_id) {
                Ok(()) => ctx.toast(format!("{}을(를) 삭제했습니다.", slot.name())),
                Err(e) => {
                    ctx.sfx(sfx::ERROR);
                    ctx.toast(format!("삭제하지 못했습니다: {e}"));
                }
            },
            SlotAction::Cancel => {}
        }
        self.refresh(ctx);
        Transition::None
    }

    /// The tools box (`T`): import a record, clear every record.
    fn open_tools(&mut self, ctx: &mut Ctx) {
        let mut actions = vec![ToolAction::Import];
        if saves::record_count(ctx.storage.as_ref(), &self.pack_id) > 0 {
            actions.push(ToolAction::ClearAll);
        }
        actions.push(ToolAction::Cancel);
        let labels: Vec<&str> = actions.iter().map(|a| a.label()).collect();
        let cancel = actions.len() - 1;
        let choice = ChoiceBox::new(&ctx.gfx, Some("기록 도구"), &labels, Some(cancel));
        self.popup = Popup::Tools { actions, choice };
    }

    /// Start reading a record from the clipboard ([`ToolAction::Import`]); the answer arrives
    /// from [`SaveLoadScreen::poll_clipboard`].
    fn start_import(&mut self, ctx: &mut Ctx) {
        match self.clipboard.paste() {
            Ok(()) => self.expect(Pending::Import),
            Err(why) => {
                ctx.sfx(sfx::ERROR);
                ctx.toast(why);
            }
        }
    }

    /// Remember what the running clipboard request is for and when it started, so a browser
    /// that never answers (an unfocused tab can leave the promise pending) is given up on
    /// instead of leaving the screen waiting forever.
    fn expect(&mut self, pending: Pending) {
        self.pending = Some(pending);
        self.pending_since = get_time();
    }

    /// How long a clipboard request may stay unanswered, in seconds: a browser permission
    /// prompt left open, or a tab that lost focus mid-request, can leave the promise pending
    /// forever. The screen gives up and says so; a late answer is ignored (the page numbers its
    /// requests, so an old one cannot be taken for a new one).
    const CLIPBOARD_TIMEOUT: f64 = 60.0;

    /// The clipboard answer the screen was waiting for. No-op while none is pending.
    fn poll_clipboard(&mut self, ctx: &mut Ctx) {
        if let Some(event) = self.clipboard.poll() {
            self.pending = None;
            self.take_event(ctx, event);
            return;
        }
        if self.pending.is_some() && get_time() - self.pending_since > Self::CLIPBOARD_TIMEOUT {
            self.clipboard.abandon();
            self.pending = None;
            ctx.sfx(sfx::ERROR);
            ctx.toast("클립보드가 응답하지 않습니다. 다시 시도해 주세요.");
        }
    }

    /// Carry out what a settled clipboard operation reported.
    fn take_event(&mut self, ctx: &mut Ctx, event: ClipboardEvent) {
        match event {
            ClipboardEvent::Copied(Ok(())) => {
                ctx.sfx(sfx::CONFIRM);
                ctx.toast("클립보드로 내보냈습니다.");
            }
            ClipboardEvent::Copied(Err(why)) => {
                ctx.sfx(sfx::ERROR);
                ctx.toast(format!("내보내지 못했습니다: {why}"));
            }
            ClipboardEvent::Pasted(Ok(None)) => {
                ctx.sfx(sfx::ERROR);
                ctx.toast("클립보드에 기록이 없습니다.");
            }
            ClipboardEvent::Pasted(Err(why)) => {
                ctx.sfx(sfx::ERROR);
                ctx.toast(format!("가져오지 못했습니다: {why}"));
            }
            ClipboardEvent::Pasted(Ok(Some(text))) => self.take_import(ctx, &text),
        }
    }

    /// Put the pasted record `text` into a manual slot: straight into the first empty one, or
    /// after a confirmation when every manual slot holds something.
    fn take_import(&mut self, ctx: &mut Ctx, text: &str) {
        let imported = match saves::import_text(text, &self.pack_id, &self.slots) {
            Ok(imported) => imported,
            Err(why) => {
                ctx.sfx(sfx::ERROR);
                ctx.toast(why.to_string());
                return;
            }
        };
        let Some(index) = self.slots.iter().position(|i| i.slot == imported.slot) else {
            ctx.sfx(sfx::ERROR);
            ctx.toast("가져온 기록을 넣을 칸이 없습니다.");
            return;
        };
        if self.slots[index].status == SlotStatus::Empty {
            self.write_import(ctx, index, &imported);
            return;
        }
        let name = imported.slot.name();
        let dialog = ConfirmDialog::new(
            &ctx.gfx,
            &format!("모든 기록 칸이 차 있습니다. {name}에 덮어쓸까요?"),
        )
        .default_no();
        self.popup = Popup::Import {
            slot: index,
            imported: Box::new(imported),
            dialog,
        };
    }

    /// Write a pasted record into slot `index` and report it.
    fn write_import(&mut self, ctx: &mut Ctx, index: usize, imported: &saves::Imported) {
        let slot = self.slots[index].slot;
        match saves::write(ctx.storage.as_mut(), slot, &imported.save) {
            Ok(()) => {
                ctx.sfx(sfx::CONFIRM);
                ctx.toast(format!(
                    "{}에 기록을 가져왔습니다: {}",
                    slot.name(),
                    imported.save.label
                ));
                self.refresh(ctx);
            }
            Err(e) => {
                ctx.sfx(sfx::ERROR);
                ctx.toast(format!("가져오지 못했습니다: {e}"));
            }
        }
    }

    /// Remove every record of the pack, after asking.
    fn clear_all(&mut self, ctx: &mut Ctx) {
        match saves::clear(ctx.storage.as_mut(), &self.pack_id) {
            Ok(0) => ctx.toast("지울 기록이 없습니다."),
            Ok(n) => {
                ctx.sfx(sfx::CONFIRM);
                ctx.toast(format!("기록 {n}개를 지웠습니다."));
                self.refresh(ctx);
            }
            Err(e) => {
                ctx.sfx(sfx::ERROR);
                ctx.toast(format!("지우지 못했습니다: {e}"));
                self.refresh(ctx);
            }
        }
    }

    fn update_popup(&mut self, ctx: &mut Ctx) -> Option<Transition> {
        match std::mem::replace(&mut self.popup, Popup::None) {
            Popup::None => None,
            Popup::Actions {
                slot,
                actions,
                mut choice,
            } => {
                match choice.update(ctx) {
                    ChoiceEvent::Chosen(i) => match actions[i] {
                        SlotAction::Cancel => {}
                        SlotAction::Delete => self.confirm(ctx, slot, SlotAction::Delete),
                        SlotAction::Save if self.slots[slot].status != SlotStatus::Empty => {
                            self.confirm(ctx, slot, SlotAction::Save)
                        }
                        action => return Some(self.perform(ctx, slot, action)),
                    },
                    ChoiceEvent::Cancelled => {}
                    ChoiceEvent::None => {
                        self.popup = Popup::Actions {
                            slot,
                            actions,
                            choice,
                        }
                    }
                }
                Some(Transition::None)
            }
            Popup::Confirm {
                slot,
                action,
                mut dialog,
            } => {
                match dialog.update(ctx) {
                    ConfirmEvent::Yes => return Some(self.perform(ctx, slot, action)),
                    ConfirmEvent::No => {}
                    ConfirmEvent::None => {
                        self.popup = Popup::Confirm {
                            slot,
                            action,
                            dialog,
                        }
                    }
                }
                Some(Transition::None)
            }
            Popup::Tools {
                actions,
                mut choice,
            } => {
                match choice.update(ctx) {
                    ChoiceEvent::Chosen(i) => match actions[i] {
                        ToolAction::Import => self.start_import(ctx),
                        ToolAction::ClearAll => {
                            let count = saves::record_count(ctx.storage.as_ref(), &self.pack_id);
                            let dialog = ConfirmDialog::new(
                                &ctx.gfx,
                                &format!(
                                    "이 팩의 기록 {count}개를 모두 지울까요? \
                                     자동 기록과 순간 저장도 함께 지워집니다."
                                ),
                            )
                            .default_no()
                            .labels("모두 지우기", "취소");
                            self.popup = Popup::ClearAll { count, dialog };
                        }
                        ToolAction::Cancel => {}
                    },
                    ChoiceEvent::Cancelled => {}
                    ChoiceEvent::None => {
                        self.popup = Popup::Tools { actions, choice };
                    }
                }
                Some(Transition::None)
            }
            Popup::Import {
                slot,
                imported,
                mut dialog,
            } => {
                match dialog.update(ctx) {
                    ConfirmEvent::Yes => self.write_import(ctx, slot, &imported),
                    ConfirmEvent::No => {}
                    ConfirmEvent::None => {
                        self.popup = Popup::Import {
                            slot,
                            imported,
                            dialog,
                        }
                    }
                }
                Some(Transition::None)
            }
            Popup::ClearAll { count, mut dialog } => {
                match dialog.update(ctx) {
                    ConfirmEvent::Yes => self.clear_all(ctx),
                    ConfirmEvent::No => {}
                    ConfirmEvent::None => self.popup = Popup::ClearAll { count, dialog },
                }
                Some(Transition::None)
            }
        }
    }
}

impl Screen for SaveLoadScreen {
    fn name(&self) -> &'static str {
        "saveload"
    }

    fn on_enter(&mut self, ctx: &mut Ctx, _how: Enter) {
        self.refresh(ctx);
    }

    fn update(&mut self, ctx: &mut Ctx) -> Transition {
        // A clipboard answer is not a key press: it can arrive while a popup is open.
        self.poll_clipboard(ctx);
        if let Some(t) = self.update_popup(ctx) {
            return t;
        }
        if ctx.input.key_pressed(TOOLS_KEY) {
            self.open_tools(ctx);
            return Transition::None;
        }
        if ctx.input.key_pressed(KeyCode::Delete) {
            let i = self.menu.cursor();
            if self
                .slots
                .get(i)
                .is_some_and(|s| s.status != SlotStatus::Empty)
            {
                self.confirm(ctx, i, SlotAction::Delete);
                return Transition::None;
            }
        }
        match self.menu.update(ctx) {
            MenuEvent::Selected(i) => {
                let info = &self.slots[i];
                if self.saving() && info.status == SlotStatus::Empty {
                    return self.perform(ctx, i, SlotAction::Save);
                }
                self.open_actions(ctx, i);
                Transition::None
            }
            MenuEvent::Cancelled => Transition::Pop,
            _ => Transition::None,
        }
    }

    fn draw(&self, ctx: &Ctx) {
        let gfx = &ctx.gfx;
        let (w, h) = (gfx.size().x, gfx.size().y);
        fill_rect(gfx.screen(), theme::BACKGROUND);
        draw_title_bar(
            ctx,
            if self.saving() {
                "기록하기"
            } else {
                "불러오기"
            },
        );
        self.menu.draw(ctx);

        // Details of the selected slot.
        let list = self.menu.rect();
        let info_rect = Rect::new(
            list.x,
            list.bottom() + 6.0,
            list.w,
            h - FOOTER_H - list.bottom() - 6.0,
        );
        if info_rect.h >= 20.0 {
            draw_window_ex(info_rect, WindowStyle::Panel, 1.0);
            let x = info_rect.x + 10.0;
            let y = info_rect.y + 6.0;
            let small = TextStyle::small(theme::TEXT_DIM);
            let text = TextStyle::main(theme::TEXT).shadow(theme::TEXT_SHADOW);
            if let Some(info) = self.slots.get(self.menu.cursor()) {
                match &info.status {
                    SlotStatus::Ready(s) => {
                        gfx.text(&s.label, x, y, text);
                        let when = if s.saved_at > 0 {
                            format!(
                                "{} ({})",
                                format::relative_time(s.saved_at, self.now),
                                format::date_utc(s.saved_at)
                            )
                        } else {
                            "시각 모름".into()
                        };
                        let mut detail = format!(
                            "기록 시각 {when}   플레이 {}",
                            format::play_time(s.play_seconds)
                        );
                        if s.mid_battle {
                            detail.push_str("   전투 중");
                        }
                        if s.mid_scene {
                            detail.push_str("   장면 중");
                        }
                        gfx.text(&detail, x, y + 17.0, small);
                    }
                    SlotStatus::Empty => {
                        gfx.text(
                            "비어 있는 칸입니다.",
                            x,
                            y,
                            TextStyle::main(theme::TEXT_DIM),
                        );
                    }
                    SlotStatus::Unreadable(why) => {
                        gfx.text(
                            "읽을 수 없는 기록입니다.",
                            x,
                            y,
                            TextStyle::main(theme::TEXT_BAD),
                        );
                        let lines = gfx.wrap(why, crate::gfx::FontId::Small, 1, info_rect.w - 20.0);
                        gfx.text_lines(&lines[..lines.len().min(2)], x, y + 17.0, small);
                    }
                }
            }
        }
        // The clipboard answers later; until then the footer says what is being waited for
        // instead of the key help, so a browser permission prompt does not look like a dead key.
        // (Both at once would overlap on the 480-wide canvas.)
        let footer = match self.pending {
            Some(Pending::Export(_)) => "클립보드로 내보내는 중…".to_string(),
            Some(Pending::Import) => "클립보드에서 가져오는 중…".to_string(),
            None => "Z/Enter 선택 · X/Esc 돌아가기 · Delete 삭제 · T 기록 도구".to_string(),
        };
        let footer_style = if self.pending.is_some() {
            TextStyle::small(theme::TEXT_ACCENT)
        } else {
            TextStyle::small(theme::TEXT_DISABLED)
        };
        gfx.text_aligned(&footer, 0.0, h - 15.0, w - 8.0, Align::Right, footer_style);

        match &self.popup {
            Popup::None => {}
            Popup::Actions { choice, .. } => {
                fill_rect(gfx.screen(), Color::new(0.0, 0.0, 0.0, 0.35));
                choice.draw(ctx);
            }
            Popup::Tools { choice, .. } => {
                fill_rect(gfx.screen(), Color::new(0.0, 0.0, 0.0, 0.35));
                choice.draw(ctx);
            }
            Popup::Confirm { dialog, .. }
            | Popup::Import { dialog, .. }
            | Popup::ClearAll { dialog, .. } => {
                fill_rect(gfx.screen(), Color::new(0.0, 0.0, 0.0, 0.35));
                dialog.draw(ctx);
            }
        }
    }
}
