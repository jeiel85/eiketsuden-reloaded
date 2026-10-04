//! Turn flow (§1), regeneration and confusion upkeep (§6, §7.5), weather (§8), battle events
//! and the outcome (§9, §11).

use crate::battle::testkit::*;
use crate::battle::{
    Action, ActionError, ActiveStatus, BattleEvent, BattleState, DefeatReason, MapImage, Outcome,
    UnitId, UnitState, Weather,
};
use crate::battledef::{
    AiMode, BattleDef, BonusDef, Condition, EventAction, EventDef, FlagCond, Side, Trigger,
    UnitSpawn,
};
use crate::data::{StatusKind, WeatherChances};
use crate::geom::Pos;
use crate::pack::Pack;

fn event(trigger: Trigger, actions: Vec<EventAction>) -> EventDef {
    EventDef {
        trigger,
        once: true,
        stage: None,
        when: Vec::new(),
        unless: Vec::new(),
        actions,
    }
}

fn spawn(side: Side, pos: Pos) -> UnitSpawn {
    UnitSpawn {
        side,
        officer: None,
        name: None,
        class: Some("infantry".into()),
        level: Some(1),
        stats: None,
        pos,
        ai: AiMode::Aggressive,
        ai_target: None,
        ai_pos: None,
        commander: false,
        tag: None,
        group: None,
        equip: None,
        drop: None,
    }
}

fn confused(turns: u8) -> ActiveStatus {
    ActiveStatus {
        status: StatusKind::Confused,
        turns,
    }
}

fn drama(scene: &str) -> BattleEvent {
    BattleEvent::Drama {
        scene: scene.into(),
    }
}

fn phase(side: Side, turn: u32) -> BattleEvent {
    BattleEvent::PhaseStart { side, turn }
}

fn end_phase(st: &mut BattleState, pack: &Pack) -> Vec<BattleEvent> {
    st.apply(pack, Action::EndPhase).unwrap()
}

fn tag(st: &mut BattleState, id: UnitId, tag: &str) {
    st.units[id].tag = Some(tag.into());
}

// ----- turn structure ----------------------------------------------------------------------

#[test]
fn phases_cycle_and_skip_sides_without_units() {
    let pack = pack(OPEN_MAP);
    let mut st = state(&pack);
    let me = add(&mut st, &pack, Side::Player, "infantry", 1, p(0, 0));
    let foe = add(&mut st, &pack, Side::Enemy, "infantry", 1, p(7, 7));
    assert_eq!(st.begin(&pack), vec![phase(Side::Player, 1)]);
    assert_eq!(
        st.next_ai_unit(),
        None,
        "no AI units during the player phase"
    );
    st.apply(&pack, Action::Wait { unit: me }).unwrap();
    assert!(!st.can_act(me));

    assert_eq!(
        end_phase(&mut st, &pack),
        vec![phase(Side::Enemy, 1)],
        "empty ally phase skipped"
    );
    assert_eq!(st.next_ai_unit(), Some(foe));
    assert_eq!(end_phase(&mut st, &pack), vec![phase(Side::Player, 2)]);
    assert_eq!(st.turn, 2);
    assert!(
        st.can_act(me),
        "flags are cleared at the start of the side's phase"
    );

    add(&mut st, &pack, Side::Ally, "infantry", 1, p(0, 7));
    assert_eq!(end_phase(&mut st, &pack), vec![phase(Side::Ally, 2)]);
    assert_eq!(end_phase(&mut st, &pack), vec![phase(Side::Enemy, 2)]);
}

#[test]
fn turn_limit_loses_after_the_last_enemy_phase() {
    let mut def = battle(OPEN_MAP);
    def.turn_limit = 2;
    let pack = pack_with(def);
    let mut st = state(&pack);
    let me = add(&mut st, &pack, Side::Player, "infantry", 1, p(0, 0));
    add(&mut st, &pack, Side::Enemy, "infantry", 1, p(7, 7));
    st.begin(&pack);
    for _ in 0..3 {
        end_phase(&mut st, &pack);
    }
    assert_eq!((st.turn, st.phase, st.outcome), (2, Side::Enemy, None));
    assert_eq!(
        end_phase(&mut st, &pack),
        vec![BattleEvent::Defeat(DefeatReason::TurnLimit)]
    );
    assert_eq!(st.outcome, Some(Outcome::Defeat(DefeatReason::TurnLimit)));
    assert!(st.is_over());
    assert_eq!(
        st.apply(&pack, Action::Wait { unit: me }),
        Err(ActionError::BattleOver)
    );
    assert_eq!(
        st.apply(&pack, Action::EndPhase),
        Err(ActionError::BattleOver)
    );
    assert_eq!(st.next_ai_unit(), None);
}

#[test]
fn surviving_the_last_turn_wins_before_the_turn_limit() {
    let mut def = battle(OPEN_MAP);
    def.turn_limit = 2;
    def.victory = vec![Condition::SurviveTurns { turns: 2 }];
    let pack = pack_with(def);
    let mut st = state(&pack);
    add(&mut st, &pack, Side::Player, "infantry", 1, p(0, 0));
    add(&mut st, &pack, Side::Enemy, "infantry", 1, p(7, 7));
    st.begin(&pack);
    for _ in 0..3 {
        end_phase(&mut st, &pack);
    }
    assert_eq!(st.outcome, None, "turn 2 is not completed yet");
    assert_eq!(end_phase(&mut st, &pack), vec![BattleEvent::Victory]);
    assert_eq!(st.outcome, Some(Outcome::Victory));
}

#[test]
fn turn_start_events_fire_for_empty_phases_and_reinforcements_join_them() {
    let mut def = battle(OPEN_MAP);
    let mut allies = spawn(Side::Ally, p(0, 7));
    allies.group = Some("allies".into());
    def.units = vec![spawn(Side::Enemy, p(7, 7)), allies];
    def.events = vec![
        event(
            Trigger::TurnStart {
                turn: 1,
                side: Side::Ally,
            },
            vec![EventAction::Drama {
                scene: "empty".into(),
            }],
        ),
        event(
            Trigger::TurnStart {
                turn: 2,
                side: Side::Ally,
            },
            vec![EventAction::Spawn {
                group: "allies".into(),
            }],
        ),
    ];
    let pack = pack_with(def);
    let mut st = state(&pack);
    add(&mut st, &pack, Side::Player, "infantry", 1, p(0, 0));
    st.begin(&pack);
    assert_eq!(
        end_phase(&mut st, &pack),
        vec![drama("empty"), phase(Side::Enemy, 1)]
    );
    end_phase(&mut st, &pack);
    assert_eq!(
        end_phase(&mut st, &pack),
        vec![
            BattleEvent::Spawned { units: vec![1] },
            phase(Side::Ally, 2)
        ]
    );
    assert_eq!(st.phase, Side::Ally);
    assert_eq!(st.next_ai_unit(), Some(1));
}

