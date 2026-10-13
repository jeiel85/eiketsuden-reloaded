//! Application core: the shared [`Ctx`], the [`Screen`] trait and the screen stack.
//!
//! # Writing a screen
//!
//! A screen is a struct implementing [`Screen`]. Every frame the app calls
//! [`Screen::update`] on the **top** screen only, then [`Screen::draw`] on the visible screens
//! (the top screen, plus the screens below it while the top ones are overlays). `update` returns
//! a [`Transition`]:
//!
//! * [`Transition::Push`] a screen on top (e.g. settings from the title screen),
//!   [`Transition::Pop`] back, [`Transition::Replace`] the top screen;
//! * [`Transition::Flow`] to move through the game flow ([`crate::flow::Flow`]: title, new game,
//!   next campaign node, ...), which replaces the whole stack;
//! * [`Transition::Quit`] (native only);
//! * [`Transition::QuickSave`] / [`Transition::QuickLoad`], what the F5 / F9 keys do
//!   ([`crate::quicksave`]), for menus that offer them.
//!
//! Stack changes fade to black and back ([`FADE_SECONDS`] each way) unless the screen being
//! pushed or popped is an overlay ([`Screen::is_overlay`]), which appears instantly on top of
//! the screen below it. [`Screen::on_enter`] runs when a screen becomes the top screen: after it
//! was pushed ([`Enter::Fresh`]) or when the screen above it was popped ([`Enter::Resumed`]) —
//! the place to (re)start music or refresh data.
//!
//! Screens draw in virtual canvas coordinates — the canvas size is the pack's presentation
//! profile, read from [`crate::gfx::Gfx::size`] (see [`crate::gfx`]) — and read input from
//! [`Ctx::input`]. Widgets from [`crate::ui`] take `&mut Ctx` in their `update` (they play UI
//! sounds) and `&Ctx` in `draw`.

use crate::assets::Media;
use crate::audio::{sfx, Audio};
use crate::flow::{Flow, Session};
use crate::gfx::{fill_rect, Gfx, TextStyle};
use crate::input::Input;
use crate::platform::storage::KeyValueStore;
use crate::platform::{DataRoot, LaunchOptions};
use crate::quicksave::{self, ResumePoint};
use crate::settings::Settings;
use crate::ui::theme;
use crate::ui::toast::Toasts;
use hero_core::pack::Pack;
use macroquad::prelude::*;
use std::rc::Rc;

/// Duration of each half of a screen transition fade.
pub const FADE_SECONDS: f32 = 0.18;
/// Longest frame time fed to the game (avoids huge jumps after a stall).
pub const MAX_FRAME_TIME: f32 = 0.1;

/// Everything screens share. Created once at startup.
pub struct Ctx {
    pub gfx: Gfx,
    pub input: Input,
    pub audio: Audio,
    pub media: Media,
    pub settings: Settings,
    pub storage: Box<dyn KeyValueStore>,
    pub toasts: Toasts,
    /// The loaded data pack (`None` before loading finishes and in the UI gallery).
    pub pack: Option<Rc<Pack>>,
    /// The campaign being played (`None` on the title screen).
    pub session: Option<Session>,
    pub options: LaunchOptions,
    pub data_root: DataRoot,
    /// Seconds since the previous frame (clamped to [`MAX_FRAME_TIME`]).
    pub dt: f32,
    /// The original mode's songs being rendered (added to the mounted pack as they finish).
    #[cfg(not(target_arch = "wasm32"))]
    pub music: Option<crate::original::MusicRender>,
    /// Seconds since startup.
    pub time: f64,
    pub frame: u64,
    /// The original mode was converted at this launch without the new art's maps (the "그림"
    /// setting was 원작, D27): choosing 새 그림 now changes the units, and the maps at the next
    /// launch.
    pub remake_maps_missing: bool,
}

