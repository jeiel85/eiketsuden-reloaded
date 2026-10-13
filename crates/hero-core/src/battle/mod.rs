//! Runtime battle state and rules.
//!
//! The frontend drives a battle through [`BattleState::apply`] and animates the returned
//! [`BattleEvent`]s in order. AI turns use [`BattleState::next_ai_unit`] +
//! [`BattleState::ai_actions`]; headless simulations use [`BattleState::run_ai_phase`].
//! Cancelling a move is done by the frontend restoring a clone taken before `Action::Move`.
//!
//! The rules implemented here are specified in `docs/RULES.md`; the submodules follow its
//! sections: `setup` (battle construction), `stats` (§2), `path` (§3), `combat` (§4, §7, §11),
//! `strategy` (§5, §10), `flow` (§1, §6, §8, §9) and `ai` (§12).

use crate::battledef::{AiMode, BattleDef, Side};
use crate::campaign::CampaignState;
use crate::data::{Equipment, Id, StatusKind, TerrainDef};
use crate::geom::{Dir, Pos};
use crate::map::BattleMap;
use crate::pack::Pack;
use crate::rng::Rng;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

mod ai;
mod board;
mod combat;
mod flow;
mod path;
mod setup;
mod stats;
mod strategy;
#[cfg(test)]
mod testkit;
#[cfg(test)]
mod tests;

use board::Board;
pub use setup::{deploy_max, normalize_deployment};