/// A `retreat` event on a unit still waiting in its reinforcement group takes it out of the
/// battle: the `spawn` after it does not bring it in, and nothing is shown for it.
#[test]
fn a_retreat_event_takes_out_a_reinforcement_before_it_arrives() {
    let mut def = battle(OPEN_MAP);
    let mut loser = spawn(Side::Enemy, p(7, 0));
    loser.group = Some("later".into());
    loser.tag = Some("loser".into());
    let mut other = spawn(Side::Enemy, p(7, 1));
    other.group = Some("later".into());
    def.units = vec![spawn(Side::Enemy, p(7, 7)), loser, other];
    def.events = vec![
        event(
            Trigger::TurnStart {
                turn: 1,
                side: Side::Player,
            },
            vec![EventAction::Retreat {
                target: "loser".into(),
            }],
        ),
        event(
            Trigger::TurnStart {
                turn: 2,
                side: Side::Player,
            },
            vec![EventAction::Spawn {
                group: "later".into(),
            }],
        ),
    ];
    let pack = pack_with(def);
    let mut st = state(&pack);
    add(&mut st, &pack, Side::Player, "infantry", 1, p(0, 0));
    assert_eq!(st.begin(&pack), vec![phase(Side::Player, 1)]);
    assert_eq!(st.units[1].state, UnitState::Retreated);
    end_phase(&mut st, &pack);
    let events = end_phase(&mut st, &pack);
    assert!(
        events.contains(&BattleEvent::Spawned { units: vec![2] }),
        "{events:?}"
    );
    assert_eq!(st.units[1].state, UnitState::Retreated);
    assert_eq!(st.units[2].state, UnitState::Active);
}

// ----- regeneration, confusion, weather ----------------------------------------------------

#[test]
fn terrain_equipment_and_band_regeneration() {
    let pack = pack(
        "
        .....
        .v...
        .....",
    );
    let mut st = state(&pack);
    let villager = add(&mut st, &pack, Side::Player, "infantry", 1, p(1, 1));
    let jade = add(&mut st, &pack, Side::Player, "infantry", 1, p(3, 1));
    add(&mut st, &pack, Side::Enemy, "band", 10, p(3, 0));
    add(&mut st, &pack, Side::Player, "infantry", 1, p(0, 2));
    let foe = add(&mut st, &pack, Side::Enemy, "infantry", 1, p(4, 2));
    st.units[villager].hp = 400;
    st.units[villager].morale = 50;
    st.units[jade].equip.accessory = Some("jade".into());
    st.units[jade].hp = 100;
    st.units[jade].mp = 0;
    st.units[foe].hp = 100;

    assert_eq!(
        st.begin(&pack),
        vec![
            phase(Side::Player, 1),
            // village: 10% of max HP and 10 morale
            BattleEvent::Regenerated {
                unit: villager,
                hp: 50,
                mp: 0,
                morale: 10
            },
            // jade: 10% HP (+5 morale, already full); enemy band Lv10 next to it: 10 / 10 + 1 MP
            BattleEvent::Regenerated {
                unit: jade,
                hp: 50,
                mp: 2,
                morale: 0
            },
        ]
    );
    assert_eq!(
        (st.units[villager].hp, st.units[villager].morale),
        (450, 60)
    );
    assert_eq!((st.units[jade].hp, st.units[jade].mp), (150, 2));
    assert_eq!(
        st.units[foe].hp, 100,
        "only the side whose phase starts regenerates"
    );

    // Healing is capped at the missing HP.
    st.units[villager].hp = 495;
    end_phase(&mut st, &pack);
    let ev = end_phase(&mut st, &pack);
    assert!(ev.contains(&BattleEvent::Regenerated {
        unit: villager,
        hp: 5,
        mp: 0,
        morale: 10
    }));
}

#[test]
fn confusion_counts_down_but_persists_while_morale_is_low() {
    let pack = pack(OPEN_MAP);
    let mut st = state(&pack);
    let calm = add(&mut st, &pack, Side::Player, "infantry", 1, p(0, 0));
    let shaken = add(&mut st, &pack, Side::Player, "infantry", 1, p(2, 0));
    let long = add(&mut st, &pack, Side::Player, "infantry", 1, p(4, 0));
    add(&mut st, &pack, Side::Enemy, "infantry", 1, p(7, 7));
    st.units[calm].statuses = vec![confused(1)];
    st.units[shaken].statuses = vec![confused(1)];
    st.units[shaken].morale = 20;
    st.units[long].statuses = vec![confused(3)];
    assert_eq!(
        st.begin(&pack),
        vec![
            phase(Side::Player, 1),
            BattleEvent::StatusExpired {
                unit: calm,
                status: StatusKind::Confused
            },
        ]
    );
    assert!(st.units[calm].statuses.is_empty());
    assert_eq!(
        st.units[shaken].statuses,
        vec![confused(1)],
        "kept at 1 while morale <= 30"
    );
    assert_eq!(st.units[long].statuses, vec![confused(2)]);
    assert!(st.can_act(calm));
}

#[test]
fn original_formulas_end_confusion_on_a_recovery_roll() {
    let mut pack = pack(OPEN_MAP);
    pack.rules.strategy_formulas = crate::data::StrategyFormulas::Original;
    let mut st = state(&pack);
    let bold = add(&mut st, &pack, Side::Player, "infantry", 1, p(0, 0));
    add(&mut st, &pack, Side::Enemy, "infantry", 1, p(7, 7));
    // (300 + 0) / 3 >= 100: always, and not confused again by its low morale: the original
    // has no phase-start confusion.
    st.units[bold].lead = 300;
    st.units[bold].morale = 0;
    st.units[bold].statuses = vec![confused(crate::battle::UNTIL_RECOVERED)];
    assert_eq!(
        st.begin(&pack),
        vec![
            phase(Side::Player, 1),
            BattleEvent::StatusExpired {
                unit: bold,
                status: StatusKind::Confused
            },
        ]
    );
    // (50 + 40) / 3 = 30 %, whatever the turns; kept as it was otherwise.
    let mut recovered = 0;
    for seed in 0..400 {
        let mut st = BattleState::new(&pack, BATTLE, &campaign(Vec::new(), &[]), seed).unwrap();
        let u = add(&mut st, &pack, Side::Player, "infantry", 1, p(0, 0));
        add(&mut st, &pack, Side::Enemy, "infantry", 1, p(7, 7));
        st.units[u].lead = 50;
        st.units[u].morale = 40;
        st.units[u].statuses = vec![confused(1)];
        st.begin(&pack);
        if st.units[u].statuses.is_empty() {
            recovered += 1;
        } else {
            assert_eq!(st.units[u].statuses, vec![confused(1)]);
        }
    }
    assert!((90..150).contains(&recovered), "{recovered} of 400");
}

