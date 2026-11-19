//! The system clipboard: text out (a save carried to another device) and text in.
//!
//! Natively the platform clipboard answers at once, except on Linux: miniquad's X11 read waits
//! for the clipboard's owner forever, so the read goes through `arboard` on a worker thread with
//! its own X11 connection and gives up on an owner that does not answer (docs/DECISIONS.md D14).
//! The browser answers through a promise instead. Both need the same shape, so [`Clipboard`]
//! starts the work and reports the outcome once, later: no call blocks the frame and none claims
//! to have succeeded before the answer is in.
//!
//! Two callers:
//!
//! * the save screen (save export / import), which must not stall the frame: [`Clipboard`];
//! * the folder browser (`crate::screens::original`), which waits for a line with its own
//!   cancelable background job and so reads through [`read_blocking`] on a thread it owns.
//!
//! The browser needs the player's key press or click to reach the clipboard at all (it may ask
//! for permission), and answers with the reason when it refuses. A native write has no failure
//! report to give (miniquad hands the text to the window's display loop), so [`Event::Copied`]
//! means the text was handed over; the browser's answer is the real one.

#[cfg(target_os = "linux")]
use std::sync::mpsc::channel;
use std::sync::mpsc::{Receiver, TryRecvError};

#[cfg(target_arch = "wasm32")]
use super::web::CLIPBOARD_MISSING;

/// What a started operation reports when it settles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The text was handed to the clipboard (the browser reports a refusal).
    Copied(Result<(), String>),
    /// The clipboard's text (`None` when it holds none), or why it could not be read.
    Pasted(Result<Option<String>, String>),
}

#[derive(Debug)]
enum Job {
    /// A read running on a worker (`Clipboard::paste` does this on Linux only, where the X11
    /// read blocks; the variant is compiled everywhere so its handling is tested on any host).
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    Running(Receiver<Result<Option<String>, String>>),
    /// A read that answered at once (the native clipboard does, except on Linux where the
    /// read runs on a worker; the variant stays compiled so its handling is tested on any host).
    #[cfg(not(target_arch = "wasm32"))]
    #[cfg_attr(target_os = "linux", allow(dead_code))]
    Read(Result<Option<String>, String>),
    /// A write with nothing to wait for (the native clipboard reports no failure).
    #[cfg(not(target_arch = "wasm32"))]
    Wrote,
    #[cfg(target_arch = "wasm32")]
    BrowserCopy,
    #[cfg(target_arch = "wasm32")]
    BrowserPaste,
}

/// The clipboard, one operation at a time.
#[derive(Debug, Default)]
pub struct Clipboard {
    job: Option<Job>,
}
impl Clipboard {
    pub fn new() -> Clipboard {
        Clipboard::default()
    }

    /// Whether an operation started earlier has not reported yet.
    pub fn busy(&self) -> bool {
        self.job.is_some()
    }

    /// Give up on the running operation: its answer is dropped and the caller may start another.
    pub fn abandon(&mut self) {
        self.job = None;
        #[cfg(target_arch = "wasm32")]
        super::web::clipboard_reset();
    }

    /// Refuse a new operation while one is still running: its answer would be lost.
    fn idle(&self) -> Result<(), String> {
        if self.busy() {
            return Err("앞선 클립보드 작업이 아직 끝나지 않았습니다".into());
        }
        Ok(())
    }

    /// Put `text` on the clipboard; the outcome arrives from [`Clipboard::poll`].
    ///
    /// Input: the text. Output: `Err` when the request could not be made — the browser has no
    /// clipboard access at all, or an earlier operation has not reported yet (starting another
    /// would drop its answer).
    pub fn copy(&mut self, text: &str) -> Result<(), String> {
        self.idle()?;
        #[cfg(target_arch = "wasm32")]
        {
            super::web::clipboard_copy(text)?;
            self.job = Some(Job::BrowserCopy);
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            macroquad::miniquad::window::clipboard_set(text);
            self.job = Some(Job::Wrote);
        }
        Ok(())
    }

    /// Read the clipboard's text; the text (or the reason there is none) arrives from
    /// [`Clipboard::poll`].
    pub fn paste(&mut self) -> Result<(), String> {
        self.idle()?;
        #[cfg(target_arch = "wasm32")]
        {
            super::web::clipboard_paste()?;
            self.job = Some(Job::BrowserPaste);
        }
        #[cfg(target_os = "linux")]
        {
            // The X11 read blocks for as long as the clipboard's owner takes: never on this thread.
            let (tx, rx) = channel();
            std::thread::Builder::new()
                .name("clipboard-read".into())
                .spawn(move || {
                    let _ = tx.send(read_blocking());
                })
                .map_err(|e| format!("클립보드를 읽는 작업을 시작하지 못했습니다: {e}"))?;
            self.job = Some(Job::Running(rx));
        }
        #[cfg(all(not(target_os = "linux"), not(target_arch = "wasm32")))]
        {
            self.job = Some(Job::Read(read_blocking()));
        }
        Ok(())
    }

