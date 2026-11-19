//! 원작 데이터 (native only): choose the folder of the player's own copy of the original game and
//! switch between the original mode and the base pack, without a command line (`crate::original`).
//!
//! The screen has two views:
//!
//! * **Overview** — what the original mode is, the chosen folder and its edition (the same
//!   verdict and evidence as `hero-tools original probe`), what is being played now, and the
//!   commands: play the original mode / the base pack, pick a folder, back.
//! * **Folder browser** — the drives (Windows) and folders, one level at a time. Folders that
//!   directly hold original files are marked ★; the current folder's edition is shown above the
//!   list, and "이 폴더 사용" is enabled only for an edition the original mode can play. When the
//!   folder is not an install but exactly one subfolder is (a DOSBox package), that subfolder is
//!   offered. "경로 입력…" (or Ctrl+V) takes a typed or pasted path.
//!
//! Every file system read (listing a folder, identifying an edition, testing a typed path) runs
//! on a worker thread ([`original::Background`]): an unresponsive network share can stall it
//! for a long time, and meanwhile the screen keeps drawing, shows "읽는 중…" after a moment and
//! lets Esc or a right click cancel the read.
//!
//! Choosing a folder or switching the mode saves the settings and reloads the data pack
//! ([`Flow::Reload`]): the loading screen converts the install in memory.

use crate::app::{Ctx, Enter, Screen, Transition};
use crate::audio::sfx;
use crate::flow::Flow;
use crate::gfx::{FontId, Gfx, TextStyle};
use crate::original::{self, Folder, FolderCheck, Listing, Place};
use crate::platform::memfs;
use crate::ui::menu::{Menu, MenuEvent, MenuItem};
use crate::ui::theme;
use crate::ui::window::draw_window;
use macroquad::prelude::*;
use std::path::{Path, PathBuf};

/// Left and right margin of the text and the menu.
const MARGIN: f32 = 24.0;
/// Top of the text area, below the heading.
const TEXT_TOP: f32 = 48.0;
/// Line height of the small font.
const LINE: f32 = 12.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command {
    PlayOriginal,
    PlayBase,
    Browse,
    Back,
}

/// A row of the folder browser.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Row {
    UseThis,
    /// The one subfolder holding a playable install.
    UseChild(Folder),
    Type,
    Up,
    Open(Folder),
}

enum View {
    Overview { commands: Vec<Command> },
    Browse { place: Place, rows: Vec<Row> },
}

/// A finished read, with what it was for.
enum Done {
    /// The saved folder's edition, for the overview.
    Saved(FolderCheck),
    /// Show a place, the cursor on the folder `focus` (the folder just left).
    Show(Listing, Option<PathBuf>),
    /// The folder of the typed path line.
    Typed(Result<Listing, String>),
    /// Use `dir` if it (still) holds a playable install.
    Use(PathBuf, FolderCheck),
    /// The clipboard's first line for the path line (`None`: no text), or why it could not be
    /// read.
    Pasted(Result<Option<String>, String>),
}

/// A read on a worker thread ([`original::Background`]): the file system can stall (a network
/// share), so the screen keeps drawing, and the player can cancel a read the browser waits for.
struct Busy {
    job: original::Background<Done>,
    /// The browser waits for it (input goes to cancelling it); the overview's check of the
    /// saved folder does not hold up the menu.
    blocking: bool,
    /// When it started (`ctx.time`): the note shows only for a slow read.
    since: f64,
    /// What is being read, for the note.
    what: String,
    /// Opening the browser at the saved folder: cancelling opens it without that folder (an
    /// unresponsive network share would otherwise keep the browser out of reach).
    skips_saved: bool,
}

/// Seconds a read may take before the "reading" note shows.
const BUSY_NOTE_AFTER: f64 = 0.3;

pub struct OriginalScreen {
    view: View,
    menu: Menu,
    /// Wrapped lines above the menu.
    lines: Vec<(String, Color)>,
    /// The browser was opened directly (from the error screen): cancel leaves the screen.
    browse_only: bool,
    /// The saved folder's edition, for the overview.
    saved: Option<FolderCheck>,
    /// The path being typed ("경로 입력…"), while the browser takes text instead of commands.
    typing: Option<String>,
    busy: Option<Busy>,
}

/// `text` shortened with `…` to fit `width` pixels of the main font.
fn fit(gfx: &Gfx, text: &str, width: f32) -> String {
    if gfx.text_width(text, FontId::Main, 1) <= width {
        return text.to_string();
    }
    let mut out: String = text.to_string();
    while !out.is_empty() && gfx.text_width(&format!("{out}…"), FontId::Main, 1) > width {
        out.pop();
    }
    format!("{out}…")
}

