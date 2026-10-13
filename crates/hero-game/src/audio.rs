//! Music and sound effects by key, with fades and volume settings.
//!
//! * [`Audio::play_bgm`] switches the music: the current track fades out, then the new one fades
//!   in (looping). Requesting the track that is already playing does nothing, so screens can
//!   call it every time they are entered. Music whose file marks a loop start
//!   ([`Media::intro`]) plays its intro once and then repeats the loop: the intro is a sound of
//!   its own, and the loop starts when the intro's length has passed on the clock (not by frame
//!   times, which are capped and stop while a browser tab is in the background), so the switch
//!   is at least a frame late and the two never overlap. While the music fades out during the
//!   intro the switch waits: it goes with the track once the fade ends, and happens late if
//!   the same music is wanted again before that. [`Audio::play_jingle`] plays a track once (victory,
//!   defeat). [`Audio::stop_bgm`] fades to silence.
//! * [`Audio::sfx`] plays an effect once (keys from `docs/ASSETS.md`, see [`sfx`]). Effects
//!   that are not loaded yet are skipped (and requested, so the next use plays); the engine
//!   effects are preloaded after the pack loads.
//! * Volumes come from [`Settings`] (master × music / master × effects).
//! * Browsers refuse to start audio before the first user gesture, so on the web the manager
//!   starts *locked*: music requests are remembered and start on the first key press, click or
//!   touch ([`Audio::update`] receives that signal from the input snapshot); effects before that
//!   are dropped.
//!
//! Missing audio files are logged by the media store and otherwise ignored — the game plays on
//! silently.

use crate::assets::{AssetState, Media};
use crate::settings::Settings;
use macroquad::audio::{play_sound, set_sound_volume, stop_sound, PlaySoundParams, Sound};
use macroquad::time::get_time;

/// Seconds for the music to fade out when it changes or stops.
pub const FADE_OUT: f32 = 0.6;
/// Seconds for new music to fade in.
pub const FADE_IN: f32 = 0.4;

/// Engine sound effect keys (`docs/ASSETS.md`).
pub mod sfx {
    pub const CURSOR: &str = "cursor";
    pub const CONFIRM: &str = "confirm";
    pub const CANCEL: &str = "cancel";
    pub const ERROR: &str = "error";
    pub const STEP: &str = "step";
    pub const HIT: &str = "hit";
    pub const HIT_HEAVY: &str = "hit_heavy";
    pub const ARROW: &str = "arrow";
    pub const FIRE: &str = "fire";
    pub const WATER: &str = "water";
    pub const ROCK: &str = "rock";
    pub const HEAL: &str = "heal";
    pub const MORALE_UP: &str = "morale_up";
    pub const MORALE_DOWN: &str = "morale_down";
    pub const CONFUSE: &str = "confuse";
    pub const LEVELUP: &str = "levelup";
    pub const RETREAT: &str = "retreat";
    pub const TREASURE: &str = "treasure";
    pub const PHASE: &str = "phase";
    pub const VICTORY: &str = "victory";
    pub const DEFEAT: &str = "defeat";

    /// Every engine effect, for preloading.
    pub const ALL: [&str; 21] = [
        CURSOR,
        CONFIRM,
        CANCEL,
        ERROR,
        STEP,
        HIT,
        HIT_HEAVY,
        ARROW,
        FIRE,
        WATER,
        ROCK,
        HEAL,
        MORALE_UP,
        MORALE_DOWN,
        CONFUSE,
        LEVELUP,
        RETREAT,
        TREASURE,
        PHASE,
        VICTORY,
        DEFEAT,
    ];
}