impl Ctx {
    /// Build the context: platform storage, settings, canvas, media and audio.
    pub fn new(options: LaunchOptions) -> Ctx {
        let data_root = DataRoot::resolve(&options);
        let storage = crate::platform::storage::open_default();
        let (settings, warning) = Settings::load(storage.as_ref());
        if let Some(w) = warning {
            macroquad::logging::warn!("{}", w);
        }
        Ctx {
            gfx: Gfx::new(),
            input: Input::new(),
            audio: Audio::new(&settings),
            media: Media::for_settings(data_root.clone(), &settings),
            settings,
            storage,
            toasts: Toasts::default(),
            pack: None,
            session: None,
            options,
            data_root,
            dt: 0.0,
            #[cfg(not(target_arch = "wasm32"))]
            music: None,
            time: 0.0,
            frame: 0,
            remake_maps_missing: false,
        }
    }

    /// Play a UI sound effect by key (see [`crate::audio::sfx`]).
    pub fn sfx(&mut self, key: &str) {
        self.audio.sfx(&self.media, key);
    }

    /// Show a short message at the top of the screen.
    pub fn toast(&mut self, text: impl Into<String>) {
        self.toasts.push(text);
    }

    /// Id of the loaded pack.
    pub fn pack_id(&self) -> Option<&str> {
        self.pack.as_deref().map(|p| p.manifest.id.as_str())
    }

    /// Apply the current settings (volumes, fullscreen) and persist them. Storage failures are
    /// reported with a toast.
    pub fn commit_settings(&mut self) {
        self.audio.apply_settings(&self.settings);
        if crate::platform::can_toggle_fullscreen() {
            set_fullscreen(self.settings.fullscreen);
        }
        if let Err(e) = self.settings.save(self.storage.as_mut()) {
            macroquad::logging::error!("cannot save settings: {}", e);
            self.toast(format!("설정을 저장하지 못했습니다: {e}"));
        }
    }
}

/// How a screen became the top screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Enter {
    /// Just pushed (or installed by a flow change).
    Fresh,
    /// The screen above it was popped.
    Resumed,
}

/// What the app should do after a screen's update.
pub enum Transition {
    None,
    Push(Box<dyn Screen>),
    Pop,
    Replace(Box<dyn Screen>),
    Flow(Flow),
    /// Close the game (ignored on the web).
    Quit,
    /// Write the quick save slot now (the F5 key), without any transition.
    QuickSave,
    /// Load the quick save slot (the F9 key): replaces the whole stack like [`Transition::Flow`].
    QuickLoad,
}

impl Transition {
    pub fn push(screen: impl Screen + 'static) -> Transition {
        Transition::Push(Box::new(screen))
    }

    pub fn replace(screen: impl Screen + 'static) -> Transition {
        Transition::Replace(Box::new(screen))
    }
}

/// A full screen or overlay. See the module docs.
pub trait Screen {
    /// Short name for logs.
    fn name(&self) -> &'static str;

    /// The screen became the top screen.
    fn on_enter(&mut self, _ctx: &mut Ctx, _how: Enter) {}

    /// Handle input and advance animations. Called once per frame while on top.
    fn update(&mut self, ctx: &mut Ctx) -> Transition;

    /// Draw in virtual coordinates.
    fn draw(&self, ctx: &Ctx);

    /// Overlays are pushed/popped without a fade and the screen below keeps being drawn.
    fn is_overlay(&self) -> bool {
        false
    }

    /// A camp screen: with the pack's camp frame it is laid out in the frame's view as if that
    /// were the whole canvas, and the frame is drawn around it (`screens::camp::frame`).
    fn in_camp_frame(&self) -> bool {
        false
    }

    /// Where this screen is, for a quick save ([`crate::quicksave`]). Called on **every** screen
    /// of the stack, bottom to top; the default reports nothing, which is right for every screen
    /// whose state is fully in the session's campaign (camp, menus, settings). A screen with
    /// state of its own that the campaign lacks (a scene half-way, a battle) reports it, or
    /// [`ResumePoint::Unavailable`] when it cannot be saved at this instant.
    fn resume_point(&self, _ctx: &Ctx) -> Option<ResumePoint> {
        None
    }
}