/// The paste shortcut as the player presses it.
const PASTE_KEY: &str = if cfg!(target_os = "macos") {
    "Cmd+V"
} else {
    "Ctrl+V"
};

/// `text` with its start cut off (`…`) to fit `width` pixels of the main font: the end of a path
/// is the part being typed. Measures each character once, from the end.
fn fit_tail(gfx: &Gfx, text: &str, width: f32) -> String {
    if gfx.text_width(text, FontId::Main, 1) <= width {
        return text.to_string();
    }
    let room = width - gfx.text_width("…", FontId::Main, 1);
    let mut used = 0.0;
    let mut start = text.len();
    for (i, c) in text.char_indices().rev() {
        used += gfx.text_width(c.encode_utf8(&mut [0; 4]), FontId::Main, 1);
        if used > room {
            break;
        }
        start = i;
    }
    format!("…{}", &text[start..])
}

/// Ctrl or Cmd is held (the paste shortcut).
fn ctrl_down(ctx: &Ctx) -> bool {
    [
        KeyCode::LeftControl,
        KeyCode::RightControl,
        KeyCode::LeftSuper,
        KeyCode::RightSuper,
    ]
    .iter()
    .any(|&k| ctx.input.key_down(k))
}

/// Cmd (Super) is held: macOS types the letter of a Cmd shortcut, Windows and X11 send a control
/// character for Ctrl shortcuts (and AltGr, which Windows reports with a Ctrl, types normally).
fn cmd_down(ctx: &Ctx) -> bool {
    ctx.input.key_down(KeyCode::LeftSuper) || ctx.input.key_down(KeyCode::RightSuper)
}

/// The first line of `text`, `None` when it has none.
fn first_line(text: &str) -> Option<String> {
    text.lines()
        .next()
        .map(str::to_string)
        .filter(|s| !s.trim().is_empty())
}

/// [`read_clipboard`] runs on a worker thread ([`OriginalScreen::paste`]). Defined with it: the
/// Linux read must not run on the main thread and miniquad's must not run on another.
#[cfg(target_os = "linux")]
const READS_IN_BACKGROUND: bool = true;
#[cfg(not(target_os = "linux"))]
const READS_IN_BACKGROUND: bool = false;

/// The first line of the clipboard, `None` when it holds no text.
///
/// [`crate::platform::clipboard::read_blocking`] takes the Linux read off this thread (the X11
/// clipboard answers as slowly as its owner does) and miniquad's read stays on it
/// (docs/DECISIONS.md D14); [`OriginalScreen::paste`] decides which thread that is.
fn read_clipboard() -> Result<Option<String>, String> {
    crate::platform::clipboard::read_blocking().map(|text| text.and_then(|t| first_line(&t)))
}

/// Forget the characters typed before (macroquad keeps them until they are read).
fn drain_chars() {
    while get_char_pressed().is_some() {}
}

/// `text` cut to [`original::MAX_TYPED`] characters.
fn capped(mut text: String) -> String {
    if let Some((i, _)) = text.char_indices().nth(original::MAX_TYPED) {
        text.truncate(i);
    }
    text
}

/// The first folder row after `cursor` (wrapping around) whose name starts with `c`, ignoring
/// case. Letters bound to confirm and cancel do not jump.
fn next_starting_with(rows: &[Row], cursor: usize, c: char) -> Option<usize> {
    // Z / X / Space are the game's confirm and cancel keys.
    if c.is_control() || c.is_whitespace() || matches!(c, 'z' | 'Z' | 'x' | 'X') {
        return None;
    }
    let lower: String = c.to_lowercase().collect();
    let n = rows.len();
    (1..=n).map(|k| (cursor + k) % n).find(|&i| match &rows[i] {
        Row::Open(f) => f.name.to_lowercase().starts_with(&lower),
        _ => false,
    })
}

/// How the browser names a place.
fn place_name(place: &Place) -> String {
    match place {
        Place::Roots => "드라이브".to_string(),
        Place::Dir(dir) => dir.display().to_string(),
    }
}

impl OriginalScreen {
    /// The overview.
    pub fn new() -> OriginalScreen {
        OriginalScreen {
            view: View::Overview {
                commands: Vec::new(),
            },
            menu: Menu::new(Vec::new()),
            lines: Vec::new(),
            browse_only: false,
            saved: None,
            typing: None,
            busy: None,
        }
    }

    /// Straight to the folder browser; leaving it leaves the screen.
    pub fn browse() -> OriginalScreen {
        OriginalScreen {
            browse_only: true,
            ..OriginalScreen::new()
        }
    }

    fn wrap_into(&mut self, gfx: &Gfx, text: &str, color: Color) {
        let width = gfx.size().x - 2.0 * MARGIN - 8.0;
        for line in gfx.wrap(text, FontId::Small, 1, width) {
            self.lines.push((line, color));
        }
    }