#[test]
fn low_morale_confuses_and_a_confused_unit_at_zero_morale_retreats() {
    let pack = pack(OPEN_MAP);
    let mut st = state(&pack);
    let broken = add(&mut st, &pack, Side::Player, "infantry", 1, p(0, 0));
    let steady = add(&mut st, &pack, Side::Player, "infantry", 1, p(2, 0));
    add(&mut st, &pack, Side::Enemy, "infantry", 1, p(7, 7));
    st.units[broken].morale = 0;
    st.units[steady].morale = 31;
    assert_eq!(
        st.begin(&pack),
        vec![
            phase(Side::Player, 1),
            BattleEvent::Confused { unit: broken },
            BattleEvent::Retreated { unit: broken },
        ],
        "chance (30 - 0) * 3 + 10 >= 100"
    );
    assert!(st.units[steady].statuses.is_empty());

    // Morale 25: (30 - 25) * 3 + 10 = 25%.
    let mut hits = 0;
    for seed in 0..400 {
        let mut st = BattleState::new(&pack, BATTLE, &campaign(Vec::new(), &[]), seed).unwrap();
        let u = add(&mut st, &pack, Side::Player, "infantry", 1, p(0, 0));
        add(&mut st, &pack, Side::Enemy, "infantry", 1, p(7, 7));
        st.units[u].morale = 25;
        st.begin(&pack);
        if st.units[u].has_status(StatusKind::Confused) {
            assert_eq!(st.units[u].statuses, vec![confused(1)]);
            assert!(st.ai_actions(&pack, u).is_empty());
            hits += 1;
        }
    }
    assert!((60..140).contains(&hits), "25% of 400: {hits}");
}

#[test]
fn weather_is_rolled_at_every_player_phase() {
    let mut pack = pack(OPEN_MAP);
    pack.rules.weather = WeatherChances {
        clear: 0,
        cloudy: 0,
        rain: 100,
    };
    let mut st = state(&pack);
    add(&mut st, &pack, Side::Player, "infantry", 1, p(0, 0));
    add(&mut st, &pack, Side::Enemy, "infantry", 1, p(7, 7));
    assert_eq!(
        st.begin(&pack),
        vec![
            phase(Side::Player, 1),
            BattleEvent::WeatherChanged {
                weather: Weather::Rain
            }
        ]
    );
    assert_eq!(st.weather, Weather::Rain);
    end_phase(&mut st, &pack);
    assert_eq!(
        end_phase(&mut st, &pack),
        vec![phase(Side::Player, 2)],
        "unchanged: no event"
    );

    pack.rules.weather = WeatherChances {
        clear: 50,
        cloudy: 0,
        rain: 50,
    };
    let rainy = (0..200)
        .filter(|&seed| {
            let mut st = BattleState::new(&pack, BATTLE, &campaign(Vec::new(), &[]), seed).unwrap();
            add(&mut st, &pack, Side::Player, "infantry", 1, p(0, 0));
            st.begin(&pack);
            assert_ne!(st.weather, Weather::Cloudy);
            st.weather == Weather::Rain
        })
        .count();
    assert!((60..140).contains(&rainy), "50% of 200: {rainy}");
}

// ----- triggers and actions ----------------------------------------------------------------

#[test]
fn unit_defeated_trigger_runs_reward_actions() {
    let mut def = battle(OPEN_MAP);
    def.events = vec![event(
        Trigger::UnitDefeated {
            target: "boss".into(),
        },
        vec![
            EventAction::GiveGold { amount: 100 },
            EventAction::GiveItem {
                item: "bean".into(),
            },
            EventAction::SetFlag {
                flag: "boss_down".into(),
                value: 1,
            },
            EventAction::Drama {
                scene: "boss_falls".into(),
            },
        ],
    )];
    let pack = pack_with(def);
    let mut st = state(&pack);
    let me = add(&mut st, &pack, Side::Player, "infantry", 1, p(3, 3));
    let boss = add(&mut st, &pack, Side::Enemy, "infantry", 1, p(3, 4));
    add(&mut st, &pack, Side::Enemy, "infantry", 1, p(7, 7));
    tag(&mut st, boss, "boss");
    st.units[boss].hp = 1;
    st.begin(&pack);
    assert_eq!(st.fired, vec![false]);
    let ev = st
        .apply(
            &pack,
            Action::Attack {
                unit: me,
                target: boss,
            },
        )
        .unwrap();
    assert_eq!(
        &ev[1..],
        &[
            BattleEvent::Retreated { unit: boss },
            BattleEvent::ExpGained {
                unit: me,
                amount: 38
            },
            drama("boss_falls"),
        ]
    );
    assert_eq!(st.gold_found, 100);
    assert_eq!(st.items_found, vec!["bean".to_string()]);
    assert_eq!(st.flags.get("boss_down"), Some(&1));
    assert_eq!(st.fired, vec![true]);
}

#[test]
fn reach_trigger_watches_player_units_or_a_named_unit() {
    let mut def = battle(OPEN_MAP);
    def.events = vec![
        event(
            Trigger::Reach {
                who: None,
                pos: p(5, 5),
                radius: 1,
                to: None,
            },
            vec![EventAction::Drama {
                scene: "near".into(),
            }],
        ),
        event(
            Trigger::Reach {
                who: Some("runner".into()),
                pos: p(0, 7),
                radius: 0,
                to: None,
            },
            vec![EventAction::Drama {
                scene: "escaped".into(),
            }],
        ),
    ];
    let pack = pack_with(def);
    let mut st = state(&pack);
    let me = add(&mut st, &pack, Side::Player, "infantry", 1, p(5, 1));
    let runner = add(&mut st, &pack, Side::Enemy, "infantry", 1, p(1, 7));
    add(&mut st, &pack, Side::Enemy, "infantry", 1, p(5, 6));
    tag(&mut st, runner, "runner");
    assert_eq!(
        st.begin(&pack),
        vec![phase(Side::Player, 1)],
        "enemies do not count as `who = None`"
    );
    let ev = st
        .apply(
            &pack,
            Action::Move {
                unit: me,
                to: p(5, 4),
            },
        )
        .unwrap();
    assert_eq!(ev[1..], [drama("near")]);
    end_phase(&mut st, &pack);
    let ev = st
        .apply(
            &pack,
            Action::Move {
                unit: runner,
                to: p(0, 7),
            },
        )
        .unwrap();
    assert_eq!(ev[1..], [drama("escaped")]);
}

#[test]
fn adjacent_trigger_runs_a_duel() {
    let mut def = battle(OPEN_MAP);
    def.events = vec![event(
        Trigger::Adjacent {
            a: Some("liu".into()),
            b: "boss".into(),
        },
        vec![
            EventAction::Drama {
                scene: "duel".into(),
            },
            EventAction::Retreat {
                target: "boss".into(),
            },
            EventAction::LevelUp {
                target: "liu".into(),
                amount: 2,
            },
        ],
    )];
    let pack = pack_with(def);
    let mut st = state(&pack);
    let liu = add(&mut st, &pack, Side::Player, "infantry", 1, p(3, 1));
    let boss = add(&mut st, &pack, Side::Enemy, "infantry", 1, p(3, 4));
    add(&mut st, &pack, Side::Enemy, "infantry", 1, p(7, 7));
    tag(&mut st, liu, "liu");
    tag(&mut st, boss, "boss");
    st.units[boss].drop = Some("bean".into());
    st.units[boss].commander = true;
    st.begin(&pack);
    let ev = st
        .apply(
            &pack,
            Action::Move {
                unit: liu,
                to: p(3, 3),
            },
        )
        .unwrap();
    assert_eq!(
        ev[1..],
        [
            drama("duel"),
            BattleEvent::Retreated { unit: boss },
            BattleEvent::LevelUp {
                unit: liu,
                level: 2,
                hp_gain: 20,
                mp_gain: 1
            },
            BattleEvent::LevelUp {
                unit: liu,
                level: 3,
                hp_gain: 20,
                mp_gain: 0
            },
            BattleEvent::Learned {
                unit: liu,
                strategy: "flood".into()
            },
        ]
    );
    assert!(st.items_found.is_empty(), "removed by an event: no drop");
    assert_eq!(
        (st.units[liu].level, st.units[liu].exp),
        (3, 0),
        "no EXP either"
    );
    assert_eq!(st.outcome, None);
}