/// The loaded pack's camp frame's view, where camp screens are laid out
/// ([`Screen::in_camp_frame`]).
fn camp_view(ctx: &Ctx) -> Option<Rect> {
    let frame = ctx
        .pack
        .as_deref()?
        .manifest
        .presentation
        .camp_frame
        .as_ref()?;
    Some(crate::screens::camp::frame::area(frame.view))
}

/// Run `f` with the canvas and the input in the camp frame's view when `framed` and the pack has
/// a camp frame (see [`Screen::in_camp_frame`]): layout, drawing and hit tests of a camp screen
/// then all see the view as the whole canvas. Drawing needs the canvas begun again afterwards.
fn in_camp_view<R>(ctx: &mut Ctx, framed: bool, f: impl FnOnce(&mut Ctx) -> R) -> R {
    let view = camp_view(ctx).filter(|_| framed);
    let canvas = ctx.gfx.canvas.full_size();
    if let Some(v) = view {
        ctx.gfx.canvas.set_view(Some(v));
        ctx.input.enter_view(v);
    }
    let result = f(ctx);
    if let Some(v) = view {
        ctx.input.leave_view(v, canvas);
        ctx.gfx.canvas.set_view(None);
    }
    result
}

/// [`Screen::on_enter`] in the camp frame's view for a camp screen.
fn enter_screen(ctx: &mut Ctx, screen: &mut dyn Screen, how: Enter) {
    let framed = screen.in_camp_frame();
    in_camp_view(ctx, framed, |ctx| screen.on_enter(ctx, how));
}

enum Fade {
    Idle,
    Out { t: f32, pending: Transition },
    In { t: f32 },
}

/// The screen stack and main loop body.
pub struct App {
    ctx: Ctx,
    stack: Vec<Box<dyn Screen>>,
    fade: Fade,
    quit: bool,
    presented: bool,
    show_fps: bool,
}

impl App {
    pub fn new(mut ctx: Ctx, mut first: Box<dyn Screen>) -> App {
        if ctx.settings.fullscreen && crate::platform::can_toggle_fullscreen() {
            set_fullscreen(true);
        }
        enter_screen(&mut ctx, first.as_mut(), Enter::Fresh);
        App {
            ctx,
            stack: vec![first],
            fade: Fade::In { t: 0.0 },
            quit: false,
            presented: false,
            show_fps: false,
        }
    }

    /// Run one frame: input, update, draw. Returns `false` when the game should exit.
    pub fn frame(&mut self) -> bool {
        let dt = get_frame_time().clamp(0.0, MAX_FRAME_TIME);
        let ctx = &mut self.ctx;
        ctx.dt = dt;
        ctx.time += f64::from(dt);
        ctx.frame += 1;
        ctx.gfx.canvas.update();
        ctx.input.update(dt, &ctx.gfx.canvas);
        ctx.media.pump();
        // A view-only setting (D25 X2): follows the settings screen at once (a rebuilt media
        // store starts with it, `Media::for_settings`).
        ctx.media
            .set_public_portraits(ctx.settings.portraits == crate::settings::PortraitStyle::Public);
        ctx.media
            .set_remake_art(ctx.settings.art == crate::settings::ArtStyle::Remake);
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(music) = ctx.music.as_mut() {
            if music.poll(&ctx.media, &mut ctx.audio) {
                ctx.music = None;
            }
        }
        ctx.audio.update(dt, &ctx.media, ctx.input.any_activity());
        ctx.toasts.update(dt);
        if let Some(session) = ctx.session.as_mut() {
            session.tick(dt);
        }
        self.global_keys();

        match std::mem::replace(&mut self.fade, Fade::Idle) {
            Fade::Out { t, pending } => {
                self.ctx.input.consume();
                let t = t + dt / FADE_SECONDS;
                if t >= 1.0 {
                    self.apply(pending);
                    self.fade = Fade::In { t: 0.0 };
                } else {
                    self.fade = Fade::Out { t, pending };
                }
            }
            Fade::In { t } => {
                self.ctx.input.consume();
                let t = t + dt / FADE_SECONDS;
                if t < 1.0 {
                    self.fade = Fade::In { t };
                }
            }
            Fade::Idle => {
                if let Some(top) = self.stack.last_mut() {
                    let framed = top.in_camp_frame();
                    let transition = in_camp_view(&mut self.ctx, framed, |ctx| top.update(ctx));
                    self.handle(transition);
                }
            }
        }

        self.draw();
        if !self.presented {
            self.presented = true;
            crate::platform::notify_ready();
        }
        !self.quit
    }