    /// Place the menu below the text, as wide as the canvas allows, `rows` rows at most.
    fn place_menu(&mut self, gfx: &Gfx, mut menu: Menu) {
        let canvas = gfx.size();
        let top = TEXT_TOP + self.lines.len() as f32 * LINE + 12.0;
        let room = (canvas.y - 8.0 - top - 2.0 * theme::PADDING).max(theme::ROW_HEIGHT);
        let rows = ((room / theme::ROW_HEIGHT) as usize).max(1);
        menu.visible_rows = rows;
        menu.set_position(MARGIN, top);
        menu.set_width(canvas.x - 2.0 * MARGIN);
        let cursor = menu.cursor();
        menu.set_cursor(cursor);
        self.menu = menu;
    }

    /// Start `work` on a worker thread; its result comes back through [`Self::finish`]. A read
    /// started while another runs replaces it (the old one is dropped).
    fn start(
        &mut self,
        ctx: &Ctx,
        blocking: bool,
        what: String,
        work: impl FnOnce() -> Done + Send + 'static,
    ) {
        self.busy = Some(Busy {
            job: original::Background::spawn(work),
            blocking,
            since: ctx.time,
            what,
            skips_saved: false,
        });
    }

    /// Paste the clipboard's first line at the end of the path line (a new line when none is
    /// being typed). Linux reads it in the background ([`read_clipboard`]).
    fn paste(&mut self, ctx: &mut Ctx) {
        if READS_IN_BACKGROUND {
            self.start(ctx, true, "클립보드".into(), || {
                Done::Pasted(read_clipboard())
            });
        } else {
            self.finish(ctx, Ok(Done::Pasted(read_clipboard())));
        }
    }

    /// Read `place` and show it, the cursor on `focus` when it is listed.
    fn open_place(&mut self, ctx: &Ctx, place: Place, focus: Option<&Path>) {
        let what = place_name(&place);
        let focus = focus.map(Path::to_path_buf);
        self.start(ctx, true, what, move || {
            Done::Show(original::look(place), focus)
        });
    }

    /// Open the browser where [`original::start_place`] says (it tests the saved path too).
    /// With `use_saved` it starts at the saved folder (or its nearest existing parent);
    /// without, at the home folder.
    fn open_browser(&mut self, ctx: &Ctx, use_saved: bool) {
        // Letters typed on earlier screens would jump through the list.
        drain_chars();
        let saved = ctx.settings.original_dir.clone().filter(|_| use_saved);
        let skips_saved = saved.is_some();
        let what = saved.clone().unwrap_or_else(|| "시작 위치".to_string());
        self.start(ctx, true, what, move || {
            Done::Show(
                original::look(original::start_place(saved.as_deref())),
                None,
            )
        });
        if let Some(busy) = &mut self.busy {
            busy.skips_saved = skips_saved;
        }
    }

    /// The overview; the saved folder's edition is read in the background.
    fn show_overview(&mut self, ctx: &Ctx) {
        self.saved = None;
        self.busy = None;
        if let Some(dir) = ctx.settings.original_dir.clone() {
            let what = dir.clone();
            self.start(ctx, false, what, move || {
                Done::Saved(original::check_folder(Path::new(&dir)))
            });
        }
        self.render_overview(ctx);
    }