#[test]
fn an_adjacent_trigger_without_a_is_any_player_unit_next_to_b() {
    let mut def = battle(OPEN_MAP);
    def.events = vec![event(
        Trigger::Adjacent {
            a: None,
            b: "stranger".into(),
        },
        vec![EventAction::Drama {
            scene: "met".into(),
        }],
    )];
    let pack = pack_with(def);
    let mut st = state(&pack);
    let liu = add(&mut st, &pack, Side::Player, "infantry", 1, p(3, 1));
    let guan = add(&mut st, &pack, Side::Player, "infantry", 1, p(0, 0));
    // An enemy next to it from the start is no player unit.
    add(&mut st, &pack, Side::Enemy, "infantry", 1, p(3, 5));
    let stranger = add(&mut st, &pack, Side::Enemy, "infantry", 1, p(3, 4));
    tag(&mut st, stranger, "stranger");
    st.begin(&pack);
    let ev = st
        .apply(
            &pack,
            Action::Move {
                unit: guan,
                to: p(0, 1),
            },
        )
        .unwrap();
    assert!(!ev.contains(&drama("met")));
    let ev = st
        .apply(
            &pack,
            Action::Move {
                unit: liu,
                to: p(3, 3),
            },
        )
        .unwrap();
    assert_eq!(ev[1..], [drama("met")]);
}

#[test]
fn hp_below_trigger_changes_ai_modes() {
    let mut def = battle(OPEN_MAP);
    def.events = vec![event(
        Trigger::HpBelow {
            target: "boss".into(),
            pct: 50,
        },
        vec![
            EventAction::SetAi {
                target: "boss".into(),
                ai: AiMode::Flee,
                ai_target: None,
                ai_pos: None,
            },
            EventAction::SetAi {
                target: "guard".into(),
                ai: AiMode::Guard,
                ai_target: None,
                ai_pos: None,
            },
            EventAction::SetAi {
                target: "hunter".into(),
                ai: AiMode::Target,
                ai_target: Some("liu".into()),
                ai_pos: None,
            },
        ],
    )];
    let pack = pack_with(def);
    let mut st = state(&pack);
    let me = add(&mut st, &pack, Side::Player, "infantry", 1, p(3, 3));
    let boss = add(&mut st, &pack, Side::Enemy, "infantry", 1, p(3, 4));
    let guard = add(&mut st, &pack, Side::Enemy, "infantry", 1, p(6, 6));
    let hunter = add(&mut st, &pack, Side::Enemy, "infantry", 1, p(7, 0));
    tag(&mut st, boss, "boss");
    tag(&mut st, guard, "guard");
    tag(&mut st, hunter, "hunter");
    st.units[boss].hp = 300;
    st.begin(&pack);
    assert_eq!(
        st.units[boss].ai,
        AiMode::Aggressive,
        "60% is not below 50%"
    );
    st.apply(
        &pack,
        Action::Attack {
            unit: me,
            target: boss,
        },
    )
    .unwrap();
    assert_eq!(st.units[boss].hp, 166);
    assert_eq!(st.units[boss].ai, AiMode::Flee);
    assert_eq!(
        (st.units[guard].ai, st.units[guard].ai_pos),
        (AiMode::Guard, Some(p(6, 6)))
    );
    assert_eq!(
        (st.units[hunter].ai, st.units[hunter].ai_target.as_deref()),
        (AiMode::Target, Some("liu"))
    );
}

/// `set_ai` replaces every AI field: an omitted `ai_pos` clears the destination (the validator
/// says so for `advance`, which then behaves as `aggressive`).
#[test]
fn set_ai_replaces_the_destination_and_target() {
    let mut def = battle(OPEN_MAP);
    def.events = vec![event(
        Trigger::TurnStart {
            turn: 1,
            side: Side::Player,
        },
        vec![EventAction::SetAi {
            target: "rider".into(),
            ai: AiMode::Advance,
            ai_target: None,
            ai_pos: None,
        }],
    )];
    let pack = pack_with(def);
    let mut st = state(&pack);
    add(&mut st, &pack, Side::Player, "infantry", 1, p(3, 3));
    let rider = add(&mut st, &pack, Side::Enemy, "infantry", 1, p(7, 7));
    tag(&mut st, rider, "rider");
    st.units[rider].ai = AiMode::Target;
    st.units[rider].ai_target = Some("nobody".into());
    st.units[rider].ai_pos = Some(p(0, 7));
    st.begin(&pack);
    let u = &st.units[rider];
    assert_eq!(
        (u.ai, u.ai_target.as_deref(), u.ai_pos),
        (AiMode::Advance, None, None)
    );
}

#[test]
fn repeatable_events_fire_after_every_check() {
    let mut def = battle(OPEN_MAP);
    let everywhere = Trigger::Reach {
        who: None,
        pos: p(0, 0),
        radius: 20,
        to: None,
    };
    def.events = vec![
        EventDef {
            trigger: everywhere.clone(),
            once: false,
            stage: None,
            when: Vec::new(),
            unless: Vec::new(),
            actions: vec![EventAction::GiveGold { amount: 10 }],
        },
        event(everywhere, vec![EventAction::GiveGold { amount: 1 }]),
    ];
    let pack = pack_with(def);
    let mut st = state(&pack);
    let a = add(&mut st, &pack, Side::Player, "infantry", 1, p(0, 0));
    let b = add(&mut st, &pack, Side::Player, "infantry", 1, p(1, 0));
    add(&mut st, &pack, Side::Enemy, "infantry", 1, p(7, 7));
    st.begin(&pack);
    assert_eq!(st.gold_found, 11);
    st.apply(&pack, Action::Wait { unit: a }).unwrap();
    st.apply(&pack, Action::Wait { unit: b }).unwrap();
    assert_eq!(st.gold_found, 31);
    assert_eq!(st.fired, vec![true, true]);
}

