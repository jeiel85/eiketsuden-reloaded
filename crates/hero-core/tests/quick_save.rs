//! A save made in the middle of a drama scene (`SceneResume`, the quick save) must continue the
//! scene exactly where it was: the same remaining steps and the same campaign at the end, with
//! no side effect (`@gold`, `@item`, `@join`) applied twice or skipped.

mod common;

use common::*;
use hero_core::campaign::CampaignState;
use hero_core::drama::{DramaRunner, Step};
use hero_core::pack::Pack;
use hero_core::save::{
    ResumeError, SaveGame, SceneKind, SceneResume, PLAIN_SAVE_VERSION, SCENE_SAVE_VERSION,
};
use std::collections::BTreeMap;

/// Everything `DramaRunner` reports from now on until the scene ends, choosing `pick` at each
/// choice (clamped to the options offered).
fn play_out(
    runner: &mut DramaRunner,
    pack: &Pack,
    campaign: &mut CampaignState,
    pick: usize,
) -> Vec<Step> {
    let mut steps = Vec::new();
    loop {
        if runner.pending_choice.is_some() {
            let n = runner.pending_choice.as_ref().unwrap().len();
            runner.choose(pack, pick.min(n - 1)).expect("choose");
        }
        let step = runner.next(pack, campaign).expect("drama step");
        let end = step == Step::End;
        steps.push(step);
        if end {
            return steps;
        }
    }
}

fn resume_of(runner: &DramaRunner, choice: Option<Vec<String>>) -> SceneResume {
    SceneResume {
        kind: SceneKind::Node,
        runner: runner.clone(),
        stage: Vec::new(),
        shown: None,
        last_text: None,
        choice,
        bgm: None,
        terrain: BTreeMap::new(),
        backlog: Vec::new(),
        fingerprint: None,
        playing: None,
    }
}

/// Write a save holding `campaign` and the scene record to JSON and read it back, as a quick
/// save and a quick load do.
fn through_a_save(campaign: &CampaignState, resume: SceneResume) -> (CampaignState, SceneResume) {
    let mut save = SaveGame {
        version: PLAIN_SAVE_VERSION,
        pack_id: "mini".into(),
        pack_version: "0.0.1".into(),
        label: "test".into(),
        saved_at: 1,
        campaign: campaign.clone(),
        battle: None,
        scene: Some(resume),
        pending_scenes: Vec::new(),
        battle_replay: None,
    };
    save.stamp_version();
    assert_eq!(save.version, SCENE_SAVE_VERSION);
    let back = SaveGame::from_json(&save.to_json(), "mini").expect("the save loads");
    (back.campaign, back.scene.expect("the scene record"))
}

/// Save after every possible number of steps of the `oath` scene (before, in and after the
/// choice, on either answer) and continue from the loaded save: the rest of the scene and the
/// campaign at its end must equal those of playing straight through.
#[test]
fn continuing_a_saved_scene_equals_playing_it_through() {
    let pack = load_fixture();
    let start = CampaignState::new_game(&pack);

    // How many steps the scene has (for either answer): the cut points to try.
    let mut total = 0;
    for pick in 0..2 {
        let mut c = start.clone();
        let mut r = DramaRunner::new(&pack, "oath").unwrap();
        total = total.max(play_out(&mut r, &pack, &mut c, pick).len());
    }
    assert!(
        total > 10,
        "the fixture scene is long enough to be worth cutting"
    );

    for pick in 0..2 {
        for cut in 0..=total {
            // The live game: `cut` steps, then on to the end.
            let mut campaign = start.clone();
            let mut runner = DramaRunner::new(&pack, "oath").unwrap();
            let mut seen = 0;
            let mut choice_texts = None;
            let mut done = false;
            while seen < cut && !done {
                if runner.pending_choice.is_some() {
                    choice_texts = None;
                    runner.choose(&pack, pick).expect("choose");
                }
                match runner.next(&pack, &mut campaign).expect("step") {
                    Step::Choice(options) => choice_texts = Some(options),
                    Step::End => done = true,
                    _ => {}
                }
                seen += 1;
            }
            let choice = runner.pending_choice.as_ref().and(choice_texts.clone());

            let (saved_campaign, saved_scene) =
                through_a_save(&campaign, resume_of(&runner, choice));
            assert!(
                saved_scene.fits(&pack),
                "cut {cut}: the record fits its pack"
            );

            let mut live_rest_campaign = campaign;
            let live_rest = play_out(&mut runner, &pack, &mut live_rest_campaign, pick);

            let mut loaded_campaign = saved_campaign;
            let mut loaded_runner = saved_scene.runner;
            let loaded_rest = play_out(&mut loaded_runner, &pack, &mut loaded_campaign, pick);

            assert_eq!(
                loaded_rest, live_rest,
                "pick {pick}, cut {cut}: remaining steps"
            );
            assert_eq!(
                loaded_campaign, live_rest_campaign,
                "pick {pick}, cut {cut}: campaign at the end"
            );
        }
    }
}