/// Engine music keys (`docs/ASSETS.md`).
pub mod bgm {
    pub const TITLE: &str = "title";
    pub const PEACE: &str = "peace";
    pub const TENSION: &str = "tension";
    pub const SAD: &str = "sad";
    pub const CAMP: &str = "camp";
    pub const BATTLE: &str = "battle";
    pub const ENEMY: &str = "enemy";
    pub const BOSS: &str = "boss";
    pub const VICTORY: &str = "victory";
    pub const DEFEAT: &str = "defeat";
    pub const ENDING: &str = "ending";
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Request {
    key: String,
    looped: bool,
}

struct Track {
    request: Request,
    sound: Sound,
    /// Fade level 0..=1.
    level: f32,
    applied_volume: f32,
    /// Its file was replaced ([`Audio::reload_bgm`]): fade it out and start the new one.
    stale: bool,
    /// While `sound` is the intro: the loop to play after it.
    then: Option<Handoff<Sound>>,
}

/// The loop that follows an intro: `next` starts at [`get_time`] `at`.
#[derive(Debug, Clone, PartialEq)]
struct Handoff<S> {
    next: S,
    at: f64,
}

/// Advance an intro's switch to its loop.
///
/// * Input: the track's pending switch, whether the track is still the wanted music (`keep`),
///   and the clock.
/// * Output: the loop to start now (`then` is cleared then): its time has come and the track is
///   wanted.
/// * Why: a pure function, so the timing can be tested without an audio device. Not wanted
///   means it is fading out for a change or a stop: starting its loop then would bring back
///   music that is on its way out. The switch is kept rather than dropped, because the same
///   music can be wanted again before the fade ends and the track fades back in; without its
///   loop it would go silent after the intro. A track that fades out completely is dropped
///   with its switch.
fn step_handoff<S>(then: &mut Option<Handoff<S>>, keep: bool, now: f64) -> Option<S> {
    if keep && then.as_ref().is_some_and(|h| now >= h.at) {
        return then.take().map(|h| h.next);
    }
    None
}

/// The music/effects manager. Owned by [`crate::app::Ctx`].
pub struct Audio {
    bgm_gain: f32,
    sfx_gain: f32,
    unlocked: bool,
    /// Music that should be playing (`None` = silence).
    wanted: Option<Request>,
    current: Option<Track>,
    /// Effects already started this frame (the same effect twice in a frame is one sound).
    played_this_frame: Vec<String>,
}

impl Audio {
    /// `unlocked` is `false` on the web until the first user gesture.
    pub fn new(settings: &Settings) -> Audio {
        Audio {
            bgm_gain: settings.bgm_gain(),
            sfx_gain: settings.sfx_gain(),
            unlocked: !crate::platform::is_web(),
            wanted: None,
            current: None,
            played_this_frame: Vec::new(),
        }
    }

    /// Apply changed volume settings (takes effect immediately).
    pub fn apply_settings(&mut self, settings: &Settings) {
        self.bgm_gain = settings.bgm_gain();
        self.sfx_gain = settings.sfx_gain();
    }

    /// Whether audio may play yet (always `true` natively).
    pub fn unlocked(&self) -> bool {
        self.unlocked
    }

    /// Play `bgm/<key>.ogg` looping, cross-fading from the current music.
    pub fn play_bgm(&mut self, key: &str) {
        self.wanted = Some(Request {
            key: key.to_string(),
            looped: true,
        });
    }

    /// Play `bgm/<key>.ogg` once (victory / defeat jingles).
    pub fn play_jingle(&mut self, key: &str) {
        self.wanted = Some(Request {
            key: key.to_string(),
            looped: false,
        });
        // A jingle restarts even if the same jingle played before.
        if let Some(t) = &self.current {
            if t.request.key == key {
                stop_sound(&t.sound);
                self.current = None;
            }
        }
    }

    /// The file of music `key` was replaced (the original mode adds its songs while the game
    /// runs): if it is playing as music, fade it out and start it again from the new file. A
    /// jingle plays on to its end (it is not started again).
    pub fn reload_bgm(&mut self, key: &str) {
        if let Some(track) = self.current.as_mut() {
            if track.request.key == key && track.request.looped {
                track.stale = true;
            }
        }
    }

    /// Fade the music out.
    pub fn stop_bgm(&mut self) {
        self.wanted = None;
    }

    /// Key of the music that is playing or about to play.
    pub fn bgm(&self) -> Option<&str> {
        self.wanted.as_ref().map(|r| r.key.as_str())
    }

    /// Play `sfx/<key>` once. Skipped while audio is locked or the effect is not loaded yet.
    pub fn sfx(&mut self, media: &Media, key: &str) {
        let full = format!("sfx/{key}");
        if !self.unlocked {
            // Still request it so it is ready once audio unlocks.
            media.sound_state(&full);
            return;
        }
        if self.sfx_gain <= 0.0 || self.played_this_frame.contains(&full) {
            return;
        }
        if let Some(sound) = media.sound(&full) {
            play_sound(
                &sound,
                PlaySoundParams {
                    looped: false,
                    volume: self.sfx_gain,
                },
            );
            self.played_this_frame.push(full);
        }
    }