/// Events with a `stage` wait for `set_stage`; an event of the stage that has passed no longer
/// fires even when its trigger holds.
#[test]
fn staged_events_fire_only_at_their_stage() {
    let mut def = battle(OPEN_MAP);
    let staged = |stage, trigger, actions| EventDef {
        trigger,
        once: true,
        stage: Some(stage),
        when: Vec::new(),
        unless: Vec::new(),
        actions,
    };
    let turn = |turn| Trigger::TurnStart {
        turn,
        side: Side::Enemy,
    };
    def.events = vec![
        // Stage 0: a unit on the gate tile moves the battle on.
        staged(
            0,
            Trigger::Reach {
                who: None,
                pos: p(3, 3),
                radius: 0,
                to: None,
            },
            vec![
                EventAction::GiveGold { amount: 1 },
                EventAction::SetStage { stage: 1 },
            ],
        ),
        // Stage 0 on turn 2: too late once the stage has moved on.
        staged(0, turn(2), vec![EventAction::GiveGold { amount: 100 }]),
        staged(1, turn(2), vec![EventAction::GiveGold { amount: 10 }]),
        // No stage: at every stage.
        event(turn(2), vec![EventAction::GiveGold { amount: 1000 }]),
    ];
    let pack = pack_with(def);
    let mut st = state(&pack);
    let me = add(&mut st, &pack, Side::Player, "infantry", 1, p(3, 3));
    add(&mut st, &pack, Side::Enemy, "infantry", 1, p(7, 7));
    st.begin(&pack);
    assert_eq!((st.stage, st.gold_found), (1, 1));
    st.apply(&pack, Action::Wait { unit: me }).unwrap();
    end_phase(&mut st, &pack); // enemy phase 1
    end_phase(&mut st, &pack); // player phase 2
    end_phase(&mut st, &pack); // enemy phase 2
    assert_eq!(st.gold_found, 1011);
    assert_eq!(st.fired, vec![true, false, true, true]);
}

/// An event with `unless` fires only while not all of those conditions hold.
#[test]
fn unless_holds_an_event_back_once_all_its_flags_hold() {
    let run = |flags: &[(&str, i64)]| {
        let mut def = battle(OPEN_MAP);
        let mut e = event(
            Trigger::TurnStart {
                turn: 1,
                side: Side::Player,
            },
            vec![EventAction::GiveGold { amount: 5 }],
        );
        e.unless = ["a", "b"]
            .map(|flag| FlagCond {
                flag: flag.into(),
                cmp: crate::script::Compare::Ne,
                value: 0,
            })
            .to_vec();
        def.events = vec![e];
        let pack = pack_with(def);
        let mut st = state(&pack);
        for (flag, value) in flags {
            st.flags.insert(flag.to_string(), *value);
        }
        add(&mut st, &pack, Side::Player, "infantry", 1, p(3, 3));
        add(&mut st, &pack, Side::Enemy, "infantry", 1, p(7, 7));
        st.begin(&pack);
        st.gold_found
    };
    assert_eq!(run(&[]), 5);
    assert_eq!(run(&[("a", 1)]), 5, "one flag of two");
    assert_eq!(run(&[("a", 1), ("b", 1)]), 0, "both hold");
}

/// `set_objective` replaces the objective text for the rest of the battle (and survives a
/// mid-battle save); the conditions stay.
#[test]
fn set_objective_replaces_the_objective_text() {
    let mut def = battle(OPEN_MAP);
    def.objective = "적을 물리쳐라".into();
    def.events = vec![event(
        Trigger::TurnStart {
            turn: 1,
            side: Side::Enemy,
        },
        vec![EventAction::SetObjective {
            text: "여포를 물리쳐라".into(),
        }],
    )];
    let pack = pack_with(def);
    let mut st = state(&pack);
    let me = add(&mut st, &pack, Side::Player, "infantry", 1, p(3, 3));
    add(&mut st, &pack, Side::Enemy, "infantry", 1, p(7, 7));
    st.begin(&pack);
    assert_eq!(st.objective_text(&pack), "적을 물리쳐라");
    st.apply(&pack, Action::Wait { unit: me }).unwrap();
    let ev = end_phase(&mut st, &pack);
    assert!(
        ev.contains(&BattleEvent::ObjectiveChanged {
            text: "여포를 물리쳐라".into()
        }),
        "{ev:?}"
    );
    assert_eq!(st.objective_text(&pack), "여포를 물리쳐라");
    let saved: BattleState = serde_json::from_str(&serde_json::to_string(&st).unwrap()).unwrap();
    assert_eq!(saved.objective_text(&pack), "여포를 물리쳐라");
}

/// Events with `when` wait for their flags: set by this battle's events, else as the campaign
/// had them when the battle began.
#[test]
fn event_conditions_read_battle_and_campaign_flags() {
    let mut def = battle(OPEN_MAP);
    let cond = |text: &str| -> Vec<FlagCond> {
        toml::from_str::<toml::Table>(&format!("when = [{text}]")).unwrap()["when"]
            .clone()
            .try_into()
            .unwrap()
    };
    let turn = |turn| Trigger::TurnStart {
        turn,
        side: Side::Enemy,
    };
    let gated = |when, turn, amount| EventDef {
        trigger: turn,
        once: true,
        stage: None,
        when,
        unless: Vec::new(),
        actions: vec![EventAction::GiveGold { amount }],
    };
    def.events = vec![
        // Turn 2: waits for `gate`, which turn 1 sets.
        gated(cond("{ flag = \"gate\" }"), turn(2), 1),
        event(
            turn(1),
            vec![EventAction::SetFlag {
                flag: "gate".into(),
                value: 1,
            }],
        ),
        // The campaign's flag at the start: `route == 2` holds, `route >= 3` does not.
        gated(
            cond("{ flag = \"route\", cmp = \"==\", value = 2 }"),
            turn(1),
            10,
        ),
        gated(
            cond("{ flag = \"route\", cmp = \">=\", value = 3 }"),
            turn(1),
            100,
        ),
        // Flags never set are 0.
        gated(cond("{ flag = \"nobody\", cmp = \"==\" }"), turn(1), 1000),
    ];
    let pack = pack_with(def);
    let mut st = state(&pack);
    st.start_flags.insert("route".into(), 2);
    add(&mut st, &pack, Side::Player, "infantry", 1, p(3, 3));
    add(&mut st, &pack, Side::Enemy, "infantry", 1, p(7, 7));
    st.begin(&pack);
    end_phase(&mut st, &pack); // enemy phase 1
    assert_eq!(st.gold_found, 1010);
    end_phase(&mut st, &pack); // player phase 2
    end_phase(&mut st, &pack); // enemy phase 2
    assert_eq!(st.gold_found, 1011);
    assert_eq!(st.fired, vec![true, true, true, false, true]);
    // Written back with operators.
    let text = toml::to_string(&def_of(&pack).events[2]).unwrap();
    assert!(text.contains("cmp = \"==\""), "{text}");
}

fn def_of(pack: &Pack) -> &BattleDef {
    &pack.battles[BATTLE]
}

