//! Frontend of Eiketsuden Reloaded: the macroquad application that runs natively
//! (Windows / Linux / macOS) and in the browser (wasm32).
//!
//! The crate is a small framework plus the screens built on it:
//!
//! | module | role |
//! |---|---|
//! | [`app`] | shared context [`app::Ctx`], the [`app::Screen`] trait, the screen stack with fades, [`app::run`] |
//! | [`flow`] | game flow (title → campaign nodes → ending) and **the plug-in point for the drama / camp / battle screens** |
//! | [`gfx`] | virtual canvas (the pack's presentation profile, 480×270 by default) with integer scaling, fonts, text, word wrap, drawing helpers |
//! | [`ui`] | procedural Eiketsuden-style widgets: windows, menus, message box, dialogs, gauges, toasts, tooltips |
//! | [`input`] | per-frame input snapshot: confirm / cancel / navigation with key repeat / pointer, tap and drag |
//! | [`audio`] | music by key with fades, sound effects by key, volumes, web audio unlock |
//! | [`assets`] | async file reads and the lazy media store (textures, sounds, icons) with fallbacks |
//! | [`platform`] | data pack location, launch options, clock, key/value storage (files or `localStorage`) |
//! | `original` | the original mode: converting the player's own copy at launch (native only) |
//! | [`saves`] | save slots (autosave, quick save, manual) on top of the storage |
//! | [`quicksave`] | quick save / load (F5 / F9) at any moment, incl. mid-scene and mid-battle |
//! | [`settings`] | player settings persisted as JSON |
//! | [`screens`] | loading, error, title, save/load, settings, controls, credits, game over, UI gallery |
//! | [`secret`] | the original's hidden command ("금단의 비법", tapping the lord's portrait) |
//!
//! Start with the module docs of [`app`] (how to write a screen) and [`flow`] (where campaign
//! screens plug in). All drawing happens in virtual canvas coordinates, see [`gfx`].

pub mod app;
pub mod assets;
pub mod audio;
pub mod flow;
pub mod gfx;
pub mod input;
#[cfg(not(target_arch = "wasm32"))]
pub mod original;
pub mod platform;
pub mod quicksave;
pub mod saves;
pub mod screens;
pub mod secret;
pub mod settings;
pub mod ui;