    /// Advance fades and start pending music. `user_gesture` is the input snapshot's
    /// "any activity" flag (unlocks audio on the web). Called by the app once per frame.
    pub fn update(&mut self, dt: f32, media: &Media, user_gesture: bool) {
        self.played_this_frame.clear();
        if !self.unlocked {
            if !user_gesture {
                if let Some(w) = &self.wanted {
                    // Load the music meanwhile so it starts right after the gesture.
                    media.sound_state(&format!("bgm/{}", w.key));
                }
                return;
            }
            self.unlocked = true;
        }

        // Fade out music that is no longer wanted.
        let keep = matches!((&self.current, &self.wanted), (Some(t), Some(w)) if t.request == *w && !t.stale);
        // The loop after an intro.
        if let Some(track) = self.current.as_mut() {
            if let Some(next) = step_handoff(&mut track.then, keep, get_time()) {
                // The intro is over by the clock; stop it in case the device started it late.
                stop_sound(&track.sound);
                play_sound(
                    &next,
                    PlaySoundParams {
                        looped: track.request.looped,
                        volume: track.applied_volume,
                    },
                );
                track.sound = next;
            }
        }
        if let Some(track) = self.current.as_mut() {
            if !keep {
                track.level -= dt / FADE_OUT;
                if track.level <= 0.0 {
                    stop_sound(&track.sound);
                    let key = format!("bgm/{}", track.request.key);
                    let stale = track.stale;
                    self.current = None;
                    // Decoded music is large; drop it unless it is wanted again right away (and
                    // is still the same file).
                    if stale || self.bgm().map(|k| format!("bgm/{k}")) != Some(key.clone()) {
                        media.release_sound(&key);
                    }
                }
            } else if track.level < 1.0 {
                track.level = (track.level + dt / FADE_IN).min(1.0);
            }
        }

        // Start the wanted music once the old track is gone and the file is loaded.
        if self.current.is_none() {
            if let Some(w) = self.wanted.clone() {
                let key = format!("bgm/{}", w.key);
                match media.sound_state(&key) {
                    AssetState::Ready => {
                        if let Some(sound) = media.sound(&key) {
                            // With an intro, it plays once and the loop (a jingle: the rest)
                            // follows.
                            let (first, then) = match media.intro(&key) {
                                Some((intro, seconds)) => (
                                    intro,
                                    Some(Handoff {
                                        next: sound,
                                        at: get_time() + seconds,
                                    }),
                                ),
                                None => (sound, None),
                            };
                            play_sound(
                                &first,
                                PlaySoundParams {
                                    looped: w.looped && then.is_none(),
                                    volume: 0.0,
                                },
                            );
                            self.current = Some(Track {
                                request: w,
                                sound: first,
                                level: 0.0,
                                applied_volume: 0.0,
                                stale: false,
                                then,
                            });
                        }
                    }
                    AssetState::Loading => {}
                    AssetState::Missing => {
                        // Logged by the media store; play nothing for this request.
                        if !w.looped {
                            self.wanted = None;
                        }
                    }
                }
            }
        }

        if let Some(track) = self.current.as_mut() {
            let volume = track.level.clamp(0.0, 1.0) * self.bgm_gain;
            if (volume - track.applied_volume).abs() > 0.001 {
                set_sound_volume(&track.sound, volume);
                track.applied_volume = volume;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_loop_follows_the_intro_on_the_clock() {
        let mut then = Some(Handoff {
            next: "loop",
            at: 12.7,
        });
        assert_eq!(step_handoff(&mut then, true, 10.0), None);
        assert_eq!(step_handoff(&mut then, true, 12.69), None);
        assert!(then.is_some());
        // Due: once (a long frame or a tab back from the background starts it at once).
        assert_eq!(step_handoff(&mut then, true, 30.0), Some("loop"));
        assert_eq!(then, None);
        assert_eq!(step_handoff(&mut then, true, 31.0), None);
    }

    #[test]
    fn a_fade_out_during_the_intro_holds_its_loop() {
        let mut then = Some(Handoff {
            next: "loop",
            at: 12.7,
        });
        // Fading out: nothing starts, even when it is due.
        assert_eq!(step_handoff(&mut then, false, 10.0), None);
        assert_eq!(step_handoff(&mut then, false, 13.0), None);
        assert!(then.is_some());
        // Wanted again before the fade ended: the loop starts (late) instead of silence.
        assert_eq!(step_handoff(&mut then, true, 13.1), Some("loop"));
        // Without an intro there is nothing to switch to.
        let mut none: Option<Handoff<&str>> = None;
        assert_eq!(step_handoff(&mut none, true, 13.0), None);
    }
}
