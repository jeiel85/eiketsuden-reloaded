//! `DramaRunner`: presentation steps, choices, flags, `@if` and side effects.

mod common;

use common::*;
use hero_core::campaign::{CampaignError, CampaignState, Growth};
use hero_core::drama::{DramaError, DramaRunner, Step};
use hero_core::pack::Pack;
use hero_core::script::Slot;

fn line(speaker: &str, portrait: Option<&str>, text: &str) -> Step {
    Step::Line {
        speaker: speaker.into(),
        portrait: portrait.map(Into::into),
        text: text.into(),
    }
}

/// Steps until (and including) the next `Choice` or `End`.
fn run_until_pause(
    runner: &mut DramaRunner,
    pack: &Pack,
    campaign: &mut CampaignState,
) -> Vec<Step> {
    let mut steps = Vec::new();
    loop {
        let step = runner.next(pack, campaign).expect("drama step");
        let pause = matches!(step, Step::Choice(_) | Step::End);
        steps.push(step);
        if pause {
            return steps;
        }
    }
}

/// Load the fixture with an extra drama file holding `src`.
fn pack_with_scene(src: &str) -> Pack {
    let mut files = fixture_files();
    append(&mut files, "dramas/battles.drama", src);
    load(&files)
}

#[test]
fn oath_up_to_the_choice() {
    let pack = load_fixture();
    let mut campaign = CampaignState::new_game(&pack);
    let mut runner = DramaRunner::new(&pack, "oath").unwrap();
    let steps = run_until_pause(&mut runner, &pack, &mut campaign);
    assert_eq!(
        steps,
        [
            Step::Background(Some("village".into())),
            Step::Music(Some("peace".into())),
            Step::Title("제1장 맹세".into()),
            Step::Narration("난세가 시작되었다.\n작은 마을에 세 사람이 모였다.".into()),
            Step::Show {
                portrait: "liu_bei".into(),
                slot: Slot::Left
            },
            // `@show 장비` finds the officer by display name.
            Step::Show {
                portrait: "zhang_fei".into(),
                slot: Slot::Right
            },
            line("유비", Some("liu_bei"), "함께 백성을 지키자."),
            line("장비", Some("zhang_fei"), "좋소, 형님!"),
            Step::Choice(vec!["적을 끝까지 쫓는다".into(), "마을을 지킨다".into()]),
        ]
    );
    assert_eq!(
        runner.next(&pack, &mut campaign),
        Err(DramaError::ChoicePending)
    );
    assert_eq!(runner.choose(&pack, 2), Err(DramaError::BadChoice(2)));
    assert!(
        runner.pending_choice.is_some(),
        "a bad index keeps the choice open"
    );
}

#[test]
fn pursue_choice_skips_the_gift() {
    let pack = load_fixture();
    let mut campaign = CampaignState::new_game(&pack);
    let mut runner = DramaRunner::new(&pack, "oath").unwrap();
    run_until_pause(&mut runner, &pack, &mut campaign);
    runner.choose(&pack, 0).unwrap();
    let steps = run_until_pause(&mut runner, &pack, &mut campaign);
    assert_eq!(
        steps,
        [
            Step::Joined {
                officer: "jian_yong".into(),
                name: "간옹".into(),
                returned: false
            },
            // The officer's own portrait key, not the id.
            line("간옹", Some("jianyong"), "저도 힘을 보태겠습니다."),
            Step::Hide(None),
            Step::FadeOut,
            Step::End,
        ]
    );
    assert_eq!(campaign.flag("pursue"), 1);
    assert!(campaign.officer("jian_yong").is_some());
    assert_eq!((campaign.gold, campaign.item_count("bean")), (500, 3));
    assert!(runner.finished);
    assert_eq!(
        runner.next(&pack, &mut campaign),
        Ok(Step::End),
        "End repeats"
    );
}