    fn render_overview(&mut self, ctx: &Ctx) {
        let gfx = &ctx.gfx;
        let saved_dir = ctx.settings.original_dir.clone();
        let playing = memfs::mounted().is_some();
        self.lines.clear();
        self.wrap_into(
            gfx,
            "가지고 있는 원작(삼국지 영걸전 한국어 DOS/V판·중국어 DOS판)의 설치 폴더를 고르면, 원작의 \
             얼굴·유닛·지형·전투 맵 그림으로 플레이합니다. 원작 파일은 읽기만 하고, 변환한 그림은 \
             저장하지 않고 실행할 때마다 다시 만듭니다. 아직 변환하지 못한 것은 기본 팩의 것을 씁니다.",
            theme::TEXT_DIM,
        );
        self.lines.push((String::new(), theme::TEXT));
        match (&saved_dir, self.saved.clone()) {
            (Some(dir), Some(check)) => {
                self.wrap_into(gfx, &format!("원작 폴더: {dir}"), theme::TEXT);
                let color = if check.is_supported() {
                    theme::TEXT_GOOD
                } else {
                    theme::TEXT_BAD
                };
                self.wrap_into(gfx, &check.summary(), color);
            }
            (Some(dir), None) => {
                self.wrap_into(gfx, &format!("원작 폴더: {dir}"), theme::TEXT);
                let note = if self.busy.is_some() {
                    "판본을 확인하는 중…"
                } else {
                    "판본을 확인하지 못했습니다"
                };
                self.wrap_into(gfx, note, theme::TEXT_DIM);
            }
            (None, _) => self.wrap_into(gfx, "원작 폴더: 아직 고르지 않았습니다", theme::TEXT),
        }
        let now = if playing {
            "지금: 원작 모드로 플레이하고 있습니다"
        } else if crate::platform::explicit_data(&ctx.options) {
            "지금: --data로 지정한 팩으로 플레이하고 있습니다(원작 모드 설정은 적용되지 않습니다)"
        } else {
            "지금: 기본 팩으로 플레이하고 있습니다"
        };
        self.wrap_into(gfx, now, theme::TEXT_ACCENT);

        let supported = self.saved.as_ref().is_some_and(FolderCheck::is_supported);
        let mut commands = Vec::new();
        commands.push(if playing {
            Command::PlayBase
        } else {
            Command::PlayOriginal
        });
        commands.push(Command::Browse);
        commands.push(Command::Back);
        let items = commands
            .iter()
            .map(|c| match c {
                Command::PlayOriginal => MenuItem::new("원작 모드로 플레이").enabled(supported),
                Command::PlayBase => MenuItem::new("기본 팩으로 플레이"),
                Command::Browse => MenuItem::new("원작 폴더 고르기…"),
                Command::Back => MenuItem::new("돌아가기"),
            })
            .collect();
        self.view = View::Overview { commands };
        self.place_menu(gfx, Menu::new(items));
    }

    /// Show a read place, the cursor on the folder `focus` when it is listed (the folder just
    /// left when going up).
    fn show_place(&mut self, ctx: &Ctx, listing: Listing, focus: Option<&Path>) {
        let gfx = &ctx.gfx;
        let Listing {
            place,
            folders,
            check,
            child,
        } = listing;
        let (folders, error) = match folders {
            Ok(f) => (f, None),
            Err(e) => (Vec::new(), Some(e)),
        };
        let all_checked = original::all_checked(&folders);
        self.lines.clear();
        self.wrap_into(gfx, &format!("위치: {}", place_name(&place)), theme::TEXT);
        match (&check, &error) {
            (_, Some(e)) => self.wrap_into(gfx, &format!("이 폴더를 읽을 수 없습니다: {e}"), theme::TEXT_BAD),
            (Some(c), None) => {
                let color = if c.is_supported() {
                    theme::TEXT_GOOD
                } else {
                    theme::TEXT_BAD
                };
                self.wrap_into(gfx, &c.summary(), color);
                // The evidence, as `hero-tools original probe` prints it.
                for e in c.evidence() {
                    self.wrap_into(gfx, &format!("· {e}"), theme::TEXT_DIM);
                }
            }
            (None, None) => match &child {
                Some(f) => self.wrap_into(
                    gfx,
                    &format!(
                        "원작 파일은 {0} 폴더 안에 있습니다. \"{0} 폴더 사용\"을 고르세요.",
                        f.name
                    ),
                    theme::TEXT_GOOD,
                ),
                None => self.wrap_into(
                    gfx,
                    &format!(
                        "원작 파일(DISK1.R3I, HEXZMAP.R3 등)이 있는 폴더로 들어가세요. ★는 원작 파일이 \
                         있는 폴더입니다.{} 경로를 알면 \"경로 입력…\"이나 {PASTE_KEY}로 붙여 넣을 \
                         수 있습니다.",
                        if all_checked {
                            String::new()
                        } else {
                            format!(
                                " 폴더가 많아 이름순 앞 {}개에만 ★를 표시합니다.",
                                original::MAX_CHECKED
                            )
                        }
                    ),
                    theme::TEXT_DIM,
                ),
            },
        }

        let mut rows = Vec::new();
        if matches!(place, Place::Dir(_)) {
            rows.push(Row::UseThis);
        }
        if let Some(f) = &child {
            rows.push(Row::UseChild(f.clone()));
        }
        rows.push(Row::Type);
        if original::parent(&place).is_some() {
            rows.push(Row::Up);
        }
        rows.extend(folders.into_iter().map(Row::Open));
        let supported = check.as_ref().is_some_and(FolderCheck::is_supported);
        let width = gfx.size().x - 2.0 * MARGIN - 2.0 * theme::PADDING - 40.0;
        let items = rows
            .iter()
            .map(|r| match r {
                Row::UseThis => MenuItem::new("이 폴더 사용").enabled(supported),
                Row::UseChild(f) => {
                    MenuItem::new(fit(gfx, &format!("{} 폴더 사용", f.name), width)).tag("★")
                }
                Row::Type => MenuItem::new("경로 입력…").detail(PASTE_KEY),
                Row::Up => MenuItem::new("..").detail("상위 폴더"),
                Row::Open(f) => {
                    let item = MenuItem::new(fit(gfx, &f.name, width));
                    if f.install {
                        item.tag("★")
                    } else {
                        item
                    }
                }
            })
            .collect();
        let mut menu = Menu::new(items);
        menu.tag_width = 16.0;
        menu.wrap = false;
        // Land on the folder just left, on "use this folder" (or the offered subfolder) when it
        // can be used, else on the first subfolder.
        let focused = focus.and_then(|focus| {
            rows.iter()
                .position(|r| matches!(r, Row::Open(f) if f.path == focus))
        });
        let offered = rows.iter().position(|r| matches!(r, Row::UseChild(_)));
        let first = if let Some(i) = focused {
            i
        } else if supported {
            0
        } else if let Some(i) = offered {
            i
        } else {
            rows.iter()
                .position(|r| matches!(r, Row::Open(_)))
                .or_else(|| rows.iter().position(|r| *r == Row::Up))
                .unwrap_or(0)
        };
        menu.set_cursor(first);
        self.view = View::Browse { place, rows };
        self.place_menu(gfx, menu);
    }