/// `set_terrain` changes the rules grid (movement follows it at once) and remembers the tile
/// picture; an unknown terrain changes nothing.
#[test]
fn set_terrain_changes_the_tile_and_keeps_its_picture() {
    let mut def = battle(
        "
        ........
        ~~~~~~~~
        ........",
    );
    let bridge_at = |x| EventAction::SetTerrain {
        pos: p(x, 1),
        terrain: "plain".into(),
        image: Some(format!("drawbridge_{x}")),
    };
    def.events = vec![event(
        Trigger::TurnStart {
            turn: 1,
            side: Side::Player,
        },
        vec![
            bridge_at(3),
            EventAction::SetTerrain {
                pos: p(4, 1),
                terrain: "lava".into(),
                image: None,
            },
            bridge_at(3),
        ],
    )];
    let pack = pack_with(def);
    let mut st = state(&pack);
    add(&mut st, &pack, Side::Player, "infantry", 1, p(3, 0));
    add(&mut st, &pack, Side::Enemy, "infantry", 1, p(3, 2));
    let ev = st.begin(&pack);
    assert_eq!(
        ev.iter()
            .filter(|e| matches!(e, BattleEvent::TerrainChanged { .. }))
            .count(),
        2,
        "{ev:?}"
    );
    assert_eq!(st.map.terrain_at(p(3, 1)), Some("plain"));
    assert_eq!(st.map.terrain_at(p(4, 1)), Some("river"));
    assert_eq!(
        st.map_images,
        [MapImage {
            pos: p(3, 1),
            image: "drawbridge_3".into()
        }],
        "one picture per tile"
    );
    assert!(
        st.terrain_at(&pack, p(3, 1))
            .is_some_and(|t| t.id == "plain"),
        "the rules follow the new terrain"
    );
    // A mid-battle save keeps the change.
    let saved: BattleState = serde_json::from_str(&serde_json::to_string(&st).unwrap()).unwrap();
    assert_eq!(saved, st);
}

#[test]
fn spawn_uses_the_nearest_free_passable_tile() {
    let mut def = battle(
        "
        ........
        ........
        ........
        ....~...
        ........
        ........
        ........
        ........",
    );
    def.victory = vec![Condition::DefeatUnit {
        target: "boss".into(),
    }];
    let mut boss = spawn(Side::Enemy, p(4, 4));
    boss.group = Some("wave".into());
    boss.tag = Some("boss".into());
    let mut second = spawn(Side::Enemy, p(4, 3));
    second.group = Some("wave".into());
    def.units = vec![boss, second];
    def.events = vec![event(
        Trigger::TurnStart {
            turn: 1,
            side: Side::Enemy,
        },
        vec![EventAction::Spawn {
            group: "wave".into(),
        }],
    )];
    let pack = pack_with(def);
    let mut st = state(&pack);
    add(&mut st, &pack, Side::Player, "infantry", 1, p(4, 4));
    assert_eq!(
        st.begin(&pack),
        vec![phase(Side::Player, 1)],
        "hidden units are not defeated"
    );
    assert_eq!(
        end_phase(&mut st, &pack),
        vec![
            BattleEvent::Spawned { units: vec![0, 1] },
            phase(Side::Enemy, 1)
        ]
    );
    // (4, 4) is taken: distance 1 in row-major order is (4, 3) = river, then (3, 4).
    assert_eq!(st.units[0].pos, p(3, 4));
    // (4, 3) is a river: the first passable free tile at distance 1 is (4, 2).
    assert_eq!(st.units[1].pos, p(4, 2));
    assert!(st.units[0].is_active() && st.units[1].is_active());
    assert_eq!(
        st.next_ai_unit(),
        Some(0),
        "reinforcements act in the phase they arrive"
    );
}

#[test]
fn victory_and_defeat_event_actions() {
    let mut def = battle(OPEN_MAP);
    def.events = vec![event(
        Trigger::Reach {
            who: None,
            pos: p(7, 0),
            radius: 0,
            to: None,
        },
        vec![EventAction::Victory],
    )];
    let pack = pack_with(def);
    let mut st = state(&pack);
    let me = add(&mut st, &pack, Side::Player, "infantry", 1, p(4, 0));
    add(&mut st, &pack, Side::Enemy, "infantry", 1, p(7, 7));
    st.begin(&pack);
    let ev = st
        .apply(
            &pack,
            Action::Move {
                unit: me,
                to: p(7, 0),
            },
        )
        .unwrap();
    assert_eq!(ev.last(), Some(&BattleEvent::Victory));
    assert_eq!(st.outcome, Some(Outcome::Victory));

    let mut def = battle(OPEN_MAP);
    def.events = vec![event(
        Trigger::TurnStart {
            turn: 2,
            side: Side::Player,
        },
        vec![EventAction::Defeat],
    )];
    let pack = pack_with(def);
    let mut st = state(&pack);
    add(&mut st, &pack, Side::Player, "infantry", 1, p(4, 0));
    add(&mut st, &pack, Side::Enemy, "infantry", 1, p(7, 7));
    st.begin(&pack);
    end_phase(&mut st, &pack);
    assert_eq!(
        end_phase(&mut st, &pack),
        vec![
            phase(Side::Player, 2),
            BattleEvent::Defeat(DefeatReason::Event)
        ]
    );
}

// ----- victory, defeat, bonus --------------------------------------------------------------

#[test]
fn defeat_all_ignores_hidden_reinforcements_and_pays_the_reward() {
    let mut def = battle(OPEN_MAP);
    def.reward_gold = 500;
    def.outro = Some("outro".into());
    let mut rein = spawn(Side::Enemy, p(7, 7));
    rein.group = Some("later".into());
    def.units = vec![spawn(Side::Enemy, p(3, 4)), rein];
    let pack = pack_with(def);
    let mut st = state(&pack);
    let me = add(&mut st, &pack, Side::Player, "infantry", 1, p(3, 3));
    st.units[0].hp = 1;
    st.begin(&pack);
    let ev = st
        .apply(
            &pack,
            Action::Attack {
                unit: me,
                target: 0,
            },
        )
        .unwrap();
    assert_eq!(ev[ev.len() - 2..], [BattleEvent::Victory, drama("outro")]);
    assert_eq!(st.outcome, Some(Outcome::Victory));
    assert_eq!(st.gold_found, 500);
    assert_eq!(st.units[1].state, UnitState::Hidden);
}

fn winning(victory: Condition) -> BattleDef {
    let mut def = battle(OPEN_MAP);
    def.victory = vec![victory];
    def
}

/// `def` with a player unit 0 at (3, 3) tagged `liu`, a 1-HP enemy 1 at (3, 4) tagged `boss`,
/// a healthy enemy far away and a second player unit 3 at (0, 7). Returns the state after `begin`.
fn one_blow(def: BattleDef, commander: bool) -> (Pack, BattleState) {
    let pack = pack_with(def);
    let mut st = state(&pack);
    let me = add(&mut st, &pack, Side::Player, "infantry", 1, p(3, 3));
    let boss = add(&mut st, &pack, Side::Enemy, "infantry", 1, p(3, 4));
    add(&mut st, &pack, Side::Enemy, "infantry", 1, p(7, 7));
    add(&mut st, &pack, Side::Player, "infantry", 1, p(0, 7));
    tag(&mut st, me, "liu");
    tag(&mut st, boss, "boss");
    st.units[boss].hp = 1;
    st.units[boss].commander = commander;
    st.begin(&pack);
    (pack, st)
}

