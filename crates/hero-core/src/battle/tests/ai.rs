//! AI planning (§12): targets, modes, strategies, determinism, full AI-vs-AI battles and speed.

use crate::battle::testkit::*;
use crate::battle::{Action, ActiveStatus, BattleState, UnitId};
use crate::battledef::{AiMode, Condition, EventAction, EventDef, Side, Trigger, UnitSpawn};
use crate::data::StatusKind;
use crate::geom::Pos;
use crate::pack::Pack;

fn enemy_phase(st: &mut BattleState) {
    st.phase = Side::Enemy;
}

/// An infantry unit without MP: generic infantry knows `fire`, whose expected damage beats a
/// level-1 physical attack, so tests about attacking use units that cannot cast.
fn brawler(st: &mut BattleState, pack: &Pack, side: Side, pos: Pos) -> UnitId {
    let id = add(st, pack, side, "infantry", 1, pos);
    st.units[id].mp = 0;
    id
}

fn last(plan: &[Action]) -> &Action {
    plan.last().expect("a non-empty plan")
}

fn move_target(plan: &[Action]) -> Option<Pos> {
    plan.iter().find_map(|a| match a {
        Action::Move { to, .. } => Some(*to),
        _ => None,
    })
}

/// Apply a plan and check that every action is accepted.
fn play(st: &mut BattleState, pack: &Pack, plan: Vec<Action>) {
    for a in plan {
        st.apply(pack, a.clone())
            .unwrap_or_else(|e| panic!("AI action {a:?} rejected: {e}"));
    }
}

#[test]
fn attacks_an_adjacent_target_without_needless_moves() {
    let pack = pack(OPEN_MAP);
    let mut st = state(&pack);
    let target = add(&mut st, &pack, Side::Player, "infantry", 1, p(3, 4));
    let foe = brawler(&mut st, &pack, Side::Enemy, p(3, 3));
    enemy_phase(&mut st);
    assert_eq!(st.next_ai_unit(), Some(foe));
    let plan = st.ai_actions(&pack, foe);
    assert_eq!(plan, vec![Action::Attack { unit: foe, target }]);
    play(&mut st, &pack, plan);
    assert!(st.units[foe].acted);
    assert!(st.ai_actions(&pack, foe).is_empty(), "nothing left to plan");
    assert_eq!(st.next_ai_unit(), None);
}

#[test]
fn prefers_a_kill() {
    let pack = pack(OPEN_MAP);
    let mut st = state(&pack);
    add(&mut st, &pack, Side::Player, "infantry", 1, p(3, 4));
    let weak = add(&mut st, &pack, Side::Player, "infantry", 1, p(4, 3));
    let foe = brawler(&mut st, &pack, Side::Enemy, p(3, 3));
    st.units[weak].hp = 50;
    enemy_phase(&mut st);
    let plan = st.ai_actions(&pack, foe);
    assert_eq!(
        last(&plan),
        &Action::Attack {
            unit: foe,
            target: weak
        }
    );
}

#[test]
fn approaches_when_nothing_is_in_reach() {
    let pack = pack(OPEN_MAP);
    let mut st = state(&pack);
    add(&mut st, &pack, Side::Player, "infantry", 1, p(7, 7));
    let foe = add(&mut st, &pack, Side::Enemy, "infantry", 1, p(0, 0));
    enemy_phase(&mut st);
    let plan = st.ai_actions(&pack, foe);
    let to = move_target(&plan).expect("moves");
    assert_eq!(
        to.manhattan(p(7, 7)),
        14 - 4,
        "a full move towards the enemy"
    );
    assert_eq!(last(&plan), &Action::Wait { unit: foe });
    play(&mut st, &pack, plan);
}