    /// The outcome of the operation started last, once it has settled.
    pub fn poll(&mut self) -> Option<Event> {
        let job = self.job.take()?;
        let done = match job {
            #[cfg(not(target_arch = "wasm32"))]
            Job::Wrote => Event::Copied(Ok(())),
            #[cfg(not(target_arch = "wasm32"))]
            Job::Read(result) => Event::Pasted(result),
            Job::Running(rx) => match rx.try_recv() {
                Ok(result) => Event::Pasted(result),
                Err(TryRecvError::Empty) => {
                    self.job = Some(Job::Running(rx));
                    return None;
                }
                Err(TryRecvError::Disconnected) => {
                    Event::Pasted(Err("클립보드를 읽는 작업이 끝나지 않았습니다".into()))
                }
            },
            #[cfg(target_arch = "wasm32")]
            Job::BrowserCopy => match super::web::clipboard_state() {
                super::web::ClipboardState::Copied => {
                    super::web::clipboard_reset();
                    Event::Copied(Ok(()))
                }
                super::web::ClipboardState::CopyFailed => {
                    let why = super::web::clipboard_text();
                    super::web::clipboard_reset();
                    Event::Copied(Err(why))
                }
                // A page that lost its clipboard API mid-request: report it instead of
                // waiting for an answer that will never come.
                super::web::ClipboardState::NoRequest => {
                    Event::Copied(Err(CLIPBOARD_MISSING.into()))
                }
                _ => {
                    self.job = Some(Job::BrowserCopy);
                    return None;
                }
            },
            #[cfg(target_arch = "wasm32")]
            Job::BrowserPaste => match super::web::clipboard_state() {
                super::web::ClipboardState::Pasted => {
                    let text = super::web::clipboard_text();
                    super::web::clipboard_reset();
                    Event::Pasted(Ok(non_empty(text)))
                }
                super::web::ClipboardState::Empty => {
                    super::web::clipboard_reset();
                    Event::Pasted(Ok(None))
                }
                super::web::ClipboardState::PasteFailed => {
                    let why = super::web::clipboard_text();
                    super::web::clipboard_reset();
                    Event::Pasted(Err(why))
                }
                super::web::ClipboardState::NoRequest => {
                    Event::Pasted(Err(CLIPBOARD_MISSING.into()))
                }
                _ => {
                    self.job = Some(Job::BrowserPaste);
                    return None;
                }
            },
        };
        Some(done)
    }
}

/// Read the clipboard and return once it has an answer: for callers that run it on a thread of
/// their own (the folder browser's cancelable read).
///
/// Only for native builds: the browser's read is a promise, which [`Clipboard`] polls.
#[cfg(not(target_arch = "wasm32"))]
pub fn read_blocking() -> Result<Option<String>, String> {
    #[cfg(target_os = "linux")]
    {
        // arboard waits 4 s for each of the six text formats it asks for, so a clipboard owner
        // that does not answer costs up to about 24 s; a timeout reads as an empty clipboard.
        let mut clipboard =
            arboard::Clipboard::new().map_err(|e| format!("클립보드를 열지 못했습니다: {e}"))?;
        match clipboard.get_text() {
            Ok(text) => Ok(non_empty(text)),
            Err(arboard::Error::ContentNotAvailable) => Ok(None),
            Err(e) => Err(format!("클립보드를 읽지 못했습니다: {e}")),
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        Ok(macroquad::miniquad::window::clipboard_get().and_then(non_empty))
    }
}

/// `text` when it holds anything, `None` for an empty clipboard.
fn non_empty(text: String) -> Option<String> {
    (!text.is_empty()).then_some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A settled operation reports once, and polling again says nothing new.
    #[test]
    fn an_outcome_is_reported_once() {
        let mut clipboard = Clipboard::new();
        assert!(!clipboard.busy());
        clipboard.job = Some(Job::Wrote);
        assert!(clipboard.busy());
        assert_eq!(clipboard.poll(), Some(Event::Copied(Ok(()))));
        assert_eq!(clipboard.poll(), None);
        assert!(!clipboard.busy());

        clipboard.job = Some(Job::Read(Ok(Some("{\"version\":6}".into()))));
        assert_eq!(
            clipboard.poll(),
            Some(Event::Pasted(Ok(Some("{\"version\":6}".into()))))
        );
        clipboard.job = Some(Job::Read(Err("클립보드가 없습니다".into())));
        assert_eq!(
            clipboard.poll(),
            Some(Event::Pasted(Err("클립보드가 없습니다".into())))
        );
    }

    /// A read that has not answered yet keeps the job (the caller asks again next frame); a
    /// worker that vanished without an answer is reported instead of being dropped silently.
    #[test]
    fn a_running_read_waits_and_a_lost_worker_is_reported() {
        use std::sync::mpsc::channel;
        let mut clipboard = Clipboard::new();
        let (tx, rx) = channel();
        clipboard.job = Some(Job::Running(rx));
        assert_eq!(clipboard.poll(), None);
        assert!(clipboard.busy(), "the read is still running");
        tx.send(Ok(Some("기록".into()))).unwrap();
        assert_eq!(
            clipboard.poll(),
            Some(Event::Pasted(Ok(Some("기록".into()))))
        );

        let (tx, rx) = channel::<Result<Option<String>, String>>();
        drop(tx);
        clipboard.job = Some(Job::Running(rx));
        assert!(matches!(clipboard.poll(), Some(Event::Pasted(Err(_)))));
    }

    /// An empty clipboard text is "no text", not a save that failed to parse.
    #[test]
    fn an_empty_clipboard_is_no_text() {
        assert_eq!(non_empty(String::new()), None);
        assert_eq!(non_empty("x".into()), Some("x".into()));
    }

    /// A second request while one is still running is refused: its answer would be dropped,
    /// and the caller would wait forever.
    #[test]
    fn a_request_while_one_runs_is_refused() {
        use std::sync::mpsc::channel;
        let mut clipboard = Clipboard::new();
        let (_tx, rx) = channel();
        clipboard.job = Some(Job::Running(rx));
        assert!(clipboard.copy("기록").is_err());
        assert!(clipboard.paste().is_err());
        // The running request is still the one that will report.
        assert!(clipboard.busy());
        assert_eq!(clipboard.poll(), None);
    }
}
