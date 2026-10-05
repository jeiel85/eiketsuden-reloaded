//! Battle scenario definitions (`battles/<id>.toml`).

use crate::data::{Equipment, Id};
use crate::geom::Pos;
use crate::script::{cmp_field, Compare};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

fn yes() -> bool {
    true
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    /// Units the player controls.
    #[default]
    Player,
    /// Friendly units controlled by the AI (move in their own phase after the player).
    Ally,
    Enemy,
}

impl Side {
    /// Player and ally are friends; enemy is hostile to both.
    pub fn is_hostile(self, other: Side) -> bool {
        (self == Side::Enemy) != (other == Side::Enemy)
    }
}

/// How an AI-controlled unit behaves.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiMode {
    /// Seek out and attack the most attractive hostile unit anywhere on the map.
    #[default]
    Aggressive,
    /// Stay put until a hostile unit comes within (move + attack) reach, then fight.
    Defensive,
    /// Never move; attack or use strategies only from the current tile.
    Hold,
    /// Stay within 3 tiles of `ai_pos` (defaults to the spawn tile); attack anything that comes close.
    Guard,
    /// Head for the unit named by `ai_target` (tag or officer id) and attack it.
    Target,
    /// Move towards `ai_pos`, attacking targets of opportunity on the way.
    Advance,
    /// Move away from hostile units (fleeing civilians, escaping commanders).
    Flee,
    /// Head for the unit named by `ai_target`, else for `ai_pos`, without attacking or using
    /// strategies (the original's 무공격이동); it waits once there or without a destination.
    March,
}

/// The map of a battle: written in the battle file, or taken from the pack's map files with
/// `use`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MapDef {
    /// Id of a map of the pack's map files ([`MapEntry`], `maps` in `pack.toml`). Such a
    /// battle writes nothing else in `[map]`: `Pack::load` copies the map's `rows`, `legend`,
    /// `theme` and `image` here and keeps the id, so a loaded battle always has its rows.
    #[serde(default, rename = "use", skip_serializing_if = "Option::is_none")]
    pub use_map: Option<Id>,
    /// One text line per map row; characters are terrain glyphs.
    #[serde(default)]
    pub rows: String,
    /// Extra glyph -> terrain id mappings for this map (single-character keys).
    #[serde(default)]
    pub legend: BTreeMap<String, Id>,
    /// Visual theme hint for the renderer (e.g. `field`, `castle`, `snow`, `desert`).
    #[serde(default)]
    pub theme: Option<String>,
    /// Picture layer: media key of `gfx/maps/<image>.png`, a picture of the whole map drawn
    /// instead of the terrain tileset. `rows` stay the rules (movement, defence, healing).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
}

impl MapDef {
    /// Whether the definition writes any part of a map itself (which a `use` map must not).
    pub fn has_own_content(&self) -> bool {
        !self.rows.trim().is_empty()
            || !self.legend.is_empty()
            || self.theme.is_some()
            || self.image.is_some()
    }
}

/// A map of a map file (`[[map]]`), shared by the battles that `use` its id. Map files let a
/// pack ship maps apart from battles, e.g. the converted original maps before the battles
/// that play on them exist, or one map for several battles.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MapEntry {
    pub id: Id,
    /// Display name for tools and authors (battles show their own name).
    #[serde(default)]
    pub name: String,
    /// Same as [`MapDef::rows`].
    pub rows: String,
    #[serde(default)]
    pub legend: BTreeMap<String, Id>,
    #[serde(default)]
    pub theme: Option<String>,
    /// Same as [`MapDef::image`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
}