#[test]
fn heals_hurt_friends_only() {
    let pack = pack(OPEN_MAP);
    let mut st = state(&pack);
    add(&mut st, &pack, Side::Player, "infantry", 1, p(7, 7));
    let band = add(&mut st, &pack, Side::Enemy, "band", 10, p(3, 3));
    let friend = add(&mut st, &pack, Side::Enemy, "infantry", 1, p(3, 4));
    enemy_phase(&mut st);
    let plan = st.ai_actions(&pack, band);
    assert!(
        !plan.iter().any(|a| matches!(a, Action::Strategy { .. })),
        "no healing at full HP: {plan:?}"
    );

    st.units[friend].hp = 100;
    let plan = st.ai_actions(&pack, band);
    assert_eq!(
        plan,
        vec![Action::Strategy {
            unit: band,
            strategy: "heal".into(),
            target: p(3, 4)
        }],
        "the single heal beats the costlier heal_all"
    );
    play(&mut st, &pack, plan);
    assert_eq!(st.units[friend].hp, 468);
}

#[test]
fn uses_a_strategy_when_it_beats_attacking() {
    let pack = pack(OPEN_MAP);
    let mut st = state(&pack);
    let target = add(&mut st, &pack, Side::Player, "infantry", 10, p(3, 4));
    let dull = add(&mut st, &pack, Side::Enemy, "infantry", 10, p(2, 3));
    let sage = add(&mut st, &pack, Side::Enemy, "infantry", 10, p(3, 3));
    set_stats(&mut st, &pack, dull, [50, 0, 50]);
    set_stats(&mut st, &pack, sage, [50, 100, 50]);
    enemy_phase(&mut st);
    // Physical damage 244; flood 504 at 93% for 6 MP.
    assert_eq!(
        last(&st.ai_actions(&pack, sage)),
        &Action::Strategy {
            unit: sage,
            strategy: "flood".into(),
            target: p(3, 4)
        }
    );
    // Without INT the strategies are weak (and without MP unusable): attack instead.
    assert!(
        matches!(last(&st.ai_actions(&pack, dull)), Action::Attack { target: t, .. } if *t == target)
    );
}

#[test]
fn hold_never_moves() {
    let pack = pack(OPEN_MAP);
    let mut st = state(&pack);
    let target = add(&mut st, &pack, Side::Player, "infantry", 1, p(3, 6));
    let foe = brawler(&mut st, &pack, Side::Enemy, p(3, 3));
    st.units[foe].ai = AiMode::Hold;
    enemy_phase(&mut st);
    assert_eq!(st.ai_actions(&pack, foe), vec![Action::Wait { unit: foe }]);
    st.units[target].pos = p(3, 4);
    assert_eq!(
        st.ai_actions(&pack, foe),
        vec![Action::Attack { unit: foe, target }]
    );
}

#[test]
fn defensive_waits_until_a_hostile_unit_is_in_reach() {
    let pack = pack(OPEN_MAP);
    let mut st = state(&pack);
    let target = add(&mut st, &pack, Side::Player, "infantry", 1, p(7, 7));
    let foe = brawler(&mut st, &pack, Side::Enemy, p(0, 0));
    st.units[foe].ai = AiMode::Defensive;
    enemy_phase(&mut st);
    assert_eq!(st.ai_actions(&pack, foe), vec![Action::Wait { unit: foe }]);
    st.units[target].pos = p(3, 2);
    let plan = st.ai_actions(&pack, foe);
    assert!(move_target(&plan).is_some());
    assert_eq!(last(&plan), &Action::Attack { unit: foe, target });
    play(&mut st, &pack, plan);
}