    fn global_keys(&mut self) {
        if is_key_pressed(KeyCode::F3) {
            self.show_fps = !self.show_fps;
        }
        let alt_enter = is_key_pressed(KeyCode::Enter)
            && (is_key_down(KeyCode::LeftAlt) || is_key_down(KeyCode::RightAlt));
        if (is_key_pressed(KeyCode::F11) || alt_enter) && crate::platform::can_toggle_fullscreen() {
            self.ctx.settings.fullscreen = !self.ctx.settings.fullscreen;
            self.ctx.commit_settings();
            self.ctx.input.consume();
        }
        if is_key_pressed(quicksave::SAVE_KEY) {
            self.handle(Transition::QuickSave);
        }
        if is_key_pressed(quicksave::LOAD_KEY) {
            self.handle(Transition::QuickLoad);
        }
        if is_key_pressed(quicksave::SLOT_KEY) {
            self.ctx.settings.next_quick_slot();
            self.ctx.commit_settings();
            let slot = self.ctx.settings.quick_save_slot();
            self.ctx.sfx(sfx::CURSOR);
            self.ctx.toast(format!("F5·F9: {}", slot.name()));
        }
    }

    /// Write the quick save slot. Ignored during a fade (the stack is about to change and would
    /// be saved half-way); the result is reported with a toast.
    fn quick_save(&mut self) {
        // No game running (title, loading ...): there is nothing to save, so F5 does nothing.
        if !matches!(self.fade, Fade::Idle) || self.ctx.session.is_none() {
            return;
        }
        match quicksave::save(&mut self.ctx, &self.stack) {
            Ok(()) => {
                self.ctx.sfx(sfx::CONFIRM);
                let slot = self.ctx.settings.quick_save_slot();
                self.ctx.toast(format!("{}에 저장했습니다.", slot.name()));
            }
            Err(why) => {
                macroquad::logging::warn!("quick save failed: {}", why);
                self.ctx.sfx(sfx::ERROR);
                self.ctx.toast(format!("순간 저장을 할 수 없습니다: {why}"));
            }
        }
    }

    /// Load the quick save slot: fades out and continues the save like 이어하기. `None` (and a
    /// toast) when there is nothing to load, so a stray F9 never costs the current progress.
    fn quick_load(&mut self) -> Option<Transition> {
        if !matches!(self.fade, Fade::Idle) {
            return None;
        }
        match quicksave::read(&self.ctx) {
            Ok(save) => {
                let slot = self.ctx.settings.quick_save_slot();
                self.ctx.toast(format!("{}을(를) 불러옵니다.", slot.name()));
                Some(Transition::Flow(Flow::Continue(Box::new(save))))
            }
            Err(why) => {
                self.ctx.sfx(sfx::ERROR);
                self.ctx
                    .toast(format!("순간 저장을 불러올 수 없습니다: {why}"));
                None
            }
        }
    }

    fn handle(&mut self, transition: Transition) {
        match transition {
            Transition::None => {}
            Transition::Quit => {
                if crate::platform::can_quit() {
                    self.quit = true;
                }
            }
            Transition::QuickSave => self.quick_save(),
            Transition::QuickLoad => {
                if let Some(flow) = self.quick_load() {
                    self.handle(flow);
                }
            }
            Transition::Push(screen) if screen.is_overlay() => self.apply(Transition::Push(screen)),
            Transition::Pop if self.stack.last().is_some_and(|s| s.is_overlay()) => {
                self.apply(Transition::Pop)
            }
            other => {
                self.fade = Fade::Out {
                    t: 0.0,
                    pending: other,
                };
            }
        }
    }