#[test]
fn a_record_that_no_longer_fits_the_pack_is_detected() {
    let pack = load_fixture();
    let runner = DramaRunner::new(&pack, "oath").unwrap();
    let len = pack.scene("oath").unwrap().cmds.len();

    let mut ok = resume_of(&runner, None);
    ok.runner.pc = len; // about to end: fine
    assert!(ok.fits(&pack));

    // The scene was rewritten shorter since the save.
    let mut past_the_end = ok.clone();
    past_the_end.runner.pc = len + 1;
    assert!(!past_the_end.fits(&pack));

    // The scene is gone.
    let mut gone = ok.clone();
    gone.runner.scene = "no_such_scene".into();
    assert!(!gone.fits(&pack));

    // A choice on screen without the runner waiting for one (or the other way round).
    let mut half = ok.clone();
    half.choice = Some(vec!["a".into()]);
    assert!(!half.fits(&pack));
    let mut waiting = ok;
    waiting.runner.pending_choice = Some(vec!["a".into()]);
    assert!(!waiting.fits(&pack));
}

fn save_of(pack: &Pack, node: &str, scene: Option<SceneResume>) -> SaveGame {
    let mut campaign = CampaignState::new_game(pack);
    campaign.node = node.into();
    let mut save = SaveGame {
        version: PLAIN_SAVE_VERSION,
        pack_id: pack.manifest.id.clone(),
        pack_version: pack.manifest.version.clone(),
        label: "test".into(),
        saved_at: 1,
        campaign,
        battle: None,
        scene,
        pending_scenes: Vec::new(),
        battle_replay: None,
    };
    save.stamp_version();
    save
}

/// A scene record is refused, never replayed from the node's start: the saved campaign already
/// holds the side effects of the steps before the position, so playing the scene again from its
/// first line would apply `@gold`, `@item` and `@set` a second time.
#[test]
fn a_scene_record_of_another_pack_version_or_node_is_refused() {
    let pack = load_fixture();
    let runner = DramaRunner::new(&pack, "oath").unwrap();
    let record = resume_of(&runner, None);

    // Made against this very pack, at the node that plays the scene: fine.
    let good = save_of(&pack, "prologue", Some(record.clone()));
    assert_eq!(good.check_resume(&pack), Ok(()));
    // No record: nothing to check, whatever the version.
    let mut plain = save_of(&pack, "prologue", None);
    plain.pack_version = "0.0.0-old".into();
    assert_eq!(plain.check_resume(&pack), Ok(()));

    // The pack was updated since the save.
    let mut old = good.clone();
    old.pack_version = "0.0.0-old".into();
    assert_eq!(
        old.check_resume(&pack),
        Err(ResumeError::PackVersion {
            saved: "0.0.0-old".into(),
            current: pack.manifest.version.clone(),
        })
    );

    // ...but a record that kept its scene's fingerprint plays on while the scene is the same
    // (another part of the pack changed), and is refused once the scene itself changed.
    let mut fingerprinted = old.clone();
    let scene = pack.scene("oath").unwrap();
    fingerprinted.scene.as_mut().unwrap().fingerprint = Some(scene.fingerprint());
    assert_eq!(fingerprinted.check_resume(&pack), Ok(()));
    let mut changed = fingerprinted.clone();
    changed.scene.as_mut().unwrap().fingerprint = Some("v1:0000000000000000".into());
    assert!(matches!(
        changed.check_resume(&pack),
        Err(ResumeError::PackVersion { .. })
    ));
    // It still has to fit the node and the scene.
    let mut elsewhere = fingerprinted.clone();
    elsewhere.campaign.node = "camp1".into();
    assert_eq!(
        elsewhere.check_resume(&pack),
        Err(ResumeError::WrongNode("camp1".into()))
    );

    // The node that played the scene is no longer where the campaign is, or is not a drama.
    assert_eq!(
        save_of(&pack, "mercy", Some(record.clone())).check_resume(&pack),
        Err(ResumeError::WrongNode("mercy".into()))
    );
    assert_eq!(
        save_of(&pack, "camp1", Some(record.clone())).check_resume(&pack),
        Err(ResumeError::WrongNode("camp1".into()))
    );

    // The scene was shortened since the save, or is gone.
    let mut past_the_end = record.clone();
    past_the_end.runner.pc = pack.scene("oath").unwrap().cmds.len() + 1;
    assert_eq!(
        save_of(&pack, "prologue", Some(past_the_end)).check_resume(&pack),
        Err(ResumeError::SceneChanged("oath".into()))
    );
    let mut gone = record.clone();
    gone.runner.scene = "no_such_scene".into();
    assert_eq!(
        save_of(&pack, "prologue", Some(gone)).check_resume(&pack),
        Err(ResumeError::SceneChanged("no_such_scene".into()))
    );

    // The ending scene belongs to the ending node only.
    let mut ending = resume_of(&DramaRunner::new(&pack, "epilogue").unwrap(), None);
    ending.kind = SceneKind::Ending {
        title: "끝".into()
    };
    assert_eq!(
        save_of(&pack, "finale", Some(ending.clone())).check_resume(&pack),
        Ok(())
    );
    assert!(save_of(&pack, "prologue", Some(ending))
        .check_resume(&pack)
        .is_err());

    // A scene over a battle needs the battle; a campaign scene must not come with one.
    let mut overlay = record;
    overlay.kind = SceneKind::Overlay;
    assert_eq!(
        save_of(&pack, "prologue", Some(overlay)).check_resume(&pack),
        Err(ResumeError::NoBattle)
    );
}