#[test]
fn defeat_unit_commander_and_reach_victories() {
    let kill = Action::Attack { unit: 0, target: 1 };
    let (pack, mut st) = one_blow(
        winning(Condition::DefeatUnit {
            target: "boss".into(),
        }),
        false,
    );
    assert_eq!(
        st.apply(&pack, kill.clone()).unwrap().last(),
        Some(&BattleEvent::Victory)
    );

    let (pack, mut st) = one_blow(winning(Condition::DefeatCommander), true);
    assert_eq!(
        st.apply(&pack, kill.clone()).unwrap().last(),
        Some(&BattleEvent::Victory)
    );
    let (pack, mut st) = one_blow(winning(Condition::DefeatCommander), false);
    st.apply(&pack, kill).unwrap();
    assert_eq!(st.outcome, None, "not a commander");

    let reach = Condition::Reach {
        who: Some("liu".into()),
        pos: p(1, 6),
        radius: 1,
        to: None,
    };
    let (pack, mut st) = one_blow(winning(reach.clone()), false);
    st.apply(
        &pack,
        Action::Move {
            unit: 3,
            to: p(1, 7),
        },
    )
    .unwrap();
    assert_eq!(st.outcome, None, "another unit reaching does not count");
    let (pack, mut st) = one_blow(winning(reach), false);
    st.apply(
        &pack,
        Action::Move {
            unit: 0,
            to: p(1, 5),
        },
    )
    .unwrap();
    assert_eq!(st.outcome, Some(Outcome::Victory));
}

#[test]
fn defeat_condition_and_victory_precedence() {
    // Losing a protected allied unit loses the battle.
    let mut def = battle(OPEN_MAP);
    def.defeat = vec![Condition::UnitRetreated {
        target: "envoy".into(),
    }];
    let pack = pack_with(def);
    let mut st = state(&pack);
    add(&mut st, &pack, Side::Player, "infantry", 1, p(0, 0));
    let envoy = add(&mut st, &pack, Side::Ally, "infantry", 1, p(3, 4));
    let foe = add(&mut st, &pack, Side::Enemy, "infantry", 1, p(3, 5));
    tag(&mut st, envoy, "envoy");
    st.units[envoy].hp = 1;
    st.begin(&pack);
    end_phase(&mut st, &pack);
    assert_eq!(st.phase, Side::Ally);
    end_phase(&mut st, &pack);
    let ev = st
        .apply(
            &pack,
            Action::Attack {
                unit: foe,
                target: envoy,
            },
        )
        .unwrap();
    assert_eq!(
        ev.last(),
        Some(&BattleEvent::Defeat(DefeatReason::Condition))
    );

    // When a victory and a defeat condition become true together, victory wins.
    let mut def = winning(Condition::DefeatUnit {
        target: "boss".into(),
    });
    def.defeat = vec![Condition::UnitRetreated {
        target: "boss".into(),
    }];
    let (pack, mut st) = one_blow(def, false);
    st.apply(&pack, Action::Attack { unit: 0, target: 1 })
        .unwrap();
    assert_eq!(st.outcome, Some(Outcome::Victory));
}

#[test]
fn the_lord_retreating_always_loses() {
    // Defeating the last enemy triggers an event that removes the lord: the lord check wins.
    let mut def = battle(OPEN_MAP);
    def.events = vec![event(
        Trigger::UnitDefeated {
            target: "boss".into(),
        },
        vec![EventAction::Retreat {
            target: "liu_bei".into(),
        }],
    )];
    let pack = pack_with(def);
    let roster = vec![
        officer_state(&pack, "liu_bei"),
        officer_state(&pack, "guan_yu"),
    ];
    let mut st = BattleState::new(&pack, BATTLE, &campaign(roster, &[]), 1).unwrap();
    let (liu, guan) = (0, 1);
    assert!(st.units[liu].lord && st.units[guan].pos == p(1, 0));
    let boss = add(&mut st, &pack, Side::Enemy, "infantry", 1, p(1, 1));
    tag(&mut st, boss, "boss");
    st.units[boss].hp = 1;
    st.begin(&pack);
    let ev = st
        .apply(
            &pack,
            Action::Attack {
                unit: guan,
                target: boss,
            },
        )
        .unwrap();
    assert_eq!(
        ev.last(),
        Some(&BattleEvent::Defeat(DefeatReason::LordRetreated))
    );
    assert!(!ev.contains(&BattleEvent::Victory));

    // An enemy defeating the lord in combat.
    let mut st = BattleState::new(
        &pack,
        BATTLE,
        &campaign(vec![officer_state(&pack, "liu_bei")], &[]),
        1,
    )
    .unwrap();
    let foe = add(&mut st, &pack, Side::Enemy, "cavalry", 5, p(0, 1));
    add(&mut st, &pack, Side::Enemy, "cavalry", 5, p(7, 7));
    st.units[0].hp = 1;
    st.begin(&pack);
    end_phase(&mut st, &pack);
    let ev = st
        .apply(
            &pack,
            Action::Attack {
                unit: foe,
                target: 0,
            },
        )
        .unwrap();
    assert_eq!(
        ev[ev.len() - 2..],
        [
            BattleEvent::Retreated { unit: 0 },
            BattleEvent::Defeat(DefeatReason::LordRetreated)
        ]
    );
}

/// A battle fought without the lord is lost when its troop has retreated, not at the turn limit.
#[test]
fn a_troop_without_the_lord_loses_when_it_has_retreated() {
    let mut def = battle(OPEN_MAP);
    def.deploy.forbidden = vec!["liu_bei".into()];
    let pack = pack_with(def);
    let roster = vec![
        officer_state(&pack, "liu_bei"),
        officer_state(&pack, "guan_yu"),
    ];
    let mut st = BattleState::new(&pack, BATTLE, &campaign(roster, &[]), 1).unwrap();
    assert!(st.units.iter().all(|u| !u.lord));
    let guan = 0;
    let foe = add(&mut st, &pack, Side::Enemy, "cavalry", 5, p(1, 1));
    add(&mut st, &pack, Side::Enemy, "cavalry", 5, p(7, 7));
    st.units[guan].hp = 1;
    st.units[guan].pos = p(1, 0);
    st.begin(&pack);
    end_phase(&mut st, &pack);
    let ev = st
        .apply(
            &pack,
            Action::Attack {
                unit: foe,
                target: guan,
            },
        )
        .unwrap();
    assert_eq!(
        ev.last(),
        Some(&BattleEvent::Defeat(DefeatReason::ArmyRetreated))
    );

    // A player reinforcement still to come: the troop is not lost yet.
    let mut def = battle(OPEN_MAP);
    def.deploy.forbidden = vec!["liu_bei".into()];
    let mut later = spawn(Side::Player, p(6, 6));
    later.group = Some("later".into());
    def.units.push(later);
    let pack = pack_with(def);
    let roster = vec![
        officer_state(&pack, "liu_bei"),
        officer_state(&pack, "guan_yu"),
    ];
    let mut st = BattleState::new(&pack, BATTLE, &campaign(roster, &[]), 1).unwrap();
    let guan = st
        .units
        .iter()
        .position(|u| u.officer.as_deref() == Some("guan_yu"))
        .expect("guan_yu is deployed");
    let foe = add(&mut st, &pack, Side::Enemy, "cavalry", 5, p(1, 1));
    add(&mut st, &pack, Side::Enemy, "cavalry", 5, p(7, 7));
    st.units[guan].hp = 1;
    st.units[guan].pos = p(1, 0);
    st.begin(&pack);
    end_phase(&mut st, &pack);
    let ev = st
        .apply(
            &pack,
            Action::Attack {
                unit: foe,
                target: guan,
            },
        )
        .unwrap();
    assert!(
        !ev.iter().any(|e| matches!(e, BattleEvent::Defeat(_))),
        "{ev:?}"
    );
}