#[test]
fn guard_stays_within_three_tiles_of_its_post() {
    let pack = pack(OPEN_MAP);
    let mut st = state(&pack);
    let target = add(&mut st, &pack, Side::Player, "infantry", 1, p(6, 1));
    let foe = brawler(&mut st, &pack, Side::Enemy, p(1, 1));
    st.units[foe].ai = AiMode::Guard;
    st.units[foe].ai_pos = Some(p(1, 1));
    enemy_phase(&mut st);
    assert_eq!(
        st.ai_actions(&pack, foe),
        vec![Action::Wait { unit: foe }],
        "(5, 1) would reach the target but is 4 tiles from the post"
    );
    st.units[target].pos = p(4, 2);
    let plan = st.ai_actions(&pack, foe);
    let to = move_target(&plan).expect("moves to attack");
    assert!(to.manhattan(p(1, 1)) <= 3, "{to:?}");
    assert_eq!(last(&plan), &Action::Attack { unit: foe, target });

    // Away from its post it walks back.
    st.units[target].pos = p(7, 0);
    st.units[foe].pos = p(7, 7);
    let to = move_target(&st.ai_actions(&pack, foe)).expect("returns");
    assert!(to.manhattan(p(1, 1)) < p(7, 7).manhattan(p(1, 1)));
}

/// `march` walks to its unit or tile without attacking anything on the way, then waits; a
/// marching foe is no threat to plan around.
#[test]
fn march_moves_without_attacking() {
    let pack = pack(OPEN_MAP);
    let mut st = state(&pack);
    // On the way: a unit that could attack it would stop next to it and strike.
    let bystander = add(&mut st, &pack, Side::Player, "infantry", 1, p(4, 2));
    let foe = brawler(&mut st, &pack, Side::Enemy, p(1, 1));
    st.units[foe].ai = AiMode::March;
    st.units[foe].ai_pos = Some(p(7, 1));
    enemy_phase(&mut st);
    let plan = st.ai_actions(&pack, foe);
    let to = move_target(&plan).expect("marches");
    assert!(to.manhattan(p(7, 1)) < p(1, 1).manhattan(p(7, 1)), "{to:?}");
    assert_eq!(
        last(&plan),
        &Action::Wait { unit: foe },
        "no attack on {bystander}"
    );
    // Towards a unit when it has one.
    st.units[foe].ai_pos = None;
    st.units[foe].ai_target = Some("mark".into());
    let mark = add(&mut st, &pack, Side::Player, "infantry", 1, p(1, 7));
    st.units[mark].tag = Some("mark".into());
    let to = move_target(&st.ai_actions(&pack, foe)).expect("marches");
    assert!(to.manhattan(p(1, 7)) < p(1, 1).manhattan(p(1, 7)), "{to:?}");
    // Next to the destination it steps onto it (a reach trigger there needs the tile itself).
    st.units[foe].ai_target = None;
    st.units[foe].ai_pos = Some(p(1, 2));
    assert_eq!(move_target(&st.ai_actions(&pack, foe)), Some(p(1, 2)));
    // At the destination it waits.
    st.units[foe].ai_pos = Some(p(1, 1));
    assert_eq!(st.ai_actions(&pack, foe), vec![Action::Wait { unit: foe }]);
}

#[test]
fn flee_maximises_the_distance_to_hostile_units() {
    let pack = pack(OPEN_MAP);
    let mut st = state(&pack);
    let chaser = p(3, 4);
    add(&mut st, &pack, Side::Player, "infantry", 1, chaser);
    let foe = brawler(&mut st, &pack, Side::Enemy, p(3, 3));
    st.units[foe].ai = AiMode::Flee;
    enemy_phase(&mut st);
    let plan = st.ai_actions(&pack, foe);
    let to = move_target(&plan).expect("runs");
    let best = st
        .movement_range(&pack, foe)
        .tiles
        .keys()
        .map(|t| t.manhattan(chaser))
        .max()
        .unwrap();
    assert_eq!(to.manhattan(chaser), best);
    assert_eq!(last(&plan), &Action::Wait { unit: foe });
}