#[test]
fn guard_choice_receives_the_gift() {
    let pack = load_fixture();
    let mut campaign = CampaignState::new_game(&pack);
    let mut runner = DramaRunner::new(&pack, "oath").unwrap();
    run_until_pause(&mut runner, &pack, &mut campaign);
    runner.choose(&pack, 1).unwrap();
    let steps = run_until_pause(&mut runner, &pack, &mut campaign);
    assert_eq!(
        steps[0],
        Step::Received {
            gold: 100,
            item: None
        }
    );
    assert_eq!(
        steps[1],
        Step::Received {
            gold: 0,
            item: Some("bean".into())
        }
    );
    assert!(matches!(steps[2], Step::Joined { .. }));
    assert_eq!(campaign.flag("pursue"), 0);
    assert_eq!((campaign.gold, campaign.item_count("bean")), (600, 4));
}

#[test]
fn an_officer_back_from_away_returns() {
    let pack = load_fixture();
    let mut campaign = CampaignState::new_game(&pack);
    campaign.join(&pack, "jian_yong").unwrap();
    campaign.set_away("jian_yong").unwrap();
    let mut runner = DramaRunner::new(&pack, "oath").unwrap();
    run_until_pause(&mut runner, &pack, &mut campaign);
    runner.choose(&pack, 0).unwrap();
    let steps = run_until_pause(&mut runner, &pack, &mut campaign);
    assert!(
        steps.contains(&Step::Joined {
            officer: "jian_yong".into(),
            name: "간옹".into(),
            returned: true
        }),
        "{steps:?}"
    );
    assert!(!campaign.officer("jian_yong").unwrap().away);
}

#[test]
fn joining_an_army_member_shows_nothing() {
    let pack = load_fixture();
    let mut campaign = CampaignState::new_game(&pack);
    campaign.join(&pack, "jian_yong").unwrap();
    let mut runner = DramaRunner::new(&pack, "oath").unwrap();
    run_until_pause(&mut runner, &pack, &mut campaign);
    runner.choose(&pack, 0).unwrap();
    let steps = run_until_pause(&mut runner, &pack, &mut campaign);
    assert!(
        !steps.iter().any(|s| matches!(s, Step::Joined { .. })),
        "{steps:?}"
    );
    assert_eq!(campaign.roster.len(), 4);
}

#[test]
fn flags_and_conditions() {
    let pack = pack_with_scene(
        "\n== maths\n@set a = 5\n@set a += 3\n@set a -= 10\n@if a >= 0 -> positive\n@narr negative\n@goto done\n@label positive\n@narr positive\n@label done\n@if missing -> never\n@narr end\n@end\n@label never\n@narr unreachable\n",
    );
    let mut campaign = CampaignState::new_game(&pack);
    let mut runner = DramaRunner::new(&pack, "maths").unwrap();
    let steps = run_until_pause(&mut runner, &pack, &mut campaign);
    assert_eq!(
        steps,
        [
            Step::Narration("negative".into()),
            Step::Narration("end".into()),
            Step::End
        ]
    );
    assert_eq!(campaign.flag("a"), -2);
    assert!(
        !campaign.flags.contains_key("missing"),
        "reading a flag does not create it"
    );
}

#[test]
fn a_story_raises_levels_and_changes_classes() {
    let pack = pack_with_scene(
        "\n== growth\n@level guan_yu 3\n@level liu_bei 200\n@class guan_yu archer\n@level jian_yong 2\n",
    );
    let mut campaign = CampaignState::new_game(&pack);
    campaign.add_item("bronze_sword", 1);
    campaign.equip(&pack, "guan_yu", "bronze_sword").unwrap();
    let level = campaign.officer("guan_yu").unwrap().level;
    let mut runner = DramaRunner::new(&pack, "growth").unwrap();
    // Nothing to show: the changes happen as the scene goes.
    assert_eq!(
        run_until_pause(&mut runner, &pack, &mut campaign),
        [Step::End]
    );
    let guan_yu = campaign.officer("guan_yu").unwrap();
    assert_eq!(
        (guan_yu.level, guan_yu.class.as_str()),
        (level + 3, "archer")
    );
    // The sword is not for archers: back to the inventory.
    assert_eq!(guan_yu.equip.weapon, None);
    assert_eq!(campaign.item_count("bronze_sword"), 1);
    // Up to the level cap; an officer not in the army is left out.
    assert_eq!(
        campaign.officer("liu_bei").unwrap().level,
        pack.rules.level_cap
    );
    assert!(campaign.officer("jian_yong").is_none());
    // One already above the cap (a pack's cap lowered) keeps their level.
    let cap = pack.rules.level_cap;
    campaign.officer_mut("guan_yu").unwrap().level = cap + 5;
    campaign.add_levels(&pack, "guan_yu", 1).unwrap();
    assert_eq!(campaign.officer("guan_yu").unwrap().level, cap + 5);
}