impl MapEntry {
    /// The battle map definition of a battle that uses this map.
    pub fn to_def(&self) -> MapDef {
        MapDef {
            use_map: Some(self.id.clone()),
            rows: self.rows.clone(),
            legend: self.legend.clone(),
            theme: self.theme.clone(),
            image: self.image.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeployDef {
    /// Maximum number of player officers that may be deployed.
    pub max: u32,
    /// Officers that must be deployed (always includes the lord implicitly).
    #[serde(default)]
    pub required: Vec<Id>,
    /// Officers that may NOT be deployed in this battle.
    #[serde(default)]
    pub forbidden: Vec<Id>,
    /// Deployment tiles, filled in order (required officers first).
    pub slots: Vec<Pos>,
}

/// A unit placed on the map at battle start (or later, when its `group` is spawned).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnitSpawn {
    pub side: Side,
    /// Named officer (portrait, stats and class come from `officers.toml`).
    #[serde(default)]
    pub officer: Option<Id>,
    /// Display name for generic units, e.g. `황건적`. Ignored when `officer` is set.
    #[serde(default)]
    pub name: Option<String>,
    /// Class; required for generic units, overrides the officer's class otherwise.
    #[serde(default)]
    pub class: Option<Id>,
    /// Level; defaults to the officer's level (required for generic units).
    #[serde(default)]
    pub level: Option<u32>,
    /// `[str, int, lead]` for generic units (default: the class's `generic` stats).
    #[serde(default)]
    pub stats: Option<[i32; 3]>,
    pub pos: Pos,
    #[serde(default)]
    pub ai: AiMode,
    /// Tag or officer id for `ai = "target"`.
    #[serde(default)]
    pub ai_target: Option<String>,
    /// Destination for `advance`, centre for `guard`.
    #[serde(default)]
    pub ai_pos: Option<Pos>,
    /// Enemy commander (the usual "defeat the commander" victory target).
    #[serde(default)]
    pub commander: bool,
    /// Name used by conditions and events to refer to this unit.
    #[serde(default)]
    pub tag: Option<String>,
    /// Reinforcement group: units with a group stay off-map until an event spawns the group.
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub equip: Option<Equipment>,
    /// Item given to the player when this unit is defeated.
    #[serde(default)]
    pub drop: Option<Id>,
}

/// Whether `tile` is in the area of a `reach`: within Manhattan distance `radius` of `pos`, or,
/// when `to` is given, in the rectangle with the corners `pos` and `to` (inclusive, in either
/// order; `radius` is then not used).
pub fn in_reach(pos: Pos, radius: i32, to: Option<Pos>, tile: Pos) -> bool {
    match to {
        None => pos.manhattan(tile) <= radius,
        Some(to) => {
            (pos.x.min(to.x)..=pos.x.max(to.x)).contains(&tile.x)
                && (pos.y.min(to.y)..=pos.y.max(to.y)).contains(&tile.y)
        }
    }
}

/// Something that can become true during a battle. Unit references (`target`, `who`)
/// accept a spawn `tag` or an officer id.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Condition {
    /// Every enemy unit on the map has retreated (hidden reinforcements do not count).
    DefeatAll,
    /// The given unit has retreated.
    DefeatUnit { target: String },
    /// Any enemy commander has retreated.
    DefeatCommander,
    /// A unit (or any player unit when `who` is absent) stands within `radius` of `pos`, or,
    /// with `to`, in the rectangle from `pos` to `to` ([`in_reach`]).
    Reach {
        #[serde(default)]
        who: Option<String>,
        pos: Pos,
        #[serde(default)]
        radius: i32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        to: Option<Pos>,
    },
    /// The given turn has been completed (all phases of that turn ended).
    SurviveTurns { turns: u32 },
    /// The given (usually allied) unit has retreated — used as a defeat condition.
    UnitRetreated { target: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Trigger {
    /// Start of `side`'s phase on `turn`.
    TurnStart {
        turn: u32,
        #[serde(default)]
        side: Side,
    },
    /// A unit retreated.
    UnitDefeated { target: String },
    /// A unit (any player unit when `who` is absent) moved within `radius` of `pos`, or, with
    /// `to`, into the rectangle from `pos` to `to` ([`in_reach`]).
    Reach {
        #[serde(default)]
        who: Option<String>,
        pos: Pos,
        #[serde(default)]
        radius: i32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        to: Option<Pos>,
    },
    /// Two units stand orthogonally adjacent (typical duel trigger); any player unit next to `b`
    /// when `a` is absent.
    Adjacent {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        a: Option<String>,
        b: String,
    },
    /// A unit's HP fell below `pct` percent of its max.
    HpBelow { target: String, pct: i32 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventAction {
    /// Play a drama scene (global scene id).
    Drama {
        scene: String,
    },
    /// Bring a reinforcement group onto the map (occupied tiles shift to the nearest free tile).
    Spawn {
        group: String,
    },
    /// Replace the AI of every unit `target` names. All AI fields are replaced: an omitted
    /// `ai_target` or `ai_pos` is cleared (`guard` without `ai_pos` guards the current tile;
    /// `advance` without `ai_pos` behaves as `aggressive`).
    SetAi {
        target: String,
        ai: AiMode,
        #[serde(default)]
        ai_target: Option<String>,
        #[serde(default)]
        ai_pos: Option<Pos>,
    },
    /// Remove a unit from the map without defeating it in combat (duel loser, escape).
    Retreat {
        target: String,
    },
    /// Grant levels (duel reward).
    LevelUp {
        target: String,
        amount: u32,
    },
    GiveItem {
        item: Id,
    },
    GiveGold {
        amount: i64,
    },
    SetFlag {
        flag: String,
        value: i64,
    },
    /// Move the battle to another stage: events with a `stage` fire only while the battle is at
    /// that stage (every battle starts at stage 0).
    SetStage {
        stage: u32,
    },
    /// Change the terrain of one tile for the rest of the battle (a gate opens, a drawbridge
    /// comes down). `image`, a media key of `gfx/maps/<image>.png` one tile in size, is drawn
    /// over the tile from then on. Without it, a map drawn from the tileset shows the new
    /// terrain's tile; a map with a picture layer keeps its picture there.
    SetTerrain {
        pos: Pos,
        terrain: Id,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        image: Option<String>,
    },
    /// Replace the objective text the battle shows (the battle's `objective`) for the rest of
    /// the battle. The victory and defeat conditions are unchanged.
    SetObjective {
        text: String,
    },
    /// Halve the morale or the HP of every unit on the map of one side, as the original's
    /// script command `2D` does (a fire or water attack, a ruse that confuses the enemy).
    /// `enemy` is the enemy's units; `player` (or `ally`) the player's units and their allies.
    /// Morale falls as an attack's loss does (it can confuse under the original formulas, and a
    /// confused unit left at 0 retreats as after a blow); HP is rounded down but stays at least 1.
    Halve {
        side: Side,
        stat: HalveStat,
    },
    /// Run `actions` here, in the event's order, only while the flags allow, as an event's own
    /// `when` and `unless` do: every `when` condition holds and not all of `unless`. For the
    /// part of what an event does that depends on flags when it cannot be an event of its own
    /// (the original's flag-guarded lines of a script played as the battle moves to its next
    /// stage: an event after it would no longer be at its stage).
    When {
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        when: Vec<FlagCond>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        unless: Vec<FlagCond>,
        actions: Vec<EventAction>,
    },
    Victory,
    Defeat,
}

/// What a [`EventAction::Halve`] halves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HalveStat {
    Morale,
    Hp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventDef {
    pub trigger: Trigger,
    /// Fire only the first time the trigger becomes true.
    #[serde(default = "yes")]
    pub once: bool,
    /// Fire only while the battle is at this stage (`set_stage`); without it, at every stage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage: Option<u32>,
    /// Fire only while every condition holds.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub when: Vec<FlagCond>,
    /// Fire only while at least one of these conditions fails (not all of them hold); empty:
    /// no such check.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unless: Vec<FlagCond>,
    pub actions: Vec<EventAction>,
}

impl EventDef {
    /// The event's actions with the nested ones ([`EventAction::all`]).
    pub fn all_actions(&self) -> Vec<&EventAction> {
        EventAction::all(&self.actions)
    }
}

/// A condition on a flag: `flag <cmp> value`, with the flag's value as this battle's events set
/// it, else as the campaign had it when the battle began (0 when never set). `cmp` defaults to
/// `!=` and `value` to 0, so `{ flag = "x" }` means "x is set".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlagCond {
    pub flag: String,
    #[serde(default = "cmp_field::default", with = "cmp_field")]
    pub cmp: Compare,
    #[serde(default)]
    pub value: i64,
}

/// Optional secondary objective; completing it gives every surviving deployed unit bonus EXP.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BonusDef {
    pub condition: Condition,
    pub exp: u32,
    #[serde(default)]
    pub desc: String,
}

/// Treasury / granary / village tile: the first player unit to stop there takes the reward.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TreasureDef {
    pub pos: Pos,
    #[serde(default)]
    pub item: Option<Id>,
    #[serde(default)]
    pub gold: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BattleDef {
    pub id: Id,
    /// Display name, e.g. `탁현 전투`.
    pub name: String,
    /// Place/year caption, e.g. `184년 유주 탁현`.
    #[serde(default)]
    pub location: String,
    /// One-line objective shown to the player, e.g. `적장 정원지를 물리쳐라`.
    pub objective: String,
    /// Music for the player phase / enemy phase.
    #[serde(default)]
    pub bgm: Option<String>,
    #[serde(default)]
    pub bgm_enemy: Option<String>,
    /// The battle is lost when this turn ends without victory.
    pub turn_limit: u32,
    pub map: MapDef,
    pub deploy: DeployDef,
    pub units: Vec<UnitSpawn>,
    /// Any satisfied condition wins the battle.
    pub victory: Vec<Condition>,
    /// Any satisfied condition loses the battle. The lord retreating and running out of
    /// turns always lose and need not be listed.
    #[serde(default)]
    pub defeat: Vec<Condition>,
    #[serde(default)]
    pub bonus: Option<BonusDef>,
    #[serde(default)]
    pub events: Vec<EventDef>,
    #[serde(default)]
    pub treasures: Vec<TreasureDef>,
    /// Gold awarded on victory.
    #[serde(default)]
    pub reward_gold: i64,
    /// Drama scene played before the first turn / after victory.
    #[serde(default)]
    pub intro: Option<String>,
    #[serde(default)]
    pub outro: Option<String>,
}

/// A reference to a unit by spawn `tag` or officer id: `(field name, value)`.
pub type UnitRef<'a> = (&'static str, &'a str);

// The `unit_refs` methods of the three enums match every variant without a catch-all arm, so a
// new variant that names a unit cannot be forgotten by the validator or the simulator, which
// both use them. (A new unit-naming *field* of a struct such as `UnitSpawn` or `EventDef` still
// has to be added to `BattleDef::unit_refs` and the validator by hand.)

impl Condition {
    /// The units this condition names.
    pub fn unit_refs(&self) -> Vec<UnitRef<'_>> {
        match self {
            Condition::DefeatUnit { target } | Condition::UnitRetreated { target } => {
                vec![("target", target)]
            }
            Condition::Reach { who, .. } => who.iter().map(|w| ("who", w.as_str())).collect(),
            Condition::DefeatAll | Condition::DefeatCommander | Condition::SurviveTurns { .. } => {
                Vec::new()
            }
        }
    }
}

impl Trigger {
    /// The units this trigger names.
    pub fn unit_refs(&self) -> Vec<UnitRef<'_>> {
        match self {
            Trigger::UnitDefeated { target } | Trigger::HpBelow { target, .. } => {
                vec![("target", target)]
            }
            Trigger::Reach { who, .. } => who.iter().map(|w| ("who", w.as_str())).collect(),
            Trigger::Adjacent { a, b } => a
                .iter()
                .map(|a| ("a", a.as_str()))
                .chain([("b", b.as_str())])
                .collect(),
            Trigger::TurnStart { .. } => Vec::new(),
        }
    }
}

impl EventAction {
    /// Every action of `actions` with those inside [`EventAction::When`] after their `when`, in
    /// order.
    ///
    /// Input: an event's actions. Output: them and the nested ones, depth first.
    ///
    /// Why one walk for all: what reads an event's actions (scenes to check, groups it brings
    /// in, whether it wins the battle) must see a nested action as well as a top-level one.
    pub fn all(actions: &[EventAction]) -> Vec<&EventAction> {
        let mut out = Vec::new();
        for a in actions {
            out.push(a);
            if let EventAction::When { actions, .. } = a {
                out.extend(EventAction::all(actions));
            }
        }
        out
    }