#[test]
fn target_mode_goes_for_its_target() {
    let pack = pack(OPEN_MAP);
    let mut st = state(&pack);
    let decoy = add(&mut st, &pack, Side::Player, "infantry", 1, p(2, 3));
    let liu = add(&mut st, &pack, Side::Player, "infantry", 1, p(7, 7));
    let hunter = brawler(&mut st, &pack, Side::Enemy, p(3, 3));
    st.units[liu].tag = Some("liu".into());
    st.units[decoy].hp = 50;
    enemy_phase(&mut st);
    assert_eq!(
        st.ai_actions(&pack, hunter),
        vec![Action::Attack {
            unit: hunter,
            target: decoy
        }],
        "aggressive: the easy kill"
    );
    st.units[hunter].ai = AiMode::Target;
    st.units[hunter].ai_target = Some("liu".into());
    let plan = st.ai_actions(&pack, hunter);
    let to = move_target(&plan).expect("heads for its target");
    assert!(to.manhattan(p(7, 7)) < p(3, 3).manhattan(p(7, 7)));
    assert_eq!(last(&plan), &Action::Wait { unit: hunter });
    // Once in reach it attacks the target.
    st.units[liu].pos = p(5, 4);
    assert_eq!(
        last(&st.ai_actions(&pack, hunter)),
        &Action::Attack {
            unit: hunter,
            target: liu
        }
    );
}

#[test]
fn advance_heads_for_its_position() {
    let pack = pack(OPEN_MAP);
    let mut st = state(&pack);
    add(&mut st, &pack, Side::Player, "infantry", 1, p(0, 7));
    let foe = add(&mut st, &pack, Side::Enemy, "infantry", 1, p(0, 0));
    st.units[foe].ai = AiMode::Advance;
    st.units[foe].ai_pos = Some(p(7, 0));
    enemy_phase(&mut st);
    assert_eq!(
        st.ai_actions(&pack, foe),
        vec![
            Action::Move {
                unit: foe,
                to: p(4, 0)
            },
            Action::Wait { unit: foe }
        ]
    );
    // Next to its destination it steps onto it (a reach trigger there needs the tile itself),
    // and acts from there.
    st.units[foe].pos = p(6, 0);
    assert_eq!(move_target(&st.ai_actions(&pack, foe)), Some(p(7, 0)));
    let near = add(&mut st, &pack, Side::Player, "infantry", 1, p(7, 1));
    let plan = st.ai_actions(&pack, foe);
    assert_eq!(move_target(&plan), Some(p(7, 0)));
    let at_near = st.units[near].pos;
    assert!(
        matches!(last(&plan), Action::Attack { target, .. } if *target == near)
            || matches!(last(&plan), Action::Strategy { target, .. } if *target == at_near),
        "{plan:?}"
    );
    // (Out of the way again, far from the destination.)
    st.units[near].pos = p(1, 7);
    // At its destination it holds the position.
    st.units[foe].pos = p(7, 0);
    assert_eq!(st.ai_actions(&pack, foe), vec![Action::Wait { unit: foe }]);
}

#[test]
fn advance_holds_its_destination_like_a_post() {
    let pack = pack(OPEN_MAP);
    let mut st = state(&pack);
    let foe = brawler(&mut st, &pack, Side::Enemy, p(7, 0));
    st.units[foe].ai = AiMode::Advance;
    st.units[foe].ai_pos = Some(p(7, 0));
    // A hostile unit within move range (4) but beyond the post's 3 tiles is not chased: it
    // could only be attacked from (4, 1) or (3, 0), 4 tiles from the post.
    let liu = add(&mut st, &pack, Side::Player, "infantry", 1, p(3, 1));
    enemy_phase(&mut st);
    assert_eq!(st.ai_actions(&pack, foe), vec![Action::Wait { unit: foe }]);
    // One near the post is fought from a tile near it.
    st.units[liu].pos = p(5, 2);
    let plan = st.ai_actions(&pack, foe);
    let tile = move_target(&plan).unwrap_or(p(7, 0));
    assert!(tile.manhattan(p(7, 0)) <= 3, "{plan:?}");
    assert!(
        matches!(last(&plan), Action::Attack { target, .. } if *target == liu),
        "{plan:?}"
    );
    // After the sortie, with the enemy gone, it goes back instead of wandering.
    st.units[foe].pos = p(5, 1);
    st.units[liu].pos = p(0, 7);
    assert_eq!(move_target(&st.ai_actions(&pack, foe)), Some(p(7, 0)));
    // With the enemy still next to it, it fights on rather than walking back first.
    st.units[liu].pos = p(4, 1);
    assert!(
        matches!(last(&st.ai_actions(&pack, foe)), Action::Attack { target, .. } if *target == liu),
    );
    // A better action near the post beats a weaker one from the destination: the sure kill
    // next to it over a full-HP unit next to the destination.
    st.units[liu].hp = 1;
    let full = add(&mut st, &pack, Side::Player, "infantry", 1, p(7, 1));
    let plan = st.ai_actions(&pack, foe);
    assert!(
        matches!(last(&plan), Action::Attack { target, .. } if *target == liu),
        "{plan:?} (not {full})"
    );
}