#[test]
fn growth_of_an_officer_not_in_the_army_waits_for_their_join() {
    let pack = pack_with_scene(
        "\n== growth\n@level jian_yong 2\n@level jian_yong 3\n@class jian_yong archer\n",
    );
    let def = pack.officer("jian_yong").unwrap().clone();
    let mut campaign = CampaignState::new_game(&pack);
    let mut runner = DramaRunner::new(&pack, "growth").unwrap();
    assert_eq!(
        run_until_pause(&mut runner, &pack, &mut campaign),
        [Step::End]
    );
    assert!(campaign.officer("jian_yong").is_none());
    assert_eq!(
        campaign.pending_growth["jian_yong"],
        Growth {
            levels: 5,
            class: Some("archer".into())
        }
    );
    // The save keeps it and reads back the same.
    let saved = serde_json::to_string(&campaign).unwrap();
    assert_eq!(
        serde_json::from_str::<CampaignState>(&saved).unwrap(),
        campaign
    );
    // They join with it, once.
    campaign.join(&pack, "jian_yong").unwrap();
    let joined = campaign.officer("jian_yong").unwrap();
    assert_eq!(
        (joined.level, joined.class.as_str()),
        (def.level + 5, "archer")
    );
    assert!(campaign.pending_growth.is_empty());
    // An officer the pack does not have is still no one to grow.
    assert_eq!(
        campaign.add_levels(&pack, "nobody", 1),
        Err(CampaignError::NotInArmy("nobody".into()))
    );
    assert_eq!(
        campaign.set_class(&pack, "nobody", "archer"),
        Err(CampaignError::NotInArmy("nobody".into()))
    );
    // A save without any writes no field, and an older one reads without it.
    let plain = serde_json::to_string(&CampaignState::new_game(&pack)).unwrap();
    assert!(!plain.contains("pending_growth"), "{plain}");
    let back: CampaignState = serde_json::from_str(&plain).unwrap();
    assert!(back.pending_growth.is_empty());
}

#[test]
fn queued_growth_is_capped_at_the_join_and_used_once() {
    let pack = pack_with_scene("\n== none\n@gold +1\n");
    let def = pack.officer("jian_yong").unwrap().clone();
    let mut state = CampaignState::new_game(&pack);
    state.add_levels(&pack, "jian_yong", 1000).unwrap();
    // What is kept waiting is no more than the cap.
    assert_eq!(
        state.pending_growth["jian_yong"].levels,
        pack.rules.level_cap
    );
    state.join(&pack, "jian_yong").unwrap();
    assert_eq!(
        state.officer("jian_yong").unwrap().level,
        pack.rules.level_cap
    );
    // Leaving and joining again starts from the officer's definition: the growth was used.
    state.leave("jian_yong").unwrap();
    state.join(&pack, "jian_yong").unwrap();
    assert_eq!(state.officer("jian_yong").unwrap().level, def.level);
    // Growth is kept in the order it came, whatever the officer's level is by the join.
    state.leave("jian_yong").unwrap();
    state.add_levels(&pack, "jian_yong", 1).unwrap();
    state.add_levels(&pack, "jian_yong", 1).unwrap();
    assert_eq!(state.pending_growth["jian_yong"].levels, 2);
}

