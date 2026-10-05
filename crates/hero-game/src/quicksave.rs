//! Quick save (순간 저장): **F5** saves and **F9** loads, at any moment — in the middle of a
//! conversation, a choice, a battle animation or an enemy phase — like the suspend point of an
//! emulator. The save goes to a slot of its own ([`saves::SaveSlot::Quick`], the one chosen in the
//! settings: **F6** steps through them) so it never replaces the autosave or a manual save;
//! 이어하기 on the title screen picks whichever save is newest.
//!
//! # How a moment is saved
//!
//! The save is an ordinary [`SaveGame`]: the campaign, the battle in progress (if any) and, new
//! for quick saves, where a drama scene was ([`SceneResume`]). Screens are not serialised. Each
//! screen of the stack reports what it knows through [`Screen::resume_point`], bottom to top,
//! and [`snapshot`] merges the reports:
//!
//! | screen | reports |
//! |---|---|
//! | drama scene (node, ending or a battle's overlay) | its runner position and stage |
//! | battle | the [`BattleState`] and the scenes it had queued but not started |
//! | camp, settings, save/load ... | nothing: the campaign state says it all |
//!
//! Loading goes through [`crate::flow::Flow::Continue`] like any other save; `flow` rebuilds the
//! drama screen (and the battle beneath it) from the report.

use crate::app::{Ctx, Screen};
use crate::flow::{save_label, Session};
use crate::saves;
use hero_core::battle::BattleState;
use hero_core::pack::Pack;
use hero_core::save::{ResumeError, SaveGame, SceneResume};
use macroquad::prelude::KeyCode;

/// Key that quick saves.
pub const SAVE_KEY: KeyCode = KeyCode::F5;
/// Key that quick loads.
pub const LOAD_KEY: KeyCode = KeyCode::F9;
/// Key that picks the next quick save slot.
pub const SLOT_KEY: KeyCode = KeyCode::F6;

/// What one screen of the stack says about where the game is (see [`Screen::resume_point`]).
#[derive(Debug, Clone)]
pub enum ResumePoint {
    /// A drama scene played half-way.
    Scene(Box<SceneResume>),
    /// A battle in progress: its state, the scenes that were queued by the animation but had
    /// not started (the state has already moved past them) and, right after a quick load, the
    /// scene that was half-way and has not been shown again yet.
    Battle {
        state: Box<BattleState>,
        pending_scenes: Vec<String>,
        scene: Option<Box<SceneResume>>,
    },
    /// The screen cannot be saved right now; the text says why (shown as a toast).
    Unavailable(&'static str),
}

/// Input: the loaded pack, the running session and what each screen of the stack reported,
/// bottom to top. Output: the save to write, or why a quick save is not possible now.
///
/// Why merge bottom to top: an overlay (a scene over the battle) adds to what the screen below
/// reports, and when two screens report the same kind of thing the upper one is the newer.
/// Why the battle comes only from the reports and never from `session.battle`: the session's
/// copy is refreshed after each action, so it can lag behind the screen, and while a battle's
/// title card is up it still holds the untouched starting state — a save without a battle
/// report must load as "the battle has not started" and begin it from the top.
pub fn snapshot(
    pack: &Pack,
    session: &Session,
    points: Vec<ResumePoint>,
) -> Result<SaveGame, &'static str> {
    let mut battle = None;
    let mut pending_scenes = Vec::new();
    let mut scene = None;
    for point in points {
        match point {
            ResumePoint::Scene(s) => scene = Some(*s),
            ResumePoint::Battle {
                state,
                pending_scenes: pending,
                scene: resumed,
            } => {
                battle = Some(*state);
                pending_scenes = pending;
                if let Some(resumed) = resumed {
                    scene = Some(*resumed);
                }
            }
            ResumePoint::Unavailable(why) => return Err(why),
        }
    }
    let mut save = session.to_save(pack);
    save.label = save_label(pack, &save.campaign, battle.as_ref());
    save.battle = battle;
    save.scene = scene;
    save.pending_scenes = pending_scenes;
    save.stamp_version();
    Ok(save)
}

/// Input: the context and the screen stack. Output: `Ok` once the save is in the quick slot,
/// else a message for a toast.
///
/// Why the checks live here and not in the app: the stack is the only thing that knows what is
/// on screen, but the app owns it, so it passes it in and keeps this logic testable.
pub fn save(ctx: &mut Ctx, stack: &[Box<dyn Screen>]) -> Result<(), String> {
    let (Some(pack), Some(session)) = (ctx.pack.clone(), ctx.session.as_ref()) else {
        return Err("진행 중인 게임이 없습니다".into());
    };
    let points = stack.iter().filter_map(|s| s.resume_point(ctx)).collect();
    let save = snapshot(&pack, session, points).map_err(str::to_string)?;
    let slot = ctx.settings.quick_save_slot();
    saves::write(ctx.storage.as_mut(), slot, &save).map_err(|e| e.to_string())
}

/// Input: the context. Output: the quick save of the loaded pack that can be played, or a
/// message for a toast (nothing saved yet, unreadable, another pack, made with another version
/// of the pack).
pub fn read(ctx: &Ctx) -> Result<SaveGame, String> {
    let (Some(pack), Some(pack_id)) = (ctx.pack.as_deref(), ctx.pack_id()) else {
        return Err("데이터 팩이 로드되지 않았습니다".into());
    };
    let save = saves::read(
        ctx.storage.as_ref(),
        ctx.settings.quick_save_slot(),
        pack_id,
    )
    .map_err(|e| e.to_string())?;
    playable(pack, &save)?;
    Ok(save)
}