#[test]
fn advance_near_its_destination_threatens_only_the_posts_radius() {
    let pack = pack(OPEN_MAP);
    let mut st = state(&pack);
    let foe = brawler(&mut st, &pack, Side::Enemy, p(1, 1));
    st.units[foe].ai = AiMode::Advance;
    st.units[foe].ai_pos = Some(p(1, 1));
    // A careful unit one hit from defeat approaches as close as it safely can: 5 tiles from
    // the post (the advance unit acts from within 3 tiles of it and strikes 1 tile further),
    // not 6 as it would keep from a unit free to use its whole move of 4.
    let me = brawler(&mut st, &pack, Side::Player, p(6, 4));
    st.units[me].hp = 1;
    let to = move_target(&st.ai_actions(&pack, me)).expect("approaches");
    assert_eq!(to.manhattan(p(1, 1)), 5, "{to:?}");
}

#[test]
fn hurt_careful_units_heal_on_villages() {
    let pack = pack(
        "
....v...
........
........
........
........
........
........
........",
    );
    let mut st = state(&pack);
    // A hold enemy: every tile next to it would defeat the hurt unit, which is stuck 2 tiles
    // away.
    let foe = add(&mut st, &pack, Side::Enemy, "infantry", 1, p(0, 0));
    st.units[foe].ai = AiMode::Hold;
    let me = brawler(&mut st, &pack, Side::Player, p(2, 0));
    let max = st.units[me].max_hp;
    st.units[me].hp = max / 10;
    // Stuck and below half its HP, it goes onto the village in reach.
    assert_eq!(move_target(&st.ai_actions(&pack, me)), Some(p(4, 0)));
    // On the village, with nothing to attack, it stays until healed to three quarters...
    st.units[me].pos = p(4, 0);
    st.units[foe].pos = p(0, 7);
    st.units[me].hp = max * 7 / 10;
    assert_eq!(st.ai_actions(&pack, me), vec![Action::Wait { unit: me }]);
    // ...then goes on.
    st.units[me].hp = max * 8 / 10;
    let to = move_target(&st.ai_actions(&pack, me)).expect("moves on");
    assert!(to.manhattan(p(0, 7)) < p(4, 0).manhattan(p(0, 7)), "{to:?}");
    // Hurt but free to get on towards the enemy, it does rather than turn to the village.
    st.units[me].pos = p(5, 3);
    st.units[me].hp = max * 4 / 10;
    let to = move_target(&st.ai_actions(&pack, me)).expect("moves on");
    assert!(to.manhattan(p(0, 7)) < p(5, 3).manhattan(p(0, 7)), "{to:?}");
    // Enemy units (not careful) do not rest: a hurt one on the village still heads for the
    // player unit.
    st.units[foe].pos = p(4, 0);
    st.units[foe].ai = AiMode::Aggressive;
    st.units[foe].hp = st.units[foe].max_hp / 10;
    st.units[foe].mp = 0;
    enemy_phase(&mut st);
    let to = move_target(&st.ai_actions(&pack, foe)).expect("moves");
    assert!(to.manhattan(p(5, 3)) < p(4, 0).manhattan(p(5, 3)), "{to:?}");
}