    /// Save the settings and load everything again.
    fn apply(ctx: &mut Ctx, dir: Option<String>, on: bool) -> Transition {
        if let Some(dir) = dir {
            ctx.settings.original_dir = Some(dir);
        }
        ctx.settings.original_mode = on;
        ctx.commit_settings();
        Transition::Flow(Flow::Reload)
    }

    fn leave_browser(&mut self, ctx: &mut Ctx) -> Transition {
        if self.browse_only {
            Transition::Pop
        } else {
            self.show_overview(ctx);
            Transition::None
        }
    }

    fn update_overview(&mut self, ctx: &mut Ctx, commands: &[Command]) -> Transition {
        match self.menu.update(ctx) {
            MenuEvent::Selected(i) => match commands[i] {
                Command::PlayOriginal => OriginalScreen::apply(ctx, None, true),
                Command::PlayBase => OriginalScreen::apply(ctx, None, false),
                Command::Browse => {
                    self.open_browser(ctx, true);
                    Transition::None
                }
                Command::Back => Transition::Pop,
            },
            MenuEvent::Cancelled => Transition::Pop,
            _ => Transition::None,
        }
    }

    /// Use `dir` as the original folder if it (still) holds a playable install: it is
    /// identified again, the folder may have changed since it was listed.
    fn use_folder(&mut self, ctx: &Ctx, dir: &Path) {
        let dir = dir.to_path_buf();
        let what = dir.display().to_string();
        self.start(ctx, true, what, move || {
            let check = original::check_folder(&dir);
            Done::Use(dir, check)
        });
    }

    /// Act on a finished read.
    fn finish(&mut self, ctx: &mut Ctx, done: Result<Done, String>) -> Transition {
        let done = match done {
            Ok(done) => done,
            Err(e) => {
                ctx.sfx(sfx::ERROR);
                ctx.toast(e);
                return self.after_cancel(ctx);
            }
        };
        match done {
            Done::Saved(check) => {
                self.saved = Some(check);
                if let View::Overview { commands } = &self.view {
                    // Keep the cursor on its command: the menu is rebuilt (and may move).
                    let on = commands.get(self.menu.cursor()).copied();
                    self.render_overview(ctx);
                    if let (Some(on), View::Overview { commands }) = (on, &self.view) {
                        if let Some(i) = commands.iter().position(|c| *c == on) {
                            self.menu.set_cursor(i);
                        }
                    }
                }
            }
            Done::Show(listing, focus) => self.show_place(ctx, listing, focus.as_deref()),
            Done::Typed(Ok(listing)) => {
                ctx.sfx(sfx::CONFIRM);
                self.typing = None;
                self.show_place(ctx, listing, None);
            }
            Done::Pasted(Ok(Some(p))) => {
                let mut text = self.typing.take().unwrap_or_default();
                text.push_str(&p);
                self.typing = Some(capped(text));
            }
            Done::Pasted(Ok(None)) => {
                ctx.sfx(sfx::ERROR);
                ctx.toast("클립보드에 텍스트가 없습니다");
            }
            Done::Pasted(Err(e)) => {
                ctx.sfx(sfx::ERROR);
                ctx.toast(e);
            }
            Done::Typed(Err(e)) => {
                ctx.sfx(sfx::ERROR);
                ctx.toast(e);
            }
            Done::Use(dir, FolderCheck::Supported(_)) => {
                return match original::storable_path(&dir) {
                    Some(path) => OriginalScreen::apply(ctx, Some(path), true),
                    None => {
                        ctx.sfx(sfx::ERROR);
                        ctx.toast(
                            "폴더 경로에 저장할 수 없는 문자가 있습니다(유니코드가 아닌 이름). \
                             폴더 이름을 바꾸거나 다른 곳으로 옮겨 주세요.",
                        );
                        Transition::None
                    }
                };
            }
            Done::Use(_, other) => {
                ctx.sfx(sfx::ERROR);
                ctx.toast(other.summary());
                // Read the place again: what it holds has changed.
                if let View::Browse { place, .. } = &self.view {
                    let place = place.clone();
                    self.open_place(ctx, place, None);
                }
            }
        }
        Transition::None
    }