/// Input: the loaded pack and a save about to be continued. Output: `Ok`, or the reason in
/// words for a toast or error screen.
///
/// Why the callers check before switching screens: a save that cannot be played must leave the
/// running game (or the title screen) untouched, not tear the screen stack down first. See
/// [`SaveGame::check_resume`] for why such a save is refused and not repaired.
pub fn playable(pack: &Pack, save: &SaveGame) -> Result<(), String> {
    save.check_resume(pack).map_err(|e| describe(&e))
}

/// The player-facing text of a [`ResumeError`].
pub fn describe(error: &ResumeError) -> String {
    match error {
        ResumeError::PackVersion { saved, current } => format!(
            "장면 도중의 기록은 그 장면이 바뀐 데이터 팩에서 이어 할 수 없습니다 (기록 {saved}, 현재 {current})"
        ),
        ResumeError::SceneChanged(scene) => {
            format!("기록된 장면 `{scene}`이(가) 데이터 팩에서 바뀌었거나 없어졌습니다")
        }
        ResumeError::WrongNode(_) | ResumeError::NoBattle | ResumeError::UnexpectedBattle => {
            "기록이 데이터 팩과 맞지 않습니다".into()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hero_core::campaign::CampaignState;
    use hero_core::drama::DramaRunner;
    use hero_core::save::{SceneKind, PLAIN_SAVE_VERSION, SCENE_SAVE_VERSION};
    use std::collections::BTreeMap;

    fn scene() -> SceneResume {
        SceneResume {
            kind: SceneKind::Node,
            runner: DramaRunner {
                scene: "c1_jade_belt".into(),
                pc: 3,
                pending_choice: None,
                finished: false,
            },
            stage: Vec::new(),
            shown: None,
            last_text: None,
            choice: None,
            bgm: None,
            terrain: BTreeMap::new(),
            backlog: Vec::new(),
            fingerprint: None,
        }
    }

    fn fixtures() -> (Pack, Session) {
        let pack = crate::screens::camp::test_pack();
        let session = Session::new(CampaignState::new_game(&pack));
        (pack, session)
    }

    #[test]
    fn a_plain_moment_is_a_plain_save() {
        let (pack, session) = fixtures();
        let save = snapshot(&pack, &session, Vec::new()).unwrap();
        assert!(save.scene.is_none() && save.battle.is_none());
        assert!(save.pending_scenes.is_empty());
        // Nothing the older games cannot read, so they still can.
        assert_eq!(save.version, PLAIN_SAVE_VERSION);
    }

    #[test]
    fn a_scene_report_is_kept_and_needs_the_new_version() {
        let (pack, session) = fixtures();
        let save = snapshot(&pack, &session, vec![ResumePoint::Scene(Box::new(scene()))]).unwrap();
        assert_eq!(save.scene, Some(scene()));
        assert_eq!(save.version, SCENE_SAVE_VERSION);
    }

    #[test]
    fn the_upper_scene_wins() {
        let (pack, session) = fixtures();
        let mut upper = scene();
        upper.kind = SceneKind::Overlay;
        let save = snapshot(
            &pack,
            &session,
            vec![
                ResumePoint::Scene(Box::new(scene())),
                ResumePoint::Scene(Box::new(upper.clone())),
            ],
        )
        .unwrap();
        assert_eq!(save.scene, Some(upper));
    }

    /// The session keeps the starting state of a battle whose title card is still up; without a
    /// battle report the save must not claim to be mid-battle.
    #[test]
    fn a_stale_session_battle_is_not_saved() {
        let (pack, mut session) = fixtures();
        let campaign = session.campaign.clone();
        session.battle =
            Some(BattleState::new(&pack, "p1_sishui", &campaign, 7).expect("battle builds"));
        let save = snapshot(&pack, &session, Vec::new()).unwrap();
        assert!(save.battle.is_none());
    }

    #[test]
    fn a_battle_report_is_saved_with_its_queued_scenes() {
        let (pack, session) = fixtures();
        let campaign = session.campaign.clone();
        let state = BattleState::new(&pack, "p1_sishui", &campaign, 7).expect("battle builds");
        let save = snapshot(
            &pack,
            &session,
            vec![ResumePoint::Battle {
                state: Box::new(state.clone()),
                pending_scenes: vec!["p1_after".into()],
                scene: None,
            }],
        )
        .unwrap();
        assert_eq!(save.battle, Some(state));
        assert_eq!(save.pending_scenes, vec!["p1_after".to_string()]);
        assert_eq!(save.version, SCENE_SAVE_VERSION);
        // The label names the battle and the turn, as a manual save made in it would.
        assert!(save.label.contains(" · "), "{}", save.label);
    }

    #[test]
    fn an_unavailable_screen_refuses_the_save() {
        let (pack, session) = fixtures();
        let points = vec![
            ResumePoint::Scene(Box::new(scene())),
            ResumePoint::Unavailable("장면이 준비되지 않았습니다"),
        ];
        assert_eq!(
            snapshot(&pack, &session, points).unwrap_err(),
            "장면이 준비되지 않았습니다"
        );
    }
}