#[test]
fn hurt_careful_units_ignore_villages_they_cannot_walk_to() {
    // The village is ringed by river: heading for it would stall the unit at the bank.
    let pack = pack(
        "
........
........
........
........
......~~
......~v
......~~
........",
    );
    let mut st = state(&pack);
    let foe = add(&mut st, &pack, Side::Enemy, "infantry", 1, p(0, 0));
    st.units[foe].ai = AiMode::Hold;
    let me = brawler(&mut st, &pack, Side::Player, p(2, 0));
    st.units[me].hp = st.units[me].max_hp / 10;
    assert_eq!(st.ai_actions(&pack, me), vec![Action::Wait { unit: me }]);
}

#[test]
fn hurt_careful_units_ignore_healing_tiles_they_cannot_enter() {
    // Horses do not enter forest, which heals in this pack: next to it would be a dead end.
    let mut pack = pack(
        "
........
........
........
........
........
........
........
.......T",
    );
    pack.terrain
        .iter_mut()
        .find(|t| t.id == "forest")
        .unwrap()
        .heal_hp = 10;
    let mut st = state(&pack);
    let foe = add(&mut st, &pack, Side::Enemy, "infantry", 1, p(0, 0));
    st.units[foe].ai = AiMode::Hold;
    let rider = add(&mut st, &pack, Side::Player, "cavalry", 1, p(2, 0));
    st.units[rider].mp = 0;
    st.units[rider].hp = st.units[rider].max_hp / 10;
    assert_eq!(
        st.ai_actions(&pack, rider),
        vec![Action::Wait { unit: rider }]
    );
}

#[test]
fn player_side_simulation_uses_healing_items() {
    let pack = pack(OPEN_MAP);
    let mut st = state(&pack);
    st.inventory.insert("bean".into(), 1);
    let me = add(&mut st, &pack, Side::Player, "cavalry", 1, p(3, 3));
    let hurt = add(&mut st, &pack, Side::Player, "infantry", 1, p(3, 4));
    add(&mut st, &pack, Side::Enemy, "infantry", 1, p(0, 7));
    st.units[me].mp = 0;
    st.units[hurt].hp = 100;
    assert_eq!(
        st.next_ai_unit(),
        None,
        "the player phase is not AI-controlled"
    );
    let plan = st.ai_actions(&pack, me);
    assert!(
        matches!(last(&plan), Action::UseItem { item, target, .. } if item == "bean" && *target == hurt),
        "{plan:?}"
    );
    play(&mut st, &pack, plan);
    assert_eq!(st.units[hurt].hp, 400);
    assert!(st.inventory.is_empty());
}

#[test]
fn planning_is_deterministic_and_skips_confused_units() {
    let pack = skirmish_pack();
    let mut st = skirmish(&pack, 3);
    st.begin(&pack);
    st.run_ai_phase(&pack);
    st.run_ai_phase(&pack);
    assert_eq!(st.phase, Side::Enemy);
    for id in 0..st.units.len() {
        assert_eq!(st.ai_actions(&pack, id), st.clone().ai_actions(&pack, id));
    }
    let first = st.next_ai_unit().expect("an enemy to move");
    st.units[first].statuses.push(ActiveStatus {
        status: StatusKind::Confused,
        turns: 1,
    });
    assert!(st.ai_actions(&pack, first).is_empty());
    let next = st.next_ai_unit().expect("another enemy");
    assert_ne!(next, first);

    let (mut a, mut b) = (st.clone(), st.clone());
    assert_eq!(a.run_ai_phase(&pack), b.run_ai_phase(&pack));
    assert_eq!(a, b);
    assert!(!a.units[first].acted, "the confused unit was skipped");
}

// ----- full battles --------------------------------------------------------------------------