    /// Where a cancelled (or failed) read leaves the screen: the view it was started from (the
    /// overview checks the saved folder again, the cancelled read may have replaced that
    /// check), or out of the screen when the browser was opened directly and has shown
    /// nothing yet.
    fn after_cancel(&mut self, ctx: &Ctx) -> Transition {
        match self.view {
            View::Overview { .. } if self.browse_only => Transition::Pop,
            View::Overview { .. } => {
                self.show_overview(ctx);
                Transition::None
            }
            View::Browse { .. } => Transition::None,
        }
    }

    /// While a read runs: its result, or Esc / a right click to cancel a read the browser waits
    /// for (its input is ignored meanwhile). The overview's check of the saved folder runs
    /// alongside the menu (`None`: go on with the view's update).
    fn update_busy(&mut self, ctx: &mut Ctx) -> Option<Transition> {
        let busy = self.busy.as_mut()?;
        if !busy.blocking {
            if let Some(done) = busy.job.poll() {
                self.busy = None;
                self.finish(ctx, done);
            }
            return None;
        }
        // Keys pressed meanwhile (or in the frame the result arrives) must not act on the view
        // that the result brings.
        let cancel = ctx.input.key_pressed(KeyCode::Escape) || ctx.input.right_click();
        ctx.input.consume();
        drain_chars();
        if let Some(done) = busy.job.poll() {
            self.busy = None;
            return Some(self.finish(ctx, done));
        }
        if cancel {
            ctx.sfx(sfx::CANCEL);
            let skips_saved = busy.skips_saved;
            self.busy = None;
            if skips_saved {
                self.open_browser(ctx, false);
                return Some(Transition::None);
            }
            return Some(self.after_cancel(ctx));
        }
        Some(Transition::None)
    }

    /// The path line: text keys type, Ctrl/Cmd+V pastes, Backspace deletes (held: repeats;
    /// Ctrl+Backspace clears), Enter opens the folder, Esc, a right click or a tap outside the
    /// line goes back to the list.
    fn update_typing(&mut self, ctx: &mut Ctx, place: &Place, mut text: String) -> Transition {
        let ctrl = ctrl_down(ctx);
        let cmd = cmd_down(ctx);
        let clear = ctrl && ctx.input.key_pressed(KeyCode::Backspace);
        // Backspace arrives as a character too (with the OS key repeat); the key press is the
        // fallback where it does not.
        let mut deleted = 0;
        while let Some(c) = get_char_pressed() {
            match c {
                '\u{8}' | '\u{7f}' => deleted += 1,
                c if c.is_control() || cmd => {}
                c => text.push(c),
            }
        }
        if deleted == 0 && ctx.input.key_pressed(KeyCode::Backspace) {
            deleted = 1;
        }
        if clear {
            text.clear();
        } else {
            for _ in 0..deleted {
                text.pop();
            }
        }
        let text = capped(text);
        if ctrl && ctx.input.key_pressed(KeyCode::V) {
            ctx.input.consume();
            self.typing = Some(text);
            self.paste(ctx);
            return Transition::None;
        }
        let frame = self.typing_frame(&ctx.gfx);
        let away = ctx.input.right_click() || ctx.input.tap().is_some_and(|p| !frame.contains(p));
        let escape = ctx.input.key_pressed(KeyCode::Escape) || away;
        let enter =
            ctx.input.key_pressed(KeyCode::Enter) || ctx.input.key_pressed(KeyCode::KpEnter);
        ctx.input.consume();
        if escape {
            ctx.sfx(sfx::CANCEL);
            return Transition::None;
        }
        if enter {
            let base = match place {
                Place::Dir(dir) => Some(dir.clone()),
                Place::Roots => None,
            };
            let typed = text.clone();
            // Testing the path reads the file system too: in the background.
            self.start(ctx, true, text.clone(), move || {
                Done::Typed(
                    original::typed_folder(&typed, base.as_deref())
                        .map(|dir| original::look(Place::Dir(dir))),
                )
            });
        }
        self.typing = Some(text);
        Transition::None
    }

