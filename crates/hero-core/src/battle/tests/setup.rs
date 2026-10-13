//! `BattleState::new`, `begin` and serialization.

use crate::battle::testkit::*;
use crate::battle::{BattleError, BattleEvent, BattleState, UnitState, Weather};
use crate::battledef::{AiMode, EventAction, EventDef, Side, TreasureDef, Trigger, UnitSpawn};
use crate::data::Equipment;
use crate::geom::Pos;

fn spawn(side: Side, pos: Pos) -> UnitSpawn {
    UnitSpawn {
        side,
        officer: None,
        name: None,
        class: Some("infantry".into()),
        level: Some(3),
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

/// Player officers placed by `st`, in unit order.
fn placed(st: &BattleState) -> Vec<&str> {
    st.units
        .iter()
        .filter(|u| u.side == Side::Player)
        .filter_map(|u| u.officer.as_deref())
        .collect()
}

#[test]
fn deployed_officers_take_slots_with_campaign_progress() {
    let pack = pack(OPEN_MAP);
    let mut guan = officer_state(&pack, "guan_yu");
    guan.level = 12;
    guan.exp = 40;
    guan.equip = Equipment {
        weapon: Some("sword".into()),
        armor: None,
        accessory: Some("horse".into()),
    };
    let liu = officer_state(&pack, "liu_bei");
    let zhang = officer_state(&pack, "zhang_fei");
    // The lord is deployed without being chosen; zhang_fei was not chosen.
    let camp = campaign(vec![guan, zhang, liu], &["guan_yu"]);
    let st = BattleState::new(&pack, BATTLE, &camp, 1).unwrap();

    assert_eq!(placed(&st), ["liu_bei", "guan_yu"]);
    let l = &st.units[0];
    assert_eq!(l.pos, p(0, 0), "the lord takes the first slot");
    assert!(l.lord);
    let g = &st.units[1];
    assert_eq!(g.pos, p(1, 0));
    assert_eq!((g.level, g.exp), (12, 40));
    assert_eq!(g.equip.weapon.as_deref(), Some("sword"));
    // cavalry: hp 600 + 20 * 11; MP (12 + 10) * 70 / 40 = 38
    assert_eq!((g.hp, g.max_hp), (820, 820));
    assert_eq!((g.mp, g.max_mp), (38, 38));
    assert_eq!(g.morale, 100);
    assert!(!g.lord);
    assert_eq!(g.portrait.as_deref(), Some("guan_yu"));
    assert_eq!(st.move_points(&pack, 1), 8, "cavalry 6 + horse 2");
    assert_eq!(
        (st.turn, st.phase, st.weather),
        (1, Side::Player, Weather::Clear)
    );
}

#[test]
fn fallback_deploys_required_then_lord_then_roster_up_to_max() {
    let mut def = battle(OPEN_MAP);
    def.deploy.max = 3;
    def.deploy.required = vec!["jian_yong".into()];
    def.deploy.forbidden = vec!["zhang_fei".into()];
    let pack = pack_with(def);
    let roster = ["guan_yu", "zhang_fei", "zhang_bao", "liu_bei", "jian_yong"]
        .iter()
        .map(|id| officer_state(&pack, id))
        .collect();
    let st = BattleState::new(&pack, BATTLE, &campaign(roster, &[]), 1).unwrap();
    assert_eq!(placed(&st), ["jian_yong", "liu_bei", "guan_yu"]);
}

/// `CampaignState::deployed` survives from battle to battle; a list chosen for another battle
/// (or before the army changed) is fitted to this battle instead of stopping it.
#[test]
fn stale_deployments_are_normalised_to_the_battle() {
    let mut def = battle(OPEN_MAP);
    def.deploy.max = 3;
    def.deploy.required = vec!["jian_yong".into()];
    def.deploy.forbidden = vec!["zhang_fei".into()];
    let pack = pack_with(def);
    let roster = ["guan_yu", "zhang_fei", "zhang_bao", "liu_bei", "jian_yong"]
        .iter()
        .map(|id| officer_state(&pack, id))
        .collect();
    // Missing the required officer and the lord; a forbidden officer, one who is not in the
    // army and a duplicate; more officers than `deploy.max`.
    let stale = ["zhang_fei", "zhang_bao", "cao_cao", "zhang_bao", "guan_yu"];
    let camp = campaign(roster, &stale);
    let st = BattleState::new(&pack, BATTLE, &camp, 1).unwrap();
    // Required, lord, then the chosen officers in roster order, cut at `max`.
    assert_eq!(placed(&st), ["jian_yong", "liu_bei", "guan_yu"]);
    let def = &pack.battles[BATTLE];
    assert_eq!(
        crate::battle::normalize_deployment(&pack, def, &camp, &camp.deployed),
        ["jian_yong", "liu_bei", "guan_yu"]
    );

    // No more officers than slots, even when `deploy.max` allows more.
    let mut def = battle(OPEN_MAP);
    def.deploy.slots.truncate(1);
    let pack = pack_with(def);
    let roster = vec![
        officer_state(&pack, "guan_yu"),
        officer_state(&pack, "liu_bei"),
    ];
    let camp = campaign(roster, &["liu_bei", "guan_yu"]);
    assert_eq!(crate::battle::deploy_max(&pack.battles[BATTLE]), 1);
    let st = BattleState::new(&pack, BATTLE, &camp, 1).unwrap();
    assert_eq!(placed(&st), ["liu_bei"]);
}

/// Another troop's battle: the forbidden lord stays behind, and losing units cannot lose it
/// through the lord.
#[test]
fn a_battle_without_the_lord_places_the_troop_only() {
    let mut def = battle(OPEN_MAP);
    def.deploy.max = 2;
    def.deploy.required = vec!["guan_yu".into()];
    def.deploy.forbidden = vec!["liu_bei".into()];
    let pack = pack_with(def);
    let roster = ["liu_bei", "guan_yu", "zhang_fei"]
        .iter()
        .map(|id| officer_state(&pack, id))
        .collect();
    let camp = campaign(roster, &[]);
    let st = BattleState::new(&pack, BATTLE, &camp, 1).unwrap();
    assert_eq!(placed(&st), ["guan_yu", "zhang_fei"]);
    assert!(st.units.iter().all(|u| !u.lord));
}

#[test]
fn spawns_use_class_or_officer_stats_and_groups_start_hidden() {
    let mut def = battle(OPEN_MAP);
    let generic = spawn(Side::Enemy, p(5, 5));
    let mut custom = spawn(Side::Enemy, p(6, 5));
    custom.stats = Some([80, 10, 20]);
    custom.name = Some("rebel".into());
    custom.commander = true;
    custom.tag = Some("boss".into());
    custom.drop = Some("bean".into());
    let mut named = spawn(Side::Ally, p(4, 4));
    named.officer = Some("zhang_bao".into());
    named.class = None;
    named.level = None;
    named.ai = AiMode::Guard;
    let mut rein = spawn(Side::Enemy, p(7, 7));
    rein.group = Some("wave".into());
    // A reinforcement listed first still comes after the units that start on the map.
    def.units = vec![rein, generic, custom, named];
    let pack = pack_with(def);
    let st = state(&pack);

    let u = &st.units[0];
    assert_eq!(
        (u.side, u.name.as_str(), u.level),
        (Side::Enemy, "infantry", 3)
    );
    assert_eq!(
        [u.strength, u.int, u.lead],
        [50, 30, 50],
        "class generic stats"
    );
    assert_eq!(u.hp, 540);
    let c = &st.units[1];
    assert_eq!([c.strength, c.int, c.lead], [80, 10, 20]);
    assert_eq!(c.name, "rebel");
    assert!(c.commander);
    assert_eq!(c.drop.as_deref(), Some("bean"));
    assert_eq!(st.find_unit("boss"), Some(1));
    let z = &st.units[2];
    assert_eq!((z.class.as_str(), z.level, z.strength), ("bandit", 8, 70));
    assert_eq!(z.ai_pos, Some(p(4, 4)), "guard defaults to its spawn tile");
    assert_eq!(st.find_unit("zhang_bao"), Some(2));
    let r = &st.units[3];
    assert_eq!(r.state, UnitState::Hidden);
    assert_eq!(st.unit_at(p(7, 7)), None);
}

/// A `side = "player"` spawn naming an army officer places the army's officer there: one unit
/// with the army's progress, which is what the campaign gets back after the battle.
#[test]
fn player_spawns_of_army_officers_place_the_army_officer() {
    let mut def = battle(OPEN_MAP);
    let mut hero = spawn(Side::Player, p(5, 5));
    hero.officer = Some("guan_yu".into());
    hero.class = Some("bandit".into()); // ignored: the army's class counts
    hero.level = Some(1);
    hero.tag = Some("hero".into());
    hero.ai = AiMode::Guard;
    let mut guest = spawn(Side::Player, p(6, 6));
    guest.officer = Some("zhang_bao".into());
    guest.class = None;
    guest.level = None;
    def.units = vec![hero, guest];
    let pack = pack_with(def);
    let mut guan = officer_state(&pack, "guan_yu");
    guan.level = 12;
    guan.exp = 40;
    let roster = vec![
        officer_state(&pack, "liu_bei"),
        guan,
        officer_state(&pack, "zhang_fei"),
    ];
    let mut camp = campaign(roster, &["guan_yu", "zhang_fei"]);
    let mut st = BattleState::new(&pack, BATTLE, &camp, 1).unwrap();

    // guan_yu is placed once, at the spawn; the deploy slots go to the others.
    assert_eq!(
        placed(&st),
        ["liu_bei", "zhang_fei", "guan_yu", "zhang_bao"]
    );
    assert_eq!((st.units[0].pos, st.units[1].pos), (p(0, 0), p(1, 0)));
    let g = &st.units[2];
    assert_eq!(
        (g.pos, g.class.as_str(), g.level, g.exp),
        (p(5, 5), "cavalry", 12, 40)
    );
    assert_eq!(g.tag.as_deref(), Some("hero"));
    assert_eq!((g.ai, g.ai_pos), (AiMode::Guard, Some(p(5, 5))));
    assert_eq!(st.find_unit("hero"), st.find_unit("guan_yu"));
    // A player guest who is not in the army is built from `officers.toml`.
    let z = &st.units[3];
    assert_eq!((z.class.as_str(), z.level, z.exp), ("bandit", 8, 0));

    // The army gets the spawned officer's progress back; guests do not join.
    st.units[2].exp = 90;
    camp.apply_battle_result(&pack, &st);
    assert_eq!(
        camp.officer("guan_yu").map(|o| (o.level, o.exp)),
        Some((12, 90))
    );
    assert!(camp.officer("zhang_bao").is_none());

    // An away officer is not the army's to field: the spawn is a guest built from
    // `officers.toml`, and the army keeps their own state after the battle.
    let mut away = camp.clone();
    away.roster
        .iter_mut()
        .find(|o| o.id == "guan_yu")
        .unwrap()
        .away = true;
    let mut st2 = BattleState::new(&pack, BATTLE, &away, 1).unwrap();
    let g = st2
        .units
        .iter()
        .find(|u| u.officer.as_deref() == Some("guan_yu"))
        .unwrap();
    assert_eq!((g.pos, g.class.as_str(), g.level), (p(5, 5), "bandit", 1));
    let gi = st2.find_unit("guan_yu").unwrap();
    st2.units[gi].exp = 5;
    away.apply_battle_result(&pack, &st2);
    assert_eq!(
        away.officer("guan_yu").map(|o| (o.level, o.exp)),
        Some((12, 90))
    );

    // Should a battle hold two player units of one officer, the first one counts.
    let mut copy = st.units[2].clone();
    copy.id = st.units.len();
    copy.level = 1;
    copy.exp = 0;
    st.units.push(copy);
    camp.apply_battle_result(&pack, &st);
    assert_eq!(
        camp.officer("guan_yu").map(|o| (o.level, o.exp)),
        Some((12, 90))
    );
}

#[test]
fn setup_errors() {
    let pack = pack(OPEN_MAP);
    assert_eq!(
        BattleState::new(&pack, "nope", &campaign(Vec::new(), &[]), 1),
        Err(BattleError::UnknownBattle("nope".into()))
    );

    let mut def = battle(OPEN_MAP);
    def.units = vec![spawn(Side::Enemy, p(0, 0))];
    let pack = pack_with(def);
    let camp = campaign(vec![officer_state(&pack, "liu_bei")], &["liu_bei"]);
    let err = BattleState::new(&pack, BATTLE, &camp, 1).unwrap_err();
    assert!(
        matches!(err, BattleError::Setup(ref m) if m.contains("both start")),
        "{err}"
    );

    let pack = pack_with(battle(OPEN_MAP));
    let mut guan = officer_state(&pack, "guan_yu");
    guan.class = "sorcerer".into();
    let camp = campaign(vec![officer_state(&pack, "liu_bei"), guan], &["guan_yu"]);
    let err = BattleState::new(&pack, BATTLE, &camp, 1).unwrap_err();
    assert!(
        matches!(err, BattleError::Setup(ref m) if m.contains("unknown class `sorcerer`")),
        "{err}"
    );
}

#[test]
fn state_copies_inventory_and_sizes_tracking_arrays() {
    let mut def = battle(OPEN_MAP);
    def.treasures = vec![
        TreasureDef {
            pos: p(3, 3),
            item: Some("bean".into()),
            gold: 0,
        };
        2
    ];
    def.events = vec![
        EventDef {
            trigger: Trigger::TurnStart {
                turn: 5,
                side: Side::Enemy,
            },
            once: true,
            stage: None,
            when: Vec::new(),
            unless: Vec::new(),
            actions: vec![EventAction::GiveGold { amount: 1 }],
        };
        3
    ];
    let pack = pack_with(def);
    let mut camp = campaign(Vec::new(), &[]);
    camp.inventory.insert("bean".into(), 2);
    let st = BattleState::new(&pack, BATTLE, &camp, 1).unwrap();
    assert_eq!(st.treasures_taken, vec![false; 2]);
    assert_eq!(st.fired, vec![false; 3]);
    assert_eq!(st.inventory.get("bean"), Some(&2));
}

#[test]
fn begin_plays_intro_then_starts_turn_one() {
    let mut def = battle(OPEN_MAP);
    def.intro = Some("intro".into());
    def.events = vec![EventDef {
        trigger: Trigger::TurnStart {
            turn: 1,
            side: Side::Player,
        },
        once: true,
        stage: None,
        when: Vec::new(),
        unless: Vec::new(),
        actions: vec![EventAction::Drama { scene: "t1".into() }],
    }];
    let pack = pack_with(def);
    let mut st = state(&pack);
    add(&mut st, &pack, Side::Player, "infantry", 1, p(0, 0));
    add(&mut st, &pack, Side::Enemy, "infantry", 1, p(7, 7));
    let ev = st.begin(&pack);
    assert_eq!(
        ev,
        vec![
            BattleEvent::Drama {
                scene: "intro".into(),
                terrain: Default::default(),
            },
            BattleEvent::PhaseStart {
                side: Side::Player,
                turn: 1
            },
            BattleEvent::Drama {
                scene: "t1".into(),
                terrain: Default::default(),
            },
        ]
    );
    assert_eq!((st.turn, st.phase), (1, Side::Player));
}

#[test]
fn battle_state_round_trips_through_json() {
    let pack = pack(OPEN_MAP);
    let mut st = state(&pack);
    let a = add(&mut st, &pack, Side::Player, "infantry", 5, p(3, 3));
    let e = add(&mut st, &pack, Side::Enemy, "bandit", 5, p(3, 4));
    st.units[e].tag = Some("boss".into());
    st.inventory.insert("bean".into(), 1);
    st.begin(&pack);
    st.apply(&pack, crate::battle::Action::Attack { unit: a, target: e })
        .unwrap();
    st.flags.insert("flag".into(), 3);
    let json = serde_json::to_string(&st).unwrap();
    let back: BattleState = serde_json::from_str(&json).unwrap();
    assert_eq!(back, st);
    // The RNG state survives: both continue identically.
    let (mut x, mut y) = (st.clone(), back);
    assert_eq!(x.rng.next_u64(), y.rng.next_u64());
}

#[test]
fn difficulty_moves_only_enemy_levels_within_bounds() {
    use crate::campaign::Difficulty;
    let mut def = battle(OPEN_MAP);
    let at = |side, x, level| UnitSpawn {
        level: Some(level),
        ..spawn(side, p(x, 5))
    };
    def.units = vec![
        at(Side::Enemy, 0, 3),
        at(Side::Enemy, 1, 1),
        at(Side::Enemy, 2, 49),
        // Above the test rules' level_cap of 50: never lowered by the cap.
        at(Side::Enemy, 3, 60),
        at(Side::Ally, 4, 3),
        at(Side::Player, 5, 3),
    ];
    let pack = pack_with(def);
    let levels = |difficulty| {
        let mut camp = campaign(Vec::new(), &[]);
        camp.difficulty = difficulty;
        let st = BattleState::new(&pack, BATTLE, &camp, 1).unwrap();
        st.units.iter().map(|u| u.level).collect::<Vec<_>>()
    };
    assert_eq!(levels(Difficulty::Normal), [3, 1, 49, 60, 3, 3]);
    assert_eq!(levels(Difficulty::Easy), [1, 1, 47, 58, 3, 3]);
    assert_eq!(levels(Difficulty::Hard), [5, 3, 50, 60, 3, 3]);

    // HP and MP follow the moved level by the usual formulas.
    let mut camp = campaign(Vec::new(), &[]);
    camp.difficulty = Difficulty::Hard;
    let hard = BattleState::new(&pack, BATTLE, &camp, 1).unwrap();
    let normal = BattleState::new(&pack, BATTLE, &campaign(Vec::new(), &[]), 1).unwrap();
    let hp_growth = pack.classes["infantry"].hp_growth;
    assert_eq!(hard.units[0].max_hp, normal.units[0].max_hp + 2 * hp_growth);
    assert_eq!(hard.units[0].hp, hard.units[0].max_hp);
}