fn spawn(side: Side, class: &str, level: u32, pos: Pos, ai: AiMode) -> UnitSpawn {
    UnitSpawn {
        side,
        officer: None,
        name: None,
        class: Some(class.into()),
        level: Some(level),
        stats: None,
        pos,
        ai,
        ai_target: None,
        ai_pos: None,
        commander: false,
        tag: None,
        group: None,
        equip: None,
        drop: None,
    }
}

/// A small campaign-style battle: Liu Bei's four officers against bandits with a commander,
/// an allied unit, and reinforcements on turn 3.
fn skirmish_pack() -> Pack {
    let mut def = battle(
        "
        ..........
        ..TT......
        ..TT...^^.
        ....v..^^.
        ..........
        ...~~.....
        ...~~..TT.
        ..........",
    );
    def.turn_limit = 20;
    def.deploy.slots = vec![p(0, 0), p(1, 0), p(0, 1), p(1, 1)];
    let mut boss = spawn(Side::Enemy, "bandit", 6, p(8, 2), AiMode::Defensive);
    boss.commander = true;
    boss.tag = Some("boss".into());
    boss.drop = Some("jade".into());
    let mut guard = spawn(Side::Enemy, "archer", 5, p(9, 4), AiMode::Guard);
    guard.ai_pos = Some(p(8, 4));
    let mut rein = spawn(Side::Enemy, "cavalry", 5, p(9, 7), AiMode::Aggressive);
    rein.group = Some("rein".into());
    let mut rein2 = spawn(Side::Enemy, "infantry", 5, p(9, 7), AiMode::Aggressive);
    rein2.group = Some("rein".into());
    def.units = vec![
        boss,
        spawn(Side::Enemy, "infantry", 5, p(6, 4), AiMode::Aggressive),
        spawn(Side::Enemy, "cavalry", 4, p(9, 0), AiMode::Aggressive),
        spawn(Side::Enemy, "band", 5, p(9, 3), AiMode::Aggressive),
        guard,
        spawn(Side::Ally, "infantry", 5, p(4, 7), AiMode::Aggressive),
        rein,
        rein2,
    ];
    def.victory = vec![Condition::DefeatCommander, Condition::DefeatAll];
    def.events = vec![
        EventDef {
            trigger: Trigger::TurnStart {
                turn: 3,
                side: Side::Enemy,
            },
            once: true,
            stage: None,
            when: Vec::new(),
            unless: Vec::new(),
            actions: vec![EventAction::Spawn {
                group: "rein".into(),
            }],
        },
        EventDef {
            trigger: Trigger::HpBelow {
                target: "boss".into(),
                pct: 40,
            },
            once: true,
            stage: None,
            when: Vec::new(),
            unless: Vec::new(),
            actions: vec![EventAction::SetAi {
                target: "boss".into(),
                ai: AiMode::Flee,
                ai_target: None,
                ai_pos: None,
            }],
        },
    ];
    pack_with(def)
}

fn skirmish(pack: &Pack, seed: u64) -> BattleState {
    let roster = ["liu_bei", "guan_yu", "zhang_fei", "jian_yong"]
        .iter()
        .map(|id| officer_state(pack, id))
        .collect();
    let mut camp = campaign(roster, &[]);
    camp.inventory.insert("bean".into(), 2);
    camp.inventory.insert("fire_scroll".into(), 1);
    BattleState::new(pack, BATTLE, &camp, seed).expect("skirmish builds")
}