    fn update_browser(&mut self, ctx: &mut Ctx, place: &Place, rows: &[Row]) -> Transition {
        if let Some(text) = self.typing.take() {
            return self.update_typing(ctx, place, text);
        }
        let ctrl = ctrl_down(ctx);
        if ctrl && ctx.input.key_pressed(KeyCode::V) {
            // Paste straight into a new path line.
            drain_chars();
            ctx.input.consume();
            self.paste(ctx);
            return Transition::None;
        }
        let up = original::parent(place);
        let here = match place {
            Place::Dir(dir) => Some(dir.as_path()),
            Place::Roots => None,
        };
        // Like a file manager: Backspace goes up one level (handled before the menu, where it
        // is a cancel key), a letter jumps to the next folder starting with it, Page Up / Page
        // Down move a page.
        if ctx.input.key_pressed(KeyCode::Backspace) {
            if let Some(p) = up.clone() {
                ctx.sfx(sfx::CANCEL);
                self.open_place(ctx, p, here);
                ctx.input.consume();
                return Transition::None;
            }
        }
        let cmd = cmd_down(ctx);
        while let Some(c) = get_char_pressed() {
            if cmd {
                continue;
            }
            if let Some(i) = next_starting_with(rows, self.menu.cursor(), c) {
                self.menu.set_cursor(i);
                ctx.sfx(sfx::CURSOR);
            }
        }
        let page = self.menu.visible_rows;
        if ctx.input.key_pressed(KeyCode::PageDown) {
            self.menu.set_cursor(self.menu.cursor() + page);
        } else if ctx.input.key_pressed(KeyCode::PageUp) {
            self.menu
                .set_cursor(self.menu.cursor().saturating_sub(page));
        }
        match self.menu.update(ctx) {
            MenuEvent::Selected(i) => match &rows[i] {
                Row::UseThis => {
                    if let Place::Dir(dir) = place {
                        self.use_folder(ctx, dir);
                    }
                    Transition::None
                }
                Row::UseChild(folder) => {
                    self.use_folder(ctx, &folder.path);
                    Transition::None
                }
                Row::Type => {
                    // Start from the folder shown, to edit or replace.
                    self.typing = Some(match place {
                        Place::Dir(dir) => dir.display().to_string(),
                        Place::Roots => String::new(),
                    });
                    Transition::None
                }
                Row::Up => {
                    if let Some(p) = up {
                        self.open_place(ctx, p, here);
                    }
                    Transition::None
                }
                Row::Open(folder) => {
                    self.open_place(ctx, Place::Dir(folder.path.clone()), None);
                    Transition::None
                }
            },
            MenuEvent::Cancelled => self.leave_browser(ctx),
            _ => Transition::None,
        }
    }
}

impl Default for OriginalScreen {
    fn default() -> Self {
        OriginalScreen::new()
    }
}

impl Screen for OriginalScreen {
    fn name(&self) -> &'static str {
        "original"
    }

    fn on_enter(&mut self, ctx: &mut Ctx, how: Enter) {
        if how != Enter::Fresh {
            return;
        }
        if self.browse_only {
            self.open_browser(ctx, true);
        } else {
            self.show_overview(ctx);
        }
    }

    fn update(&mut self, ctx: &mut Ctx) -> Transition {
        if let Some(t) = self.update_busy(ctx) {
            return t;
        }
        // The views are rebuilt by the handlers, so work on a copy of the rows.
        match &self.view {
            View::Overview { commands } => {
                let commands = commands.clone();
                self.update_overview(ctx, &commands)
            }
            View::Browse { place, rows } => {
                let (place, rows) = (place.clone(), rows.clone());
                self.update_browser(ctx, &place, &rows)
            }
        }
    }

    fn draw(&self, ctx: &Ctx) {
        clear_background(theme::BACKGROUND);
        let gfx = &ctx.gfx;
        let heading = match self.view {
            View::Overview { .. } => "원작 데이터",
            View::Browse { .. } => "원작 폴더 고르기",
        };
        gfx.text(
            heading,
            MARGIN,
            14.0,
            TextStyle::main(theme::TEXT_ACCENT)
                .size(2)
                .shadow(theme::TEXT_SHADOW),
        );
        if !self.lines.is_empty() {
            let frame = Rect::new(
                MARGIN - 6.0,
                TEXT_TOP - 6.0,
                gfx.size().x - 2.0 * MARGIN + 12.0,
                self.lines.len() as f32 * LINE + 10.0,
            );
            draw_window(frame);
        }
        for (i, (line, color)) in self.lines.iter().enumerate() {
            gfx.text(
                line,
                MARGIN + 4.0,
                TEXT_TOP + i as f32 * LINE,
                TextStyle::small(*color),
            );
        }
        match &self.typing {
            Some(text) => self.draw_typing(ctx, text),
            None => self.menu.draw(ctx),
        }
        if let Some(busy) = &self.busy {
            if busy.blocking && ctx.time - busy.since >= BUSY_NOTE_AFTER {
                self.draw_busy(ctx, busy);
            }
        }
    }
}