    /// The units this action names.
    pub fn unit_refs(&self) -> Vec<UnitRef<'_>> {
        match self {
            EventAction::SetAi {
                target, ai_target, ..
            } => std::iter::once(("target", target.as_str()))
                .chain(ai_target.iter().map(|t| ("ai_target", t.as_str())))
                .collect(),
            EventAction::Retreat { target } | EventAction::LevelUp { target, .. } => {
                vec![("target", target)]
            }
            EventAction::Drama { .. }
            | EventAction::Spawn { .. }
            | EventAction::GiveItem { .. }
            | EventAction::GiveGold { .. }
            | EventAction::SetFlag { .. }
            | EventAction::SetStage { .. }
            | EventAction::SetTerrain { .. }
            | EventAction::SetObjective { .. }
            | EventAction::Halve { .. }
            | EventAction::Victory
            | EventAction::Defeat => Vec::new(),
            // Its actions' references are theirs: walk them with [`EventAction::all`], which
            // yields each nested action once.
            EventAction::When { .. } => Vec::new(),
        }
    }
}

impl BattleDef {
    /// Every unit reference of the battle: victory, defeat and bonus conditions, event triggers
    /// and actions, and the units' `ai_target`s (in that order, repeats kept).
    pub fn unit_refs(&self) -> Vec<UnitRef<'_>> {
        let conditions = self
            .victory
            .iter()
            .chain(&self.defeat)
            .chain(self.bonus.as_ref().map(|b| &b.condition));
        let mut refs: Vec<UnitRef<'_>> = conditions.flat_map(Condition::unit_refs).collect();
        for e in &self.events {
            refs.extend(e.trigger.unit_refs());
            refs.extend(e.all_actions().into_iter().flat_map(EventAction::unit_refs));
        }
        refs.extend(
            self.units
                .iter()
                .filter_map(|u| u.ai_target.as_deref().map(|t| ("ai_target", t))),
        );
        refs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_refs_name_every_referenced_unit() {
        let b: BattleDef = toml::from_str(
            r#"
id = "t"
name = "t"
objective = "t"
turn_limit = 10
victory = [{ type = "defeat_unit", target = "boss" }, { type = "defeat_all" }]
defeat = [{ type = "unit_retreated", target = "liu_bei" }]
bonus = { condition = { type = "reach", who = "zhang_fei", pos = [1, 1] }, exp = 1, desc = "" }
[map]
rows = "."
legend = { "." = "plain" }
[deploy]
max = 1
slots = [[0, 0]]
[[units]]
officer = "guan_yu"
side = "player"
pos = [0, 0]
ai_target = "boss"
[[events]]
trigger = { type = "adjacent", a = "liu_bei", b = "lu_bu" }
actions = [
  { type = "set_ai", target = "lu_bu", ai = "target", ai_target = "liu_bei" },
  { type = "level_up", target = "liu_bei", amount = 1 },
  { type = "retreat", target = "lu_bu" },
  { type = "give_gold", amount = 5 },
]
[[events]]
trigger = { type = "hp_below", target = "boss", pct = 50 }
actions = [{ type = "victory" }]
"#,
        )
        .unwrap();
        let names: Vec<&str> = b.unit_refs().into_iter().map(|(_, n)| n).collect();
        assert_eq!(
            names,
            [
                "boss",
                "liu_bei",
                "zhang_fei",
                "liu_bei",
                "lu_bu",
                "lu_bu",
                "liu_bei",
                "liu_bei",
                "lu_bu",
                "boss",
                "boss"
            ]
        );
        // Variants without the optional unit: nothing.
        let none = Condition::Reach {
            who: None,
            pos: Pos::new(0, 0),
            radius: 0,
            to: None,
        };
        assert!(none.unit_refs().is_empty());
        let set_ai = EventAction::SetAi {
            target: "x".into(),
            ai: AiMode::Hold,
            ai_target: None,
            ai_pos: None,
        };
        assert_eq!(set_ai.unit_refs(), [("target", "x")]);
        let defeated = Trigger::UnitDefeated { target: "y".into() };
        assert_eq!(defeated.unit_refs(), [("target", "y")]);
        let fields: Vec<&str> = Trigger::Adjacent {
            a: Some("x".into()),
            b: "y".into(),
        }
        .unit_refs()
        .into_iter()
        .map(|(f, _)| f)
        .collect();
        assert_eq!(fields, ["a", "b"]);
    }