#[test]
fn ai_versus_ai_battles_finish() {
    let pack = skirmish_pack();
    for seed in 0..6 {
        let mut st = skirmish(&pack, seed);
        st.begin(&pack);
        let mut phases = 0;
        while !st.is_over() {
            let before = (st.turn, st.phase);
            st.run_ai_phase(&pack);
            assert!(
                st.is_over() || (st.turn, st.phase) != before,
                "the phase must advance"
            );
            phases += 1;
            assert!(
                phases <= 3 * 20,
                "seed {seed}: the turn limit must end the battle"
            );
        }
        println!("seed {seed}: {:?} on turn {}", st.outcome, st.turn);
        assert!(st.outcome.is_some());
        assert!(st
            .units
            .iter()
            .all(|u| u.hp >= 0 && u.hp <= u.max_hp && (0..=100).contains(&u.morale)));
        // The finished battle still round-trips through a save.
        let json = serde_json::to_string(&st).unwrap();
        assert_eq!(serde_json::from_str::<BattleState>(&json).unwrap(), st);
    }
}

#[test]
fn plans_one_unit_quickly_on_a_crowded_30x30_map() {
    let mut rows = String::new();
    for y in 0..30 {
        for x in 0..30 {
            let open_row = [12, 13, 16, 17].contains(&y);
            rows.push(match (x * 7 + y * 13) % 17 {
                _ if open_row => '.',
                0 | 1 => 'T',
                2 => '^',
                3 => 'v',
                _ => '.',
            });
        }
        rows.push('\n');
    }
    let pack = pack(&rows);
    let mut st = state(&pack);
    let classes = ["infantry", "cavalry", "archer", "bandit", "band"];
    let mut enemies: Vec<UnitId> = Vec::new();
    for i in 0..20 {
        let class = classes[i % classes.len()];
        let (x, row) = ((i % 10) as i32 * 3, (i / 10) as i32);
        add(&mut st, &pack, Side::Player, class, 12, p(x, 12 + row));
        enemies.push(add(
            &mut st,
            &pack,
            Side::Enemy,
            class,
            12,
            p(x + 1, 16 + row),
        ));
    }
    enemy_phase(&mut st);
    let start = std::time::Instant::now();
    let plan = st.ai_actions(&pack, enemies[5]);
    let elapsed = start.elapsed();
    assert!(!plan.is_empty());
    let mut total = std::time::Duration::ZERO;
    for &id in &enemies {
        let t = std::time::Instant::now();
        let plan = st.ai_actions(&pack, id);
        total += t.elapsed();
        play(&mut st.clone(), &pack, plan);
    }
    println!("one plan: {elapsed:?}; all 20 enemies: {total:?}");
    if !cfg!(debug_assertions) {
        assert!(elapsed.as_millis() < 50, "planning took {elapsed:?}");
        assert!(
            total.as_millis() < 20 * 50,
            "planning 20 units took {total:?}"
        );
    }
}

#[test]
fn a_stalled_advance_fights_its_way() {
    let pack = pack(
        "
........
........
........
........
........
........
........
........
........
........
........
........",
    );
    let mut st = state(&pack);
    // A lord heading for (0, 10): the tiles closer to it are unsafe or no closer, and the
    // defensive and hold units in the way never come to it.
    let lord = add(&mut st, &pack, Side::Player, "infantry", 10, p(0, 3));
    st.units[lord].lord = true;
    st.units[lord].ai = AiMode::Advance;
    st.units[lord].ai_pos = Some(p(0, 10));
    st.units[lord].hp = 656;
    let blocker = add(&mut st, &pack, Side::Enemy, "infantry", 4, p(1, 5));
    st.units[blocker].ai = AiMode::Hold;
    let rider = add(&mut st, &pack, Side::Enemy, "cavalry", 2, p(0, 11));
    st.units[rider].ai = AiMode::Defensive;
    // Waiting would change nothing: it attacks the unit in its way from a safe tile.
    let plan = st.ai_actions(&pack, lord);
    assert_eq!(
        last(&plan),
        &Action::Attack {
            unit: lord,
            target: blocker
        },
        "{plan:?}"
    );
    // With the way clear it goes on instead.
    st.units[blocker].pos = p(7, 0);
    let plan = st.ai_actions(&pack, lord);
    let to = move_target(&plan).expect("moves");
    assert!(to.manhattan(p(0, 10)) < 7, "{plan:?}");
}