    fn apply(&mut self, transition: Transition) {
        let ctx = &mut self.ctx;
        match transition {
            Transition::None | Transition::Quit => {}
            // Both are resolved in `handle` before a transition can be applied.
            Transition::QuickSave | Transition::QuickLoad => {}
            Transition::Push(mut screen) => {
                enter_screen(ctx, screen.as_mut(), Enter::Fresh);
                self.stack.push(screen);
            }
            Transition::Pop => {
                if self.stack.len() <= 1 {
                    macroquad::logging::error!("screen stack: cannot pop the last screen");
                    return;
                }
                self.stack.pop();
                if let Some(top) = self.stack.last_mut() {
                    enter_screen(ctx, top.as_mut(), Enter::Resumed);
                }
            }
            Transition::Replace(mut screen) => {
                self.stack.pop();
                enter_screen(ctx, screen.as_mut(), Enter::Fresh);
                self.stack.push(screen);
            }
            Transition::Flow(flow) => {
                let mut screen = crate::flow::enter(flow, ctx);
                self.stack.clear();
                enter_screen(ctx, screen.as_mut(), Enter::Fresh);
                self.stack.push(screen);
            }
        }
        // The new top screen starts with a clean input frame.
        ctx.input.consume();
    }

    fn draw(&mut self) {
        let ctx = &mut self.ctx;
        let stack = &self.stack;
        ctx.gfx.canvas.begin();
        clear_background(BLACK);
        let base = stack.iter().rposition(|s| !s.is_overlay()).unwrap_or(0);
        let frame = ctx
            .pack
            .as_deref()
            .and_then(|p| p.manifest.presentation.camp_frame.clone());
        for screen in &stack[base..] {
            match &frame {
                Some(frame) if screen.in_camp_frame() => {
                    // Hover tests while drawing see the view too.
                    in_camp_view(ctx, true, |ctx| {
                        ctx.gfx.canvas.begin();
                        screen.draw(ctx);
                    });
                    ctx.gfx.canvas.begin();
                    crate::screens::camp::frame::draw_camp_frame(ctx, frame);
                }
                _ => screen.draw(ctx),
            }
        }
        ctx.toasts.draw(&ctx.gfx);

        let fade = match &self.fade {
            Fade::Idle => 0.0,
            Fade::Out { t, .. } => t.clamp(0.0, 1.0),
            Fade::In { t } => 1.0 - t.clamp(0.0, 1.0),
        };
        if fade > 0.0 {
            fill_rect(ctx.gfx.screen(), Color::new(0.0, 0.0, 0.0, fade));
        }
        if self.show_fps {
            let top = self.stack.last().map(|s| s.name()).unwrap_or("-");
            let text = format!(
                "{} fps  S={}  {}x{}  {}  media:{}",
                get_fps(),
                ctx.gfx.scale(),
                ctx.gfx.size().x,
                ctx.gfx.size().y,
                top,
                ctx.media.pending()
            );
            fill_rect(
                Rect::new(0.0, 0.0, ctx.gfx.size().x, 13.0),
                Color::new(0.0, 0.0, 0.0, 0.6),
            );
            ctx.gfx.text(&text, 3.0, 0.0, TextStyle::small(theme::TEXT));
        }
        ctx.gfx.canvas.present(theme::LETTERBOX);
    }
}

/// Run the game until it quits: the entry point used by `main.rs`.
pub async fn run(options: LaunchOptions) {
    for w in &options.warnings {
        macroquad::logging::warn!("{}", w);
    }
    let ctx = Ctx::new(options);
    let target = if ctx.options.gallery {
        crate::screens::loading::Target::Gallery
    } else {
        crate::screens::loading::Target::Game
    };
    let first = Box::new(crate::screens::loading::LoadingScreen::new(target));
    let mut app = App::new(ctx, first);
    while app.frame() {
        next_frame().await;
    }
}