    #[test]
    fn reach_areas() {
        let (p, q) = (Pos::new(5, 5), Pos::new(3, 7));
        assert!(in_reach(p, 1, None, Pos::new(5, 6)));
        assert!(!in_reach(p, 1, None, Pos::new(6, 6)), "manhattan");
        // A rectangle, corners in any order; the radius is not used.
        for tile in [Pos::new(3, 5), Pos::new(5, 7), Pos::new(4, 6)] {
            assert!(in_reach(p, 0, Some(q), tile), "{tile:?}");
        }
        assert!(!in_reach(p, 0, Some(q), Pos::new(6, 6)));
        assert!(!in_reach(p, 0, Some(q), Pos::new(4, 8)));

        let c: Condition = toml::from_str(
            "type = \"reach\"
who = \"liu_bei\"
pos = [29, 10]
to = [29, 14]",
        )
        .unwrap();
        assert_eq!(
            c,
            Condition::Reach {
                who: Some("liu_bei".into()),
                pos: Pos::new(29, 10),
                radius: 0,
                to: Some(Pos::new(29, 14)),
            }
        );
        // Without `to` the written form is unchanged.
        let t = Trigger::Reach {
            who: None,
            pos: Pos::new(1, 2),
            radius: 3,
            to: None,
        };
        assert!(!toml::to_string(&t).unwrap().contains("to"));
    }

    #[test]
    fn parses_minimal_battle() {
        let src = r#"
id = "b01"
name = "탁현 전투"
objective = "적장을 물리쳐라"
turn_limit = 20
reward_gold = 300
victory = [{ type = "defeat_commander" }]

[map]
rows = """
..T
~~.
"""

[deploy]
max = 3
slots = [[0, 0], [1, 0]]

[[units]]
side = "enemy"
name = "황건적"
class = "bandit"
level = 2
pos = [2, 1]
ai = "aggressive"
commander = true

[[events]]
trigger = { type = "turn_start", turn = 3, side = "enemy" }
actions = [{ type = "spawn", group = "rein" }, { type = "drama", scene = "b01_rein" }]
"#;
        let b: BattleDef = toml::from_str(src).unwrap();
        assert_eq!(b.units[0].side, Side::Enemy);
        assert!(b.units[0].commander);
        assert_eq!(b.victory, vec![Condition::DefeatCommander]);
        assert_eq!(b.events[0].actions.len(), 2);
        assert!(b.events[0].once);
        assert!(Side::Player.is_hostile(Side::Enemy));
        assert!(!Side::Player.is_hostile(Side::Ally));
    }
}
