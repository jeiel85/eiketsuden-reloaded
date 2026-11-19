//! Browser glue: the functions `web/hero_web.js` registers in the WebAssembly import object.
//!
//! Keep this file and `web/hero_web.js` in sync. When the set of functions or their meaning
//! changes, bump [`HERO_WEB_VERSION`] here and `version` in the JS plugin together; the miniquad
//! loader then reports a mismatch in the console instead of failing in obscure ways.

use super::storage::{validate_key, KeyValueStore, StorageError};
use sapp_jsutils::JsObject;

/// Version of the `hero_web` JS plugin this build expects.
pub const HERO_WEB_VERSION: u32 = 5;

/// Where an async clipboard request of `web/hero_web.js` stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardState {
    /// The request was made and has not answered yet.
    Running,
    /// The text was copied.
    Copied,
    /// The browser refused the copy.
    CopyFailed,
    /// The clipboard's text is in [`clipboard_text`].
    Pasted,
    /// The clipboard held no text (or only blank text).
    Empty,
    /// The browser refused the read.
    PasteFailed,
    /// There is no request at all (the page has no clipboard API to ask).
    NoRequest,
}

// Imported from the `env` module that the miniquad loader fills from registered plugins.
#[link(wasm_import_module = "env")]
extern "C" {
    fn hero_web_ready();
    fn hero_web_panic(message: JsObject);
    fn hero_storage_has(key: JsObject) -> i32;
    fn hero_storage_get(key: JsObject) -> JsObject;
    fn hero_storage_set(key: JsObject, value: JsObject) -> i32;
    fn hero_storage_remove(key: JsObject) -> i32;
    fn hero_now_seconds() -> f64;
    fn hero_location_hash() -> JsObject;
    fn hero_pointer_inside() -> i32;
    fn hero_clipboard_copy(text: JsObject);
    fn hero_clipboard_paste();
    /// 0 while the request runs, 1 copied, 2 copy refused, 3 pasted, 4 empty, 5 paste refused.
    fn hero_clipboard_state() -> i32;
    fn hero_clipboard_text() -> JsObject;
    fn hero_clipboard_reset();
}

/// Prefix of every `localStorage` item written by the game.
const KEY_PREFIX: &str = "eiketsuden.";

/// Checked by the miniquad JS loader against the plugin's `version` field.
#[no_mangle]
pub extern "C" fn hero_web_crate_version() -> u32 {
    HERO_WEB_VERSION
}

pub fn ready() {
    unsafe { hero_web_ready() }
}

/// Show a crash message over the canvas (the game cannot continue after a panic).
pub fn show_panic(message: &str) {
    unsafe { hero_web_panic(JsObject::string(message)) }
}

pub fn now_seconds() -> f64 {
    unsafe { hero_now_seconds() }
}

/// Whether the pointer is over the canvas and the page has focus.
pub fn pointer_inside() -> bool {
    unsafe { hero_pointer_inside() != 0 }
}

pub fn location_hash() -> String {
    let obj = unsafe { hero_location_hash() };
    let mut s = String::new();
    obj.to_string(&mut s);
    s
}

/// Ask the page to put `text` on the clipboard; the answer arrives from [`clipboard_state`].
///
/// Output: `Err` when the page has no clipboard API to ask, so the caller shows that instead of
/// waiting for an answer that never comes.
pub fn clipboard_copy(text: &str) -> Result<(), String> {
    unsafe { hero_clipboard_copy(JsObject::string(text)) }
    if unsafe { hero_clipboard_state() } == NO_REQUEST {
        return Err(CLIPBOARD_MISSING.into());
    }
    Ok(())
}

/// Ask the page to read the clipboard; the answer arrives from [`clipboard_state`].
pub fn clipboard_paste() -> Result<(), String> {
    unsafe { hero_clipboard_paste() }
    if unsafe { hero_clipboard_state() } == NO_REQUEST {
        return Err(CLIPBOARD_MISSING.into());
    }
    Ok(())
}

/// Where the request made last stands (never "still running" is conflated with "no request").
pub fn clipboard_state() -> ClipboardState {
    match unsafe { hero_clipboard_state() } {
        0 => ClipboardState::Running,
        1 => ClipboardState::Copied,
        2 => ClipboardState::CopyFailed,
        3 => ClipboardState::Pasted,
        4 => ClipboardState::Empty,
        5 => ClipboardState::PasteFailed,
        _ => ClipboardState::NoRequest,
    }
}

/// The text of a settled read, or the browser's reason for a refusal.
pub fn clipboard_text() -> String {
    let obj = unsafe { hero_clipboard_text() };
    let mut s = String::new();
    obj.to_string(&mut s);
    s
}

/// Forget the last request (its text is kept by the caller).
pub fn clipboard_reset() {
    unsafe { hero_clipboard_reset() }
}

/// `hero_clipboard_state`'s "there is no request at all" (the page lacks the API).
const NO_REQUEST: i32 = -1;

/// Shown when the page has no clipboard API to ask.
pub const CLIPBOARD_MISSING: &str =
    "이 브라우저에서는 클립보드를 쓸 수 없습니다 (HTTPS 또는 localhost에서 열어 주세요)";

/// `localStorage`-backed [`KeyValueStore`].
pub struct LocalStorage;

fn full_key(key: &str) -> Result<String, StorageError> {
    validate_key(key)?;
    Ok(format!("{KEY_PREFIX}{key}"))
}

impl KeyValueStore for LocalStorage {
    fn get(&self, key: &str) -> Result<Option<String>, StorageError> {
        let k = full_key(key)?;
        if unsafe { hero_storage_has(JsObject::string(&k)) } == 0 {
            return Ok(None);
        }
        let obj = unsafe { hero_storage_get(JsObject::string(&k)) };
        let mut s = String::new();
        obj.to_string(&mut s);
        Ok(Some(s))
    }

    fn set(&mut self, key: &str, value: &str) -> Result<(), StorageError> {
        let k = full_key(key)?;
        let ok = unsafe { hero_storage_set(JsObject::string(&k), JsObject::string(value)) };
        if ok == 1 {
            Ok(())
        } else {
            Err(StorageError {
                key: key.to_string(),
                msg: "browser storage is full or disabled".into(),
            })
        }
    }

    fn remove(&mut self, key: &str) -> Result<(), StorageError> {
        let k = full_key(key)?;
        if unsafe { hero_storage_remove(JsObject::string(&k)) } == 1 {
            Ok(())
        } else {
            Err(StorageError {
                key: key.to_string(),
                msg: "browser storage is disabled".into(),
            })
        }
    }

    fn location(&self) -> String {
        "browser localStorage".into()
    }
}