#[test]
fn side_effects_follow_the_campaign_rules() {
    let pack = pack_with_scene("\n== effects\n@leave jian_yong\n@leave zhang_fei\n@gold +100\n@gold -700\n@wait 250\n@sfx confirm\n@bgm stop\n@bg none\n@hide left\n@fade in\n");
    let mut campaign = CampaignState::new_game(&pack);
    campaign.gold = 9950;
    let mut runner = DramaRunner::new(&pack, "effects").unwrap();
    let steps = run_until_pause(&mut runner, &pack, &mut campaign);
    assert_eq!(
        steps,
        [
            // Leaving an officer who is not in the army is silently skipped.
            Step::Received {
                gold: 49,
                item: None
            },
            Step::Received {
                gold: -700,
                item: None
            },
            Step::Wait { ms: 250 },
            Step::Sound("confirm".into()),
            Step::Music(None),
            Step::Background(None),
            Step::Hide(Some(Slot::Left)),
            Step::FadeIn,
            Step::End,
        ]
    );
    assert!(campaign.officer("zhang_fei").is_none());
    assert_eq!(campaign.gold, 9999 - 700);
}

#[test]
fn unknown_ids_are_errors() {
    let pack = pack_with_scene("\n== bad_item\n@item peach\n\n== bad_join\n@join cao_cao\n");
    let mut campaign = CampaignState::new_game(&pack);
    let mut runner = DramaRunner::new(&pack, "bad_item").unwrap();
    assert_eq!(
        runner.next(&pack, &mut campaign),
        Err(DramaError::Campaign(CampaignError::UnknownItem(
            "peach".into()
        )))
    );
    let mut runner = DramaRunner::new(&pack, "bad_join").unwrap();
    assert_eq!(
        runner.next(&pack, &mut campaign),
        Err(DramaError::Campaign(CampaignError::UnknownOfficer(
            "cao_cao".into()
        )))
    );
    assert_eq!(
        DramaRunner::new(&pack, "nowhere"),
        Err(DramaError::UnknownScene("nowhere".into()))
    );
}

#[test]
fn free_speakers_keep_their_text() {
    let pack = pack_with_scene("\n== messenger\n전령: 급보요!\n@show 전령 center\n");
    let mut campaign = CampaignState::new_game(&pack);
    let mut runner = DramaRunner::new(&pack, "messenger").unwrap();
    assert_eq!(
        runner.next(&pack, &mut campaign),
        Ok(line("전령", None, "급보요!"))
    );
    assert_eq!(
        runner.next(&pack, &mut campaign),
        Ok(Step::Show {
            portrait: "전령".into(),
            slot: Slot::Center
        })
    );
}

#[test]
fn endless_goto_loop_ends_the_scene() {
    let pack = pack_with_scene("\n== spin\n@label top\n@set turns += 1\n@goto top\n");
    let mut campaign = CampaignState::new_game(&pack);
    let mut runner = DramaRunner::new(&pack, "spin").unwrap();
    assert_eq!(runner.next(&pack, &mut campaign), Ok(Step::End));
    assert!(runner.finished);
    assert!(campaign.flag("turns") > 1000);
}

#[test]
fn choose_without_a_choice() {
    let pack = load_fixture();
    let mut runner = DramaRunner::new(&pack, "mercy").unwrap();
    assert_eq!(runner.choose(&pack, 0), Err(DramaError::NoChoicePending));
}

#[test]
fn runner_resumes_from_json() {
    let pack = load_fixture();
    let mut campaign = CampaignState::new_game(&pack);
    let mut runner = DramaRunner::new(&pack, "oath").unwrap();
    run_until_pause(&mut runner, &pack, &mut campaign);
    let saved = serde_json::to_string(&runner).unwrap();
    let mut resumed: DramaRunner = serde_json::from_str(&saved).unwrap();
    assert_eq!(resumed, runner);
    resumed.choose(&pack, 1).unwrap();
    assert_eq!(
        resumed.next(&pack, &mut campaign),
        Ok(Step::Received {
            gold: 100,
            item: None
        })
    );
}