pub type UnitId = usize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnitState {
    /// Reinforcement not yet on the map.
    Hidden,
    Active,
    /// HP reached 0 or removed by an event.
    Retreated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveStatus {
    pub status: StatusKind,
    /// Remaining turns; decremented at the start of the owner's phase, removed at 0.
    /// [`UNTIL_RECOVERED`] under the original strategy formulas.
    pub turns: u8,
}

/// `ActiveStatus::turns` of a confusion without a length (the original strategy formulas): it
/// ends on a recovery roll at the start of the unit's phase (RULES.md §6).
pub const UNTIL_RECOVERED: u8 = u8::MAX;

/// Weather, re-rolled at the start of every turn. Rain blocks `fire` strategies and gives
/// `water` strategies +25% damage.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Weather {
    #[default]
    Clear,
    Cloudy,
    Rain,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Unit {
    pub id: UnitId,
    pub side: Side,
    pub officer: Option<Id>,
    pub name: String,
    pub class: Id,
    pub level: u32,
    pub exp: u32,
    #[serde(rename = "str")]
    pub strength: i32,
    pub int: i32,
    pub lead: i32,
    pub hp: i32,
    pub max_hp: i32,
    pub mp: i32,
    pub max_mp: i32,
    /// 0..=100; part of the attack/defense formula.
    pub morale: i32,
    pub pos: Pos,
    pub facing: Dir,
    pub moved: bool,
    pub acted: bool,
    pub equip: Equipment,
    pub statuses: Vec<ActiveStatus>,
    pub ai: AiMode,
    pub ai_target: Option<String>,
    pub ai_pos: Option<Pos>,
    pub commander: bool,
    pub lord: bool,
    pub tag: Option<String>,
    pub group: Option<String>,
    pub state: UnitState,
    pub portrait: Option<String>,
    pub drop: Option<Id>,
}

impl Unit {
    pub fn is_active(&self) -> bool {
        self.state == UnitState::Active
    }

    pub fn has_status(&self, s: StatusKind) -> bool {
        self.statuses.iter().any(|a| a.status == s)
    }

    /// Whether `reference` (a tag or an officer id) names this unit.
    pub fn matches(&self, reference: &str) -> bool {
        self.tag.as_deref() == Some(reference) || self.officer.as_deref() == Some(reference)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DefeatReason {
    LordRetreated,
    /// Every player unit retreated in a battle fought without the lord.
    ArmyRetreated,
    TurnLimit,
    /// A `defeat` condition of the battle definition became true.
    Condition,
    /// An event action forced defeat.
    Event,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Victory,
    Defeat(DefeatReason),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Action {
    /// Move within the movement range. A unit moves at most once per phase.
    Move {
        unit: UnitId,
        to: Pos,
    },
    Attack {
        unit: UnitId,
        target: UnitId,
    },
    /// Aim a strategy at a tile (area effects are centred there).
    Strategy {
        unit: UnitId,
        strategy: Id,
        target: Pos,
    },
    /// Use a battle consumable from the army inventory. For strategy scrolls `target` is the
    /// unit on the aimed tile.
    UseItem {
        unit: UnitId,
        item: Id,
        target: UnitId,
    },
    /// End this unit's action without doing anything.
    Wait {
        unit: UnitId,
    },
    /// End the current side's phase (remaining units forfeit their actions).
    EndPhase,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ActionError {
    #[error("battle is already over")]
    BattleOver,
    #[error("unit {0} does not exist or is not on the map")]
    NoSuchUnit(UnitId),
    #[error("unit {0} does not belong to the side whose phase it is")]
    NotYourTurn(UnitId),
    #[error("unit {0} has already acted")]
    AlreadyActed(UnitId),
    #[error("unit {0} has already moved")]
    AlreadyMoved(UnitId),
    #[error("unit {0} is confused and cannot act")]
    Confused(UnitId),
    #[error("destination is out of range or blocked")]
    Unreachable,
    #[error("target is not in range")]
    OutOfRange,
    #[error("invalid target")]
    InvalidTarget,
    #[error("strategy `{0}` is unknown or not learned")]
    UnknownStrategy(Id),
    #[error("not enough MP")]
    NotEnoughMp,
    #[error("terrain or weather does not allow this strategy")]
    WrongTerrain,
    #[error("item `{0}` is not usable or not in the inventory")]
    BadItem(Id),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BattleError {
    #[error("unknown battle `{0}`")]
    UnknownBattle(Id),
    #[error("battle setup failed: {0}")]
    Setup(String),
}

/// One reachable tile of a movement range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MoveStep {
    /// Movement points spent to get here.
    pub cost: i32,
    /// Previous tile on the cheapest path (`None` for the origin).
    pub prev: Option<Pos>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct MoveRange {
    pub origin: Pos,
    /// Every tile the unit may end its move on (the origin included). Tiles occupied by
    /// friendly units can be passed through but are not listed.
    pub tiles: BTreeMap<Pos, MoveStep>,
    /// Tiles the unit can cross but not stop on (occupied by friendly units). Paths to tiles
    /// in `tiles` may lead through them; [`MoveRange::path_to`] uses both maps.
    #[serde(default)]
    pub through: BTreeMap<Pos, MoveStep>,
}

impl MoveRange {
    pub fn contains(&self, p: Pos) -> bool {
        self.tiles.contains_key(&p)
    }

    /// Path from origin to `p`, both included. `None` when `p` is not a destination.
    pub fn path_to(&self, p: Pos) -> Option<Vec<Pos>> {
        if !self.tiles.contains_key(&p) {
            return None;
        }
        let step = |q: &Pos| self.tiles.get(q).or_else(|| self.through.get(q));
        let limit = self.tiles.len() + self.through.len();
        let mut path = vec![p];
        let mut cur = p;
        while let Some(prev) = step(&cur).and_then(|s| s.prev) {
            if path.len() > limit {
                return None; // malformed range (cycle)
            }
            path.push(prev);
            cur = prev;
        }
        if cur != self.origin {
            return None;
        }
        path.reverse();
        Some(path)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CounterForecast {
    pub damage: i32,
    /// Chance in percent that the counter happens.
    pub chance: i32,
}

/// Numbers shown in the attack preview window. Physical attacks are deterministic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttackForecast {
    /// Damage the attack will deal.
    pub damage: i32,
    /// Defender DEF multiplier from class affinity in percent (75 = attacker has the advantage).
    pub affinity: i32,
    /// Counter-attack preview, when the defender can counter this attack.
    pub counter: Option<CounterForecast>,
}

/// Preview of a strategy against one affected unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StrategyForecast {
    pub unit: UnitId,
    /// Success chance in percent.
    pub chance: i32,
    /// Expected HP damage (positive) or healing (negative) without the random bonus;
    /// 0 for pure morale/status effects.
    pub amount: i32,
}

/// Result of a strategy on one unit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StrategyHit {
    pub unit: UnitId,
    pub success: bool,
    /// HP damage dealt (0 when none).
    pub damage: i32,
    /// HP healed (0 when none).
    pub healed: i32,
    /// Morale change applied (negative for morale-down).
    pub morale: i32,
    pub status: Option<StatusKind>,
}

/// Everything the frontend needs to animate, in the order it happened.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum BattleEvent {
    PhaseStart {
        side: Side,
        turn: u32,
    },
    Moved {
        unit: UnitId,
        path: Vec<Pos>,
    },
    /// One strike: the attack itself, or the defender's counter-attack.
    Strike {
        attacker: UnitId,
        defender: UnitId,
        damage: i32,
        /// Morale the defender lost.
        morale_loss: i32,
        /// This strike is a counter-attack.
        counter: bool,
    },
    StrategyUsed {
        caster: UnitId,
        strategy: Id,
        target: Pos,
        hits: Vec<StrategyHit>,
    },
    ItemUsed {
        user: UnitId,
        target: UnitId,
        item: Id,
        healed: i32,
        morale: i32,
    },
    /// Terrain / treasure / band-aura regeneration at phase start.
    Regenerated {
        unit: UnitId,
        hp: i32,
        mp: i32,
        morale: i32,
    },
    /// A unit became confused (strategy or low morale), or confused again (the original
    /// formulas: a morale fall on a confused unit).
    Confused {
        unit: UnitId,
    },
    StatusExpired {
        unit: UnitId,
        status: StatusKind,
    },
    WeatherChanged {
        weather: Weather,
    },
    ExpGained {
        unit: UnitId,
        amount: u32,
    },
    LevelUp {
        unit: UnitId,
        level: u32,
        hp_gain: i32,
        mp_gain: i32,
    },
    Promoted {
        unit: UnitId,
        from: Id,
        to: Id,
    },
    Learned {
        unit: UnitId,
        strategy: Id,
    },
    Retreated {
        unit: UnitId,
    },
    Spawned {
        units: Vec<UnitId>,
    },
    TreasureFound {
        unit: UnitId,
        item: Option<Id>,
        gold: i64,
    },
    ItemDropped {
        unit: UnitId,
        item: Id,
    },
    /// Play a drama scene now (battle state has already been updated).
    Drama {
        scene: String,
        /// The terrain under each officer as the scene began ([`BattleState::officer_terrain`]),
        /// for the backgrounds of its duels (`@duel` with a `terrain` background); empty for a
        /// scene without a duel.
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        terrain: BTreeMap<String, String>,
    },
    BonusAchieved {
        exp: u32,
    },
    /// An event changed the terrain of a tile (`set_terrain`); [`BattleState::map_images`]
    /// holds the picture drawn over it, if the event gave one.
    TerrainChanged {
        pos: Pos,
    },
    /// An event changed the objective text to `text` ([`BattleState::objective_text`]).
    ObjectiveChanged {
        text: String,
    },
    Victory,
    Defeat(DefeatReason),
}

/// A picture drawn over one tile of the map from a `set_terrain` event on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MapImage {
    pub pos: Pos,
    /// Media key of `gfx/maps/<image>.png`.
    pub image: String,
}

/// Full battle state; `Clone` for move-cancel snapshots and `Serialize` for mid-battle saves.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BattleState {
    pub battle_id: Id,
    pub map: BattleMap,
    pub units: Vec<Unit>,
    pub turn: u32,
    pub turn_limit: u32,
    pub phase: Side,
    pub weather: Weather,
    pub rng: Rng,
    /// Per event definition: already fired (for `once` events).
    pub fired: Vec<bool>,
    /// Per treasure definition: already taken.
    pub treasures_taken: Vec<bool>,
    pub outcome: Option<Outcome>,
    pub bonus_done: bool,
    /// Consumables available to the player in this battle: a copy of the campaign inventory
    /// taken when the battle was built, minus what has been used since.
    pub inventory: BTreeMap<Id, u32>,
    /// Battle consumables used from `inventory` so far: item id -> count. When the battle ends,
    /// won or lost, [`CampaignState::apply_battle_result`] takes exactly these out of the
    /// campaign inventory. (Mid-battle saves written before this field existed load with an
    /// empty map: items used before such a save are not taken out.)
    #[serde(default)]
    pub items_used: BTreeMap<Id, u32>,
    /// Gold and items picked up during the battle (added to the campaign after victory).
    pub gold_found: i64,
    pub items_found: Vec<Id>,
    /// Campaign flags set by event actions during the battle.
    pub flags: BTreeMap<String, i64>,
    /// Stage set by `set_stage` events (0 at the start); events with a `stage` fire only at it.
    #[serde(default)]
    pub stage: u32,
    /// The objective text a `set_objective` event put in place of the battle's `objective`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub objective: Option<String>,
    /// The campaign's flags when the battle began, for event conditions (`when`). Mid-battle
    /// saves from before this field load without them: conditions then see 0.
    #[serde(default)]
    pub start_flags: BTreeMap<String, i64>,
    /// Pictures of tiles whose terrain events changed, in the order they were changed (at most
    /// one per tile). `map` already holds the new terrain.
    #[serde(default)]
    pub map_images: Vec<MapImage>,
    /// The campaign's extended rules are on (DECISIONS D25): the joint attack bonus of
    /// [`BattleState::joint_attack_pct`].
    #[serde(default)]
    pub extended_rules: bool,
}

impl BattleState {
    /// Build the initial state of `battle`: player units from `campaign.deployed` (the whole
    /// roster when empty), normalised to this battle by [`normalize_deployment`] and placed on
    /// deploy slots; spawns without a `group` placed on the map, grouped spawns hidden.
    pub fn new(
        pack: &Pack,
        battle: &str,
        campaign: &CampaignState,
        seed: u64,
    ) -> Result<BattleState, BattleError> {
        setup::build(pack, battle, campaign, seed)
    }

    /// Start turn 1 (player phase): fires `TurnStart{1, player}` events and the intro drama.
    /// The intro `Drama` event comes first, then the phase start (weather, regeneration, ...).
    /// Call once, right after [`BattleState::new`].
    pub fn begin(&mut self, pack: &Pack) -> Vec<BattleEvent> {
        self.begin_battle(pack)
    }

    pub fn def<'a>(&self, pack: &'a Pack) -> &'a BattleDef {
        &pack.battles[&self.battle_id]
    }

    /// The objective the battle shows: the one a `set_objective` event set, else the battle's.
    pub fn objective_text<'a>(&'a self, pack: &'a Pack) -> &'a str {
        self.objective
            .as_deref()
            .unwrap_or(&self.def(pack).objective)
    }

    pub fn unit(&self, id: UnitId) -> &Unit {
        &self.units[id]
    }

    /// Active unit standing on `pos`.
    pub fn unit_at(&self, pos: Pos) -> Option<UnitId> {
        self.units
            .iter()
            .find(|u| u.is_active() && u.pos == pos)
            .map(|u| u.id)
    }

    /// First unit (any state) matching a tag or officer id.
    pub fn find_unit(&self, reference: &str) -> Option<UnitId> {
        self.units
            .iter()
            .find(|u| u.matches(reference))
            .map(|u| u.id)
    }

    /// A flag's value for event conditions: as this battle's events set it, else as the
    /// campaign had it when the battle began.
    pub fn flag(&self, name: &str) -> i64 {
        self.flags
            .get(name)
            .or_else(|| self.start_flags.get(name))
            .copied()
            .unwrap_or(0)
    }

    /// Take in the flags a scene shown during the battle set.
    ///
    /// * Input: the campaign's flags after the scene (the screen merged this battle's flags
    ///   into them before it, so they hold every flag the battle has set).
    /// * Output: every flag whose value differs from [`BattleState::flag`] is set in `flags`,
    ///   as if an event had set it.
    /// * Why: a scene's `@set` writes the campaign's flags, but event conditions read this
    ///   battle's (and the campaign's only as they were when it began), so an event gated on a
    ///   flag a mid-battle scene set never saw it. In `flags`, they also reach the campaign when
    ///   the battle ends, which already has them.
    pub fn take_scene_flags(&mut self, campaign: &BTreeMap<String, i64>) {
        for (name, &value) in campaign {
            if self.flag(name) != value {
                self.flags.insert(name.clone(), value);
            }
        }
    }

    /// Whether every condition of an event holds now.
    pub fn conditions_hold(&self, when: &[crate::battledef::FlagCond]) -> bool {
        when.iter().all(|c| c.cmp.eval(self.flag(&c.flag), c.value))
    }

    /// Whether the flags let `event` fire now: all of its `when` hold and, if it has an
    /// `unless`, not all of that.
    pub fn flags_allow(&self, event: &crate::battledef::EventDef) -> bool {
        self.flags_hold(&event.when, &event.unless)
    }

    /// The actions of `actions` that would run now: those inside a `when` action only while its
    /// flags allow ([`BattleState::flags_hold`]), in order.
    ///
    /// Input: an event's actions. Output: the ones on the path the flags take now.
    ///
    /// Why: what judges an event by its actions (the AI's goals: does it win or lose the
    /// battle?) must not count a guarded part that cannot run, nor let one guarded ending
    /// hide another that can (mutually exclusive `victory` and `defeat` parts).
    pub fn active_actions<'a>(
        &self,
        actions: &'a [crate::battledef::EventAction],
    ) -> Vec<&'a crate::battledef::EventAction> {
        let mut out = Vec::new();
        for a in actions {
            out.push(a);
            if let crate::battledef::EventAction::When {
                when,
                unless,
                actions,
            } = a
            {
                if self.flags_hold(when, unless) {
                    out.extend(self.active_actions(actions));
                }
            }
        }
        out
    }

    /// Every condition of `when` holds and, when `unless` has any, not all of them do (an
    /// event's or an [`crate::battledef::EventAction::When`]'s flags).
    pub fn flags_hold(
        &self,
        when: &[crate::battledef::FlagCond],
        unless: &[crate::battledef::FlagCond],
    ) -> bool {
        self.conditions_hold(when) && (unless.is_empty() || !self.conditions_hold(unless))
    }

    /// The terrain id under each officer on the map, for the duels of the battle's scenes
    /// (`@duel ... terrain`). Retreated officers count at their last cell, after the ones on
    /// the map; hidden reinforcements do not count.
    ///
    /// Input: the pack. Output: officer id → terrain id.
    ///
    /// Why the retreated ones: a duel told after its loser left the field (an event's scene
    /// played once its actions have run) still shows the ground the loser fought on.
    pub fn officer_terrain(&self, pack: &Pack) -> BTreeMap<String, String> {
        let mut terrain = BTreeMap::new();
        for on_map in [true, false] {
            for u in &self.units {
                let wanted = if on_map {
                    u.is_active()
                } else {
                    u.state == UnitState::Retreated
                };
                let (Some(officer), true) = (u.officer.as_ref(), wanted) else {
                    continue;
                };
                if let Some(t) = self.terrain_at(pack, u.pos) {
                    terrain
                        .entry(officer.to_string())
                        .or_insert_with(|| t.id.to_string());
                }
            }
        }
        terrain
    }

    /// The event that plays scene `scene` now: with the officers' terrain at this moment when
    /// the scene has a duel ([`BattleEvent::Drama`]).
    ///
    /// Why now and not when the frontend opens the scene: an event's actions all run before
    /// its scene is shown, so a `set_terrain` after the `drama` in the same event would
    /// otherwise already show under the duel (ROADMAP M6-3).
    pub(crate) fn drama_event(&self, pack: &Pack, scene: &str) -> BattleEvent {
        let duel = pack.scene(scene).is_some_and(|s| {
            s.cmds
                .iter()
                .any(|c| matches!(c, crate::script::Cmd::Duel { .. }))
        });
        BattleEvent::Drama {
            scene: scene.to_string(),
            terrain: if duel {
                self.officer_terrain(pack)
            } else {
                BTreeMap::new()
            },
        }
    }

    pub fn terrain_at<'a>(&self, pack: &'a Pack, pos: Pos) -> Option<&'a TerrainDef> {
        self.map.terrain_at(pos).and_then(|id| pack.terrain(id))
    }

    pub fn is_over(&self) -> bool {
        self.outcome.is_some()
    }

    /// Unit may still act in the current phase.
    pub fn can_act(&self, id: UnitId) -> bool {
        let u = &self.units[id];
        u.is_active() && u.side == self.phase && !u.acted && self.outcome.is_none()
    }

    // ----- derived stats -------------------------------------------------------------------

    /// Attack power: `(level + 10) * (morale/10 + 400/(140 - str) + class.atk)`, times the best
    /// weapon's `atk_pct`. See `docs/RULES.md`.
    pub fn attack_power(&self, pack: &Pack, id: UnitId) -> i32 {
        self.attack_with_morale(pack, id, self.units[id].morale)
    }

    /// Defense power: same shape as attack with `lead`, `class.def` and `def_pct`.
    pub fn defense_power(&self, pack: &Pack, id: UnitId) -> i32 {
        self.defense_with_morale(pack, id, self.units[id].morale)
    }

    /// Movement points including the best horse (0 while confused).
    pub fn move_points(&self, pack: &Pack, id: UnitId) -> i32 {
        if self.units[id].has_status(StatusKind::Confused) {
            0
        } else {
            self.base_move_points(pack, id)
        }
    }

    // ----- queries -------------------------------------------------------------------------

    /// Tiles the unit may move to this phase. Hostile units block; zone of control applies
    /// (entering a tile adjacent to a hostile unit ends movement). Empty when already moved.
    ///
    /// For a unit whose side is not in its phase this is the range it will have when its
    /// phase starts (useful to show enemy ranges). Hidden and retreated units get an empty range.
    pub fn movement_range(&self, pack: &Pack, id: UnitId) -> MoveRange {
        let u = &self.units[id];
        let spent = u.side == self.phase && (u.moved || u.acted);
        if !u.is_active() || spent {
            return MoveRange {
                origin: u.pos,
                ..MoveRange::default()
            };
        }
        let board = Board::new(self, pack);
        self.reach(pack, &board, id, u.pos, self.move_points(pack, id))
    }

    /// [`BattleState::movement_range`] with the unit's full move even while it is confused: a
    /// confused unit may recover when its phase starts, so a view of what it threatens (the
    /// danger range) counts its whole reach. Confusion changes nothing else about the range.
    pub fn threat_range(&self, pack: &Pack, id: UnitId) -> MoveRange {
        let u = &self.units[id];
        let spent = u.side == self.phase && (u.moved || u.acted);
        if !u.is_active() || spent {
            return MoveRange {
                origin: u.pos,
                ..MoveRange::default()
            };
        }
        let board = Board::new(self, pack);
        self.reach(pack, &board, id, u.pos, self.base_move_points(pack, id))
    }

    /// In-bounds tiles covered by the unit's attack range if it stood on `from`.
    pub fn attack_tiles(&self, pack: &Pack, id: UnitId, from: Pos) -> Vec<Pos> {
        let Some(offsets) = self.class_of(pack, id).range.offsets() else {
            return Vec::new();
        };
        let mut tiles: Vec<Pos> = Vec::with_capacity(offsets.len());
        for p in offsets.into_iter().map(|o| from.offset(o.x, o.y)) {
            if p != from && self.map.in_bounds(p) && !tiles.contains(&p) {
                tiles.push(p);
            }
        }
        tiles
    }

    /// Hostile active units attackable from `from`.
    pub fn attack_targets(&self, pack: &Pack, id: UnitId, from: Pos) -> Vec<UnitId> {
        let side = self.units[id].side;
        let mut targets: Vec<UnitId> = self
            .attack_tiles(pack, id, from)
            .into_iter()
            .filter_map(|p| self.unit_at(p))
            .filter(|&t| t != id && self.units[t].side.is_hostile(side))
            .collect();
        targets.sort_unstable();
        targets.dedup();
        targets
    }

    /// Strategies the unit knows and can currently afford (empty when confused).
    pub fn usable_strategies(&self, pack: &Pack, id: UnitId) -> Vec<Id> {
        let u = &self.units[id];
        if !u.is_active() || u.has_status(StatusKind::Confused) {
            return Vec::new();
        }
        pack.known_strategies(&u.class, u.level)
            .into_iter()
            .filter(|s| pack.strategy(s).is_some_and(|d| u.mp >= d.mp))
            .collect()
    }

    /// Tiles where `strategy` may be aimed from `from` that would affect at least one valid
    /// unit and satisfy the terrain/weather requirement. For `Area::AllInRange` strategies
    /// this returns the caster's own tile when at least one target is in reach.
    pub fn strategy_targets(&self, pack: &Pack, id: UnitId, strategy: &str, from: Pos) -> Vec<Pos> {
        self.strategy_aims(pack, id, strategy, from)
    }

    /// Units `item` can be used on: for healing items the user and orthogonally adjacent
    /// friendly units; for strategy scrolls the targets of that strategy from the user's tile.
    pub fn item_targets(&self, pack: &Pack, id: UnitId, item: &str) -> Vec<UnitId> {
        self.item_target_list(pack, id, item)
    }

    pub fn forecast_attack(
        &self,
        pack: &Pack,
        attacker: UnitId,
        defender: UnitId,
    ) -> AttackForecast {
        self.attack_forecast(pack, attacker, defender)
    }

    pub fn forecast_strategy(
        &self,
        pack: &Pack,
        caster: UnitId,
        strategy: &str,
        target: Pos,
    ) -> Vec<StrategyForecast> {
        self.strategy_forecast(pack, caster, strategy, target)
    }

    // ----- mutation ------------------------------------------------------------------------

    /// Validate and perform an action; returns the resulting events (including level ups,
    /// retreats, triggered events, phase changes and victory/defeat).
    ///
    /// A rejected action leaves the state untouched. On victory the battle's `outro` scene,
    /// if any, is emitted as the last event (`Drama`).
    pub fn apply(&mut self, pack: &Pack, action: Action) -> Result<Vec<BattleEvent>, ActionError> {
        if self.outcome.is_some() {
            return Err(ActionError::BattleOver);
        }
        let mut ev = Vec::new();
        match action {
            Action::Move { unit, to } => self.act_move(pack, unit, to, &mut ev)?,
            Action::Attack { unit, target } => self.act_attack(pack, unit, target, &mut ev)?,
            Action::Strategy {
                unit,
                strategy,
                target,
            } => self.act_strategy(pack, unit, &strategy, target, &mut ev)?,
            Action::UseItem { unit, item, target } => {
                self.act_item(pack, unit, &item, target, &mut ev)?
            }
            Action::Wait { unit } => {
                self.check_actor(unit)?;
                self.units[unit].acted = true;
            }
            Action::EndPhase => {
                self.end_phase(pack, &mut ev);
                return Ok(ev);
            }
        }
        self.settle(pack, &mut ev);
        Ok(ev)
    }

    // ----- AI ------------------------------------------------------------------------------

    /// Next AI-controlled unit of the current phase that has not acted (None during the
    /// player phase or when all have acted).
    pub fn next_ai_unit(&self) -> Option<UnitId> {
        if self.phase == Side::Player {
            None
        } else {
            self.next_actor(self.phase)
        }
    }

    /// Plan the actions for one AI unit: optionally a `Move`, then exactly one of `Attack`,
    /// `Strategy`, `UseItem` or `Wait`. Deterministic for a given state.
    ///
    /// Empty when the unit cannot act (not its phase, already acted, confused). A `Move` can
    /// trigger battle events that change the situation; if the planned action is then
    /// rejected, call `ai_actions` again, which plans from the unit's new tile.
    pub fn ai_actions(&self, pack: &Pack, id: UnitId) -> Vec<Action> {
        self.plan_ai(pack, id)
    }

    /// Let the AI play every unit of the current phase (also usable for the player side in
    /// simulations) and then end the phase. Returns all events.
    pub fn run_ai_phase(&mut self, pack: &Pack) -> Vec<BattleEvent> {
        self.run_ai(pack)
    }
}