impl OriginalScreen {
    /// Where the path line is drawn: where the menu is.
    fn typing_frame(&self, gfx: &Gfx) -> Rect {
        let top = TEXT_TOP + self.lines.len() as f32 * LINE + 12.0;
        Rect::new(
            MARGIN,
            top,
            gfx.size().x - 2.0 * MARGIN,
            2.0 * theme::ROW_HEIGHT + 2.0 * theme::PADDING,
        )
    }

    /// The note over a slow read: what is being read and how to cancel it.
    fn draw_busy(&self, ctx: &Ctx, busy: &Busy) {
        let gfx = &ctx.gfx;
        let canvas = gfx.size();
        let height = 2.0 * theme::ROW_HEIGHT + 2.0 * theme::PADDING;
        let frame = Rect::new(
            MARGIN,
            canvas.y - height - 8.0,
            canvas.x - 2.0 * MARGIN,
            height,
        );
        draw_window(frame);
        let x = MARGIN + theme::PADDING;
        let dots = ".".repeat(1 + (ctx.time * 2.0) as usize % 3);
        let cancel = if busy.skips_saved {
            "저장된 폴더를 건너뛰고 홈 폴더에서 열기"
        } else {
            "취소"
        };
        gfx.text(
            &format!("읽는 중{dots} — Esc·우클릭: {cancel}"),
            x,
            frame.y + theme::PADDING,
            TextStyle::small(theme::TEXT_DIM),
        );
        let shown = fit_tail(gfx, &busy.what, frame.w - 2.0 * theme::PADDING);
        gfx.text(
            &shown,
            x,
            frame.y + theme::PADDING + theme::ROW_HEIGHT,
            TextStyle::main(theme::TEXT),
        );
    }

    /// The path line.
    fn draw_typing(&self, ctx: &Ctx, text: &str) {
        let gfx = &ctx.gfx;
        let frame = self.typing_frame(gfx);
        let (top, width) = (frame.y, frame.w);
        draw_window(frame);
        let x = MARGIN + theme::PADDING;
        gfx.text(
            &format!("경로를 입력하거나 붙여 넣고({PASTE_KEY}) Enter — Esc·우클릭: 목록으로"),
            x,
            top + theme::PADDING,
            TextStyle::small(theme::TEXT_DIM),
        );
        // A blinking caret after the text.
        let caret = if (ctx.time * 2.0) as i64 % 2 == 0 {
            "_"
        } else {
            " "
        };
        let shown = fit_tail(gfx, text, width - 2.0 * theme::PADDING - 12.0);
        gfx.text(
            &format!("{shown}{caret}"),
            x,
            top + theme::PADDING + theme::ROW_HEIGHT,
            TextStyle::main(theme::TEXT),
        );
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_paste_takes_the_first_line() {
        assert_eq!(first_line("C:\\GAME\r\nmore"), Some("C:\\GAME".to_string()));
        assert_eq!(
            first_line("/home/me/game\n"),
            Some("/home/me/game".to_string())
        );
        assert_eq!(first_line(""), None);
        assert_eq!(first_line("   \nsecond"), None);
    }

    use super::*;
    use std::path::PathBuf;

    fn folder(name: &str) -> Row {
        Row::Open(Folder {
            name: name.into(),
            path: PathBuf::from(name),
            install: false,
        })
    }

    #[test]
    fn letters_jump_to_folders() {
        let rows = vec![
            Row::UseThis,
            Row::Up,
            folder("Game"),
            folder("hero"),
            folder("Hermes"),
            folder("tools"),
        ];
        assert_eq!(next_starting_with(&rows, 0, 'h'), Some(3));
        assert_eq!(next_starting_with(&rows, 3, 'H'), Some(4));
        // Wraps around to the first match.
        assert_eq!(next_starting_with(&rows, 4, 'h'), Some(3));
        assert_eq!(next_starting_with(&rows, 5, 'g'), Some(2));
        assert_eq!(next_starting_with(&rows, 0, 'x'), None);
        assert_eq!(next_starting_with(&rows, 0, ' '), None);
        let rows = vec![folder("xargs"), folder("zelda")];
        assert_eq!(next_starting_with(&rows, 0, 'x'), None, "cancel key");
        assert_eq!(next_starting_with(&rows, 0, 'Z'), None, "confirm key");
    }
}