#[test]
fn bonus_objective_is_announced_and_paid_at_victory() {
    let mut def = battle(OPEN_MAP);
    def.bonus = Some(BonusDef {
        condition: Condition::Reach {
            who: None,
            pos: p(0, 3),
            radius: 0,
            to: None,
        },
        exp: 30,
        desc: String::new(),
    });
    let pack = pack_with(def);
    let mut st = state(&pack);
    let a = add(&mut st, &pack, Side::Player, "infantry", 1, p(0, 1));
    let b = add(&mut st, &pack, Side::Player, "infantry", 1, p(5, 5));
    let gone = add(&mut st, &pack, Side::Player, "infantry", 1, p(7, 0));
    let foe = add(&mut st, &pack, Side::Enemy, "infantry", 1, p(5, 6));
    st.units[foe].hp = 1;
    st.begin(&pack);
    assert!(!st.bonus_done);
    let ev = st
        .apply(
            &pack,
            Action::Move {
                unit: a,
                to: p(0, 3),
            },
        )
        .unwrap();
    assert_eq!(ev[1..], [BattleEvent::BonusAchieved { exp: 30 }]);
    assert!(st.bonus_done);
    st.apply(
        &pack,
        Action::Move {
            unit: a,
            to: p(0, 2),
        },
    )
    .unwrap_err();
    let ev = st.apply(&pack, Action::Wait { unit: a }).unwrap();
    assert!(ev.is_empty(), "announced once");

    st.units[gone].state = UnitState::Retreated;
    let ev = st
        .apply(
            &pack,
            Action::Attack {
                unit: b,
                target: foe,
            },
        )
        .unwrap();
    assert_eq!(
        ev[1..],
        [
            BattleEvent::Retreated { unit: foe },
            BattleEvent::ExpGained {
                unit: b,
                amount: 38
            },
            BattleEvent::ExpGained {
                unit: a,
                amount: 30
            },
            BattleEvent::ExpGained {
                unit: b,
                amount: 30
            },
            BattleEvent::Victory,
        ]
    );
    assert_eq!(
        (st.units[a].exp, st.units[b].exp, st.units[gone].exp),
        (30, 68, 0)
    );
}

#[test]
fn original_formulas_confuse_as_morale_falls_and_recover_as_it_rises() {
    let original = |rows: &str| {
        let mut pack = pack(rows);
        pack.rules.strategy_formulas = crate::data::StrategyFormulas::Original;
        pack
    };
    // No confusion at a phase start, however low the morale.
    let pack = original(OPEN_MAP);
    let mut st = state(&pack);
    let broken = add(&mut st, &pack, Side::Player, "infantry", 1, p(0, 0));
    add(&mut st, &pack, Side::Enemy, "infantry", 1, p(7, 7));
    st.units[broken].morale = 0;
    assert_eq!(st.begin(&pack), vec![phase(Side::Player, 1)]);
    assert!(st.units[broken].statuses.is_empty());

    // A blow that leaves less than 30 morale: confused with 60 %; never under the engine's.
    let blow = |pack: &crate::pack::Pack, seed| {
        let mut st = BattleState::new(pack, BATTLE, &campaign(Vec::new(), &[]), seed).unwrap();
        let a = add(&mut st, pack, Side::Player, "infantry", 1, p(0, 0));
        let d = add(&mut st, pack, Side::Enemy, "infantry", 1, p(1, 0));
        st.units[d].max_hp = 10_000;
        st.units[d].hp = 10_000;
        st.units[d].morale = 30;
        let ev = st
            .apply(pack, Action::Attack { unit: a, target: d })
            .unwrap();
        assert!(st.units[d].morale < 30);
        ev.contains(&BattleEvent::Confused { unit: d })
    };
    let hits = (0..400).filter(|&seed| blow(&pack, seed)).count();
    assert!((200..280).contains(&hits), "{hits} of 400");
    let engine = self::pack(OPEN_MAP);
    assert!((0..50).all(|seed| !blow(&engine, seed)));

    // A defender the blow confused does not counter (MAIN.EXE 0x2B872); one it did not, does.
    let mut both = [false, false];
    for seed in 0..40 {
        let mut st = BattleState::new(&pack, BATTLE, &campaign(Vec::new(), &[]), seed).unwrap();
        let a = add(&mut st, &pack, Side::Player, "cavalry", 1, p(0, 0));
        let d = add(&mut st, &pack, Side::Enemy, "bandit", 1, p(1, 0));
        st.units[d].max_hp = 10_000;
        st.units[d].hp = 10_000;
        st.units[d].morale = 30;
        // strength * 100 / 150 >= 100: a sure counter.
        st.units[d].strength = 200;
        let ev = st
            .apply(&pack, Action::Attack { unit: a, target: d })
            .unwrap();
        let confused = ev.contains(&BattleEvent::Confused { unit: d });
        let countered = ev
            .iter()
            .any(|e| matches!(e, BattleEvent::Strike { counter: true, .. }));
        assert_eq!(countered, !confused, "{ev:?}");
        both[usize::from(confused)] = true;
    }
    assert_eq!(both, [true, true]);
    // The forecast counts it: a blow leaving less than 30 morale is countered with 40 % of the
    // chance (the rest of the time the defender is confused); the engine's formulas do not.
    for (pack, chance) in [(&pack, 40), (&engine, 100)] {
        let mut st = state(pack);
        let a = add(&mut st, pack, Side::Player, "cavalry", 1, p(0, 0));
        let d = add(&mut st, pack, Side::Enemy, "bandit", 1, p(1, 0));
        st.units[d].max_hp = 10_000;
        st.units[d].hp = 10_000;
        st.units[d].morale = 30;
        st.units[d].strength = 200;
        let counter = st.forecast_attack(pack, a, d).counter.expect("a counter");
        assert_eq!(counter.chance, chance);
    }

    // A morale gain rolls the recovery: a village's at the phase start.
    let pack = original(".v.\n...\n...");
    let mut st = state(&pack);
    let u = add(&mut st, &pack, Side::Player, "infantry", 1, p(1, 0));
    add(&mut st, &pack, Side::Enemy, "infantry", 1, p(2, 2));
    st.units[u].lead = 300;
    st.units[u].morale = 50;
    st.units[u].statuses = vec![confused(crate::battle::UNTIL_RECOVERED)];
    let ev = st.begin(&pack);
    let at = |e: &BattleEvent| ev.iter().position(|x| x == e);
    let expired = BattleEvent::StatusExpired {
        unit: u,
        status: StatusKind::Confused,
    };
    let regenerated = ev
        .iter()
        .position(|e| matches!(e, BattleEvent::Regenerated { unit, .. } if *unit == u))
        .expect("the village regenerates");
    assert_eq!(at(&expired), Some(regenerated + 1), "{ev:?}");
    assert_eq!(ev.iter().filter(|e| **e == expired).count(), 1);
}
