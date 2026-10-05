//! AI planning (RULES.md §12).
//!
//! A unit scores every (reachable tile × action) candidate: the value of the action (damage,
//! kills, healing, morale and confusion effects) plus a position term (terrain defence,
//! regeneration, minus half the damage hostile units could deal on that tile next phase).
//! When no action is possible it walks towards its goal along the cheapest path. All
//! iteration is in unit-id / position order and ties are broken by the affected unit's id,
//! then staying on the current tile, then the tile's position order, so a plan is
//! deterministic for a given state.
//!
//! The threat on a tile is the sum over hostile units of the damage each could deal there
//! next phase (forecast numbers: physical damage, or expected damage of its damage
//! strategies), from the tiles it can move to — only its own tile for an AI `hold` unit, its
//! post's surroundings for a `guard` at its post.
//!
//! Refinements *(design)* beyond RULES.md §12:
//!
//! * **Lords** are careful, because losing the lord loses the battle: a lord never ends its
//!   move where the hostile units could defeat it within their next two phases (the least
//!   dangerous tiles are used when no tile is safe), attacks only when that still holds after
//!   the counter, weighs the full threat instead of half of it, likes tiles next to friends,
//!   and when it has nothing to do it stays with its army instead of leading the charge.
//!   Other units of its side value hitting the hostile units that threaten their lord.
//! * **Player units** — the AI only commands them in simulations — are played like a careful
//!   human plays them: being defeated on a tile costs twice the unit's max HP, idle moves
//!   keep off such tiles, a unit acts only when that beats just moving on, and friends that
//!   may still move this phase are not counted on to block hostile units.
//! * **Healing**: a careful unit below half its HP that cannot get closer to its goal goes
//!   onto a healing tile it can safely stand on (or towards the nearest one it can walk to),
//!   and a careful unit on a healing tile stays there until it is back at three quarters.
//! * **Scripted endings**: the player's side knows the battle's `adjacent` / `reach` events
//!   that end it and its `reach` conditions. A unit that can win the battle by moving does so,
//!   never moves where it would lose it, and heads for its objective when idle.
//!
//! Scores are expressed in "HP-equivalents": one point is one HP of damage dealt or healed.

use super::board::Board;
use super::combat::{hit_damage, morale_loss, with_joint_attack, JOINT_ATTACK_MAX};
use super::strategy;
use super::{Action, BattleEvent, BattleState, Unit, UnitId};
use crate::battledef::{in_reach, AiMode, Condition, EventAction, Side, Trigger};
use crate::data::{Area, Effect, ItemDef, StatusKind, StrategyDef, TargetSide, TerrainDef};
use crate::geom::Pos;
use crate::pack::Pack;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

/// `guard` units stay within this manhattan distance of their `ai_pos`.
const GUARD_RADIUS: i32 = 3;
/// A careful unit below this percentage of its max HP heads for a healing tile when it has
/// nothing better to do *(design)*...
const HEAL_BELOW_PCT: i64 = 50;
/// ...and stays on it until healed to this percentage.
const HEAL_UNTIL_PCT: i64 = 75;
/// A lord values each orthogonally adjacent friendly unit (friends take the attack slots
/// around it) at this percentage of its max HP.
const LORD_ESCORT_PCT: i64 = 5;
/// A lord keeps to tiles where the hostile units could not defeat it within this many of
/// their phases.
const LORD_SAFETY_PHASES: i64 = 2;

impl BattleState {
    /// First unit of `side` (in unit order) that can still act and is not confused.
    pub(super) fn next_actor(&self, side: Side) -> Option<UnitId> {
        if self.outcome.is_some() || self.phase != side {
            return None;
        }
        self.units
            .iter()
            .find(|u| {
                u.is_active() && u.side == side && !u.acted && !u.has_status(StatusKind::Confused)
            })
            .map(|u| u.id)
    }

    pub(super) fn plan_ai(&self, pack: &Pack, id: UnitId) -> Vec<Action> {
        if id >= self.units.len()
            || !self.can_act(id)
            || self.units[id].has_status(StatusKind::Confused)
        {
            return Vec::new();
        }
        Planner::new(self, pack, id).plan()
    }

    pub(super) fn run_ai(&mut self, pack: &Pack) -> Vec<BattleEvent> {
        let mut ev = Vec::new();
        let (side, turn) = (self.phase, self.turn);
        let same_phase = |s: &BattleState| s.outcome.is_none() && s.phase == side && s.turn == turn;
        while same_phase(self) {
            let Some(id) = self.next_actor(side) else {
                break;
            };
            self.run_ai_unit(pack, id, &mut ev);
        }
        if same_phase(self) {
            self.end_phase(pack, &mut ev);
        }
        ev
    }

    /// Play one unit: apply the planned move, re-plan from the new tile (events fired by the
    /// move may have changed the situation), apply the action.
    fn run_ai_unit(&mut self, pack: &Pack, id: UnitId, ev: &mut Vec<BattleEvent>) {
        for _ in 0..2 {
            let Some(first) = self.plan_ai(pack, id).into_iter().next() else {
                break;
            };
            let is_move = matches!(first, Action::Move { .. });
            match self.apply(pack, first) {
                Ok(events) => ev.extend(events),
                Err(err) => {
                    debug_assert!(false, "AI planned an invalid action for unit {id}: {err}");
                    break;
                }
            }
            if !is_move || !self.can_act(id) {
                break;
            }
        }
        if self.can_act(id) {
            // Planning failed or produced nothing: the unit waits so the phase always advances.
            match self.apply(pack, Action::Wait { unit: id }) {
                Ok(events) => ev.extend(events),
                Err(err) => {
                    debug_assert!(false, "AI unit {id} cannot wait: {err}");
                    self.units[id].acted = true;
                }
            }
        }
    }
}

/// Counter-attack data of a potential target (the attacker's tile decides whether it applies).
struct CounterInfo {
    /// The target's attack offsets (the attacker must stand on one of them).
    offsets: Vec<Pos>,
    chance: i32,
    /// The target's ATK after losing morale to our hit.
    atk: i32,
    /// Our DEF multiplier against the target.
    affinity: i32,
}

/// Facts about physically attacking one unit that do not depend on the attacker's tile.
struct AttackInfo {
    value: i64,
    counter: Option<CounterInfo>,
}

/// Value of aiming a strategy at a tile.
#[derive(Clone, Copy)]
struct AimValue {
    value: i64,
    /// Lowest affected unit id (tie-break key).
    key: UnitId,
    /// A hostile unit is affected.
    hostile: bool,
    /// The unit this AI is focused on (`ai = "target"`) is affected.
    hits_focus: bool,
}

#[derive(Clone)]
struct Choice {
    score: i64,
    /// Unit the action is aimed at (tie-break: lower id first).
    key: UnitId,
    tile: Pos,
    action: Action,
}

/// A set of tile indices that is cleared in O(1), reused for every hostile unit.
struct TileSet {
    stamp: Vec<u32>,
    generation: u32,
    /// Members in insertion order.
    tiles: Vec<usize>,
}

impl TileSet {
    fn new(len: usize) -> TileSet {
        TileSet {
            stamp: vec![0; len],
            generation: 1,
            tiles: Vec::new(),
        }
    }

    fn clear(&mut self) {
        self.generation += 1;
        self.tiles.clear();
    }

    /// Adds `i`; returns whether it was not in the set yet.
    fn insert(&mut self, i: usize) -> bool {
        if self.stamp[i] == self.generation {
            return false;
        }
        self.stamp[i] = self.generation;
        self.tiles.push(i);
        true
    }

    fn contains(&self, i: usize) -> bool {
        self.stamp[i] == self.generation
    }
}

/// Value of dealing `damage` to `target` (§12): the damage, ×3 when it defeats the target,
/// +50% against the lord or a commander.
fn damage_value(damage: i32, target: &Unit) -> i64 {
    let mut value = damage as i64;
    if damage >= target.hp {
        value *= 3;
    }
    if target.lord || target.commander {
        value = value * 3 / 2;
    }
    value
}

/// Where a unit has to stand for a scripted victory or defeat.
enum Place {
    /// Orthogonally next to one of these tiles (the units an `adjacent` trigger pairs it with).
    NextTo(Vec<Pos>),
    /// In the area of a `reach` trigger or condition ([`in_reach`]).
    Near {
        pos: Pos,
        radius: i32,
        to: Option<Pos>,
    },
}

impl Place {
    fn holds(&self, tile: Pos) -> bool {
        match self {
            Place::NextTo(partners) => partners.iter().any(|p| p.manhattan(tile) == 1),
            Place::Near { pos, radius, to } => in_reach(*pos, *radius, *to, tile),
        }
    }
}

/// A battle ending this unit brings about by standing somewhere: an `adjacent` or `reach`
/// event trigger naming it whose actions include `victory` (`wins`) or `defeat`, or a `reach`
/// victory/defeat condition.
struct Scripted {
    wins: bool,
    place: Place,
}

/// Scripted endings unit `me` can bring about. Only the player's side plays the script
/// *(design)*: the AI stands in for a player who knows the objective, while enemy units
/// behave like the original's, which ignore it. `hold` units never move for it.
fn scripted_endings(st: &BattleState, pack: &Pack, me: &Unit) -> Vec<Scripted> {
    let mut out = Vec::new();
    if Side::Player.is_hostile(me.side) || me.ai == AiMode::Hold {
        return out;
    }
    let def = st.def(pack);
    let named = |who: Option<&str>| who.map_or(me.side == Side::Player, |w| me.matches(w));
    // The positions of the other active units `who` names (any player unit when `None`).
    let partners = |who: Option<&str>| -> Vec<Pos> {
        st.units
            .iter()
            .filter(|u| {
                u.is_active()
                    && u.id != me.id
                    && who.map_or(u.side == Side::Player, |w| u.matches(w))
            })
            .map(|u| u.pos)
            .collect()
    };
    for (i, e) in def.events.iter().enumerate() {
        if (e.once && st.fired.get(i).copied().unwrap_or(false))
            || e.stage.is_some_and(|s| s != st.stage)
            || !st.flags_allow(e)
        {
            continue;
        }
        let actions = e.all_actions();
        let wins = if actions.iter().any(|a| matches!(a, EventAction::Defeat)) {
            false
        } else if actions.iter().any(|a| matches!(a, EventAction::Victory)) {
            true
        } else {
            continue;
        };
        let place = match &e.trigger {
            Trigger::Adjacent { a, b } => {
                let mut next_to = Vec::new();
                if named(a.as_deref()) {
                    next_to.extend(partners(Some(b)));
                }
                if me.matches(b) {
                    next_to.extend(partners(a.as_deref()));
                }
                if next_to.is_empty() {
                    continue;
                }
                Place::NextTo(next_to)
            }
            Trigger::Reach {
                who,
                pos,
                radius,
                to,
            } if named(who.as_deref()) => Place::Near {
                pos: *pos,
                radius: *radius,
                to: *to,
            },
            _ => continue,
        };
        out.push(Scripted { wins, place });
    }
    for (conditions, wins) in [(&def.victory, true), (&def.defeat, false)] {
        for c in conditions {
            if let Condition::Reach {
                who,
                pos,
                radius,
                to,
            } = c
            {
                if named(who.as_deref()) {
                    out.push(Scripted {
                        wins,
                        place: Place::Near {
                            pos: *pos,
                            radius: *radius,
                            to: *to,
                        },
                    });
                }
            }
        }
    }
    out
}

/// Keep `c` when it beats `best`: higher score, then lower affected unit id, then acting from
/// `origin` (no needless move), then the lower tile in position order.
fn offer(best: &mut Option<Choice>, c: Choice, origin: Pos) {
    let rank = |x: &Choice| (x.score, Reverse(x.key), x.tile == origin, Reverse(x.tile));
    let better = match best {
        None => true,
        Some(b) => rank(&c) > rank(b),
    };
    if better {
        *best = Some(c);
    }
}

struct Planner<'a> {
    st: &'a BattleState,
    pack: &'a Pack,
    id: UnitId,
    me: &'a Unit,
    board: Board<'a>,
    /// Tiles the unit may end its move on, in position order.
    reach: Vec<Pos>,
    /// Expected damage hostile units could deal to this unit on each tile during their next
    /// phase (physical attacks and damage strategies, terrain included, not capped at HP).
    threat: Vec<i64>,
    /// Per unit id: the damage that hostile unit could deal next phase to this unit's lord
    /// (0 when it cannot reach the lord, or this unit is the lord or has none).
    lord_threat: Vec<i64>,
    atk: i32,
    def: i32,
    attack_offsets: Vec<Pos>,
    strategies: Vec<&'a StrategyDef>,
    /// Deduplicated reach offsets per strategy.
    strategy_offsets: Vec<Vec<Pos>>,
    /// `ai = "target"` or `"march"`: the unit to go for.
    focus: Option<UnitId>,
    /// Scripted victories and defeats this unit can bring about by moving.
    script: Vec<Scripted>,
    attack_cache: HashMap<UnitId, AttackInfo>,
    /// Enemy-targeted single/cross strategies do not depend on the caster's tile.
    aim_cache: HashMap<(usize, Pos), Option<AimValue>>,
}

impl<'a> Planner<'a> {
    fn new(st: &'a BattleState, pack: &'a Pack, id: UnitId) -> Planner<'a> {
        let me = &st.units[id];
        let board = Board::new(st, pack);
        let reach: Vec<Pos> = if me.moved {
            vec![me.pos]
        } else {
            st.movement_range(pack, id).tiles.keys().copied().collect()
        };
        let mut attack_offsets: Vec<Pos> = Vec::new();
        for o in st.class_of(pack, id).range.offsets().unwrap_or_default() {
            if o != Pos::new(0, 0) && !attack_offsets.contains(&o) {
                attack_offsets.push(o);
            }
        }
        let strategies: Vec<&StrategyDef> = st
            .usable_strategies(pack, id)
            .iter()
            .filter_map(|s| pack.strategy(s))
            .collect();
        let strategy_offsets = strategies
            .iter()
            .map(|s| {
                let mut v: Vec<Pos> = Vec::new();
                for o in s.range.offsets().unwrap_or_default() {
                    if !v.contains(&o) {
                        v.push(o);
                    }
                }
                v
            })
            .collect();
        let focus = match me.ai {
            AiMode::Target | AiMode::March => me
                .ai_target
                .as_deref()
                .and_then(|r| st.units.iter().find(|u| u.is_active() && u.matches(r)))
                .map(|u| u.id),
            _ => None,
        };
        let mut planner = Planner {
            st,
            pack,
            id,
            me,
            board,
            reach,
            threat: Vec::new(),
            lord_threat: Vec::new(),
            atk: st.attack_power(pack, id),
            def: st.defense_power(pack, id),
            attack_offsets,
            strategies,
            strategy_offsets,
            focus,
            script: scripted_endings(st, pack, me),
            attack_cache: HashMap::new(),
            aim_cache: HashMap::new(),
        };
        (planner.threat, planner.lord_threat) = planner.compute_threats();
        planner
    }

    fn plan(mut self) -> Vec<Action> {
        let origin = self.me.pos;
        let mut reach = std::mem::take(&mut self.reach);
        if !self.script.is_empty() {
            let winning = reach
                .iter()
                .copied()
                .filter(|&p| self.scripted_at(p) == Some(true))
                .min_by_key(|&p| (p != origin, self.danger(p), p));
            if let Some(tile) = winning {
                // The move ends the battle (events fire after it); the wait is never played.
                let mut out = Vec::with_capacity(2);
                if tile != origin {
                    out.push(Action::Move {
                        unit: self.id,
                        to: tile,
                    });
                }
                out.push(Action::Wait { unit: self.id });
                return out;
            }
            if reach.iter().any(|&p| self.scripted_at(p) != Some(false)) {
                reach.retain(|&p| self.scripted_at(p) != Some(false));
            }
        }
        if self.me.lord {
            reach = self.lord_tiles(reach);
        }
        let (tile, action) = match self.me.ai {
            AiMode::Hold => self.act_at(origin),
            AiMode::Aggressive => self.aggressive(&reach),
            AiMode::Defensive => self.defensive(&reach),
            AiMode::Guard => {
                let center = self.me.ai_pos.unwrap_or(origin);
                if origin.manhattan(center) <= GUARD_RADIUS {
                    let zone: Vec<Pos> = reach
                        .iter()
                        .copied()
                        .filter(|p| p.manhattan(center) <= GUARD_RADIUS)
                        .collect();
                    match self.best_from(&zone, None).0 {
                        Some(c) => (c.tile, Some(c.action)),
                        None => (origin, None),
                    }
                } else {
                    self.approach_and_act(&[center], &reach)
                }
            }
            AiMode::Target => match self.focus {
                Some(target) => match self.best_from(&reach, Some(target)).0 {
                    Some(c) => (c.tile, Some(c.action)),
                    None => self.approach_and_act(&[self.st.units[target].pos], &reach),
                },
                None => self.aggressive(&reach),
            },
            AiMode::Advance => match self.me.ai_pos {
                None => self.aggressive(&reach),
                Some(p) => self.advance(p, &reach),
            },
            AiMode::Flee => {
                let tile = self.flee_tile(&reach);
                self.act_at(tile)
            }
            AiMode::March => {
                let goal = match self.focus {
                    Some(target) => Some(self.st.units[target].pos),
                    None => self.me.ai_pos,
                };
                match goal {
                    Some(g) if g == origin => (origin, None),
                    // A destination tile is entered, not just reached: `approach` stops next to it.
                    Some(g) if self.focus.is_none() && reach.contains(&g) => (g, None),
                    Some(g) => (self.approach(&[g], &reach), None),
                    None => (origin, None),
                }
            }
        };
        let mut out = Vec::with_capacity(2);
        if tile != origin {
            out.push(Action::Move {
                unit: self.id,
                to: tile,
            });
        }
        out.push(action.unwrap_or(Action::Wait { unit: self.id }));
        out
    }

    // ----- modes ---------------------------------------------------------------------------

    fn aggressive(&mut self, reach: &[Pos]) -> (Pos, Option<Action>) {
        let best = self.best_from(reach, None).0;
        if !self.careful() {
            if let Some(c) = best {
                return (c.tile, Some(c.action));
            }
        }
        let idle = self.idle_tile(reach);
        match best {
            Some(c) if c.score >= self.position_value(idle) => (c.tile, Some(c.action)),
            _ => (idle, None),
        }
    }

    /// Where an aggressive unit goes when it does not act: towards a scripted victory it can
    /// bring about, otherwise towards the nearest hostile unit. A lord stays with its army
    /// rather than leading the charge, and without an army it keeps to the best position it
    /// can reach.
    /// Tiles to head for to bring about `place`: the partners' tiles for an `adjacent` trigger
    /// (standing next to one is enough), every tile of a `reach` area this unit can stand on
    /// (an impassable tile of the area must not look like an arrival).
    fn place_goals(&self, place: &Place) -> Vec<Pos> {
        match place {
            Place::NextTo(partners) => partners.clone(),
            &Place::Near { pos, radius, to } => {
                let (lo, hi) = match to {
                    Some(to) => (
                        Pos::new(pos.x.min(to.x), pos.y.min(to.y)),
                        Pos::new(pos.x.max(to.x), pos.y.max(to.y)),
                    ),
                    None => (pos.offset(-radius, -radius), pos.offset(radius, radius)),
                };
                let move_type = &self.st.class_of(self.pack, self.id).move_type;
                (lo.y..=hi.y)
                    .flat_map(|y| (lo.x..=hi.x).map(move |x| Pos::new(x, y)))
                    .filter(|&t| in_reach(pos, radius, to, t))
                    .filter(|&t| {
                        self.board
                            .index(t)
                            .and_then(|i| self.board.terrain_at_index(i))
                            .and_then(|terrain| terrain.move_cost(move_type))
                            .is_some()
                    })
                    .collect()
            }
        }
    }

    fn idle_tile(&self, reach: &[Pos]) -> Pos {
        let (tile, goals) = self.idle_goal_tile(reach);
        if self.careful() {
            let resting = self.heals(self.me.pos) && self.below_pct(HEAL_UNTIL_PCT);
            if resting || (self.below_pct(HEAL_BELOW_PCT) && !self.progresses(tile, &goals)) {
                if let Some(heal) = self.heal_tile(reach) {
                    return heal;
                }
            }
        }
        tile
    }

    /// The idle move towards the unit's goal, and the goal tiles.
    fn idle_goal_tile(&self, reach: &[Pos]) -> (Pos, Vec<Pos>) {
        let objective: Vec<Pos> = self
            .script
            .iter()
            .filter(|s| s.wins)
            .flat_map(|s| self.place_goals(&s.place))
            .collect();
        if !objective.is_empty() {
            return (self.approach(&objective, reach), objective);
        }
        let goals = if self.me.lord {
            self.friend_positions()
        } else {
            self.hostile_positions()
        };
        let tile = match (goals.is_empty(), self.me.lord) {
            (false, _) => self.approach(&goals, reach),
            (true, true) => self.best_position(reach),
            (true, false) => self.me.pos,
        };
        (tile, goals)
    }

    /// Whether moving to `tile` brings the unit closer to `goals` along its paths.
    fn progresses(&self, tile: Pos, goals: &[Pos]) -> bool {
        if goals.is_empty() || tile == self.me.pos {
            return false;
        }
        let dist = self.goal_distance(goals);
        let at = |p: Pos| self.board.index(p).map_or(i32::MAX, |i| dist[i]);
        at(tile) < at(self.me.pos)
    }

    fn below_pct(&self, pct: i64) -> bool {
        (self.me.hp as i64) * 100 < self.me.max_hp as i64 * pct
    }

    /// Whether standing on `tile` restores HP to this unit (a small max HP can round the
    /// terrain's percentage down to nothing).
    fn heals(&self, tile: Pos) -> bool {
        self.board
            .terrain(tile)
            .is_some_and(|t| self.me.max_hp as i64 * t.heal_hp.max(0) as i64 / 100 > 0)
    }

    /// A healing tile for a careful unit that cannot get closer to its goal while badly hurt,
    /// or is already resting on one: the tile it stands on, else the safest one in reach, else
    /// a step towards the nearest free one it can walk to; `None` when there is none. Without
    /// this a hurt unit facing units that `hold` finds every tile towards them deadly and
    /// waits where it is for the rest of the battle.
    fn heal_tile(&self, reach: &[Pos]) -> Option<Pos> {
        let hp = self.me.hp as i64;
        // As safe as the unit's own moves must be: a lord keeps its two-phase margin.
        let safe = |p: Pos| {
            if self.me.lord {
                self.lord_safe(p, 0)
            } else {
                self.danger(p) < hp
            }
        };
        let safe_heal = |p: Pos| self.heals(p) && safe(p);
        let origin = self.me.pos;
        if let Some(tile) = reach
            .iter()
            .copied()
            .filter(|&p| safe_heal(p))
            .min_by_key(|&p| (p != origin, self.threat_at(p), p))
        {
            return Some(tile);
        }
        // Tiles this unit can stand on: entering a goal costs nothing in `goal_distance`, so an
        // impassable one would look reachable from next to it.
        let move_type = &self.st.class_of(self.pack, self.id).move_type;
        let enterable = |p: Pos| {
            self.board
                .terrain(p)
                .is_some_and(|t| t.move_cost(move_type).is_some())
        };
        let goals: Vec<Pos> = (0..self.board.len())
            .map(|i| self.board.pos_of(i))
            .filter(|&p| safe_heal(p) && enterable(p) && self.occupant(p, origin).is_none())
            .collect();
        // Only a healing tile it can walk to: `approach` falls back to straight-line distance.
        let dist = self.goal_distance(&goals);
        let walkable = self.board.index(origin).is_some_and(|i| dist[i] < i32::MAX);
        walkable.then(|| self.approach(&goals, reach))
    }

    /// The scripted ending standing on `tile` brings about: `Some(false)` for a defeat (which
    /// takes precedence), `Some(true)` for a victory.
    fn scripted_at(&self, tile: Pos) -> Option<bool> {
        let mut outcome = None;
        for s in self.script.iter().filter(|s| s.place.holds(tile)) {
            if !s.wins {
                return Some(false);
            }
            outcome = Some(true);
        }
        outcome
    }

    /// Tile of `tiles` with the best position value, preferring to stay, then position order.
    fn best_position(&self, tiles: &[Pos]) -> Pos {
        let origin = self.me.pos;
        tiles
            .iter()
            .copied()
            .max_by_key(|&p| (self.position_value(p), p == origin, Reverse(p)))
            .unwrap_or(origin)
    }

    /// Player units, which the AI only commands in simulations, and lords are played the way
    /// a careful human plays them *(design)*: they avoid tiles where they would be defeated
    /// and act only when that beats just moving on.
    fn careful(&self) -> bool {
        self.me.side == Side::Player || self.me.lord
    }

    /// Fight only when a hostile unit can be reached this phase; otherwise stay (supporting
    /// friends from the current tile).
    fn defensive(&mut self, reach: &[Pos]) -> (Pos, Option<Action>) {
        let (best, hostile_in_reach) = self.best_from(reach, None);
        match best {
            Some(c) if hostile_in_reach => (c.tile, Some(c.action)),
            _ => self.act_at(self.me.pos),
        }
    }

    /// `advance` towards `dest`. A destination in reach is entered (`approach` stops next to
    /// it) and the unit acts from there. Near it (within [`GUARD_RADIUS`]) the unit holds it
    /// like a `guard` post: it takes the best action from the tiles within that radius (the
    /// destination winning ties, so a reach trigger there still fires), else returns to the
    /// destination. It never chases beyond the post's radius, so a sortie is followed by more
    /// fighting or by the way back, not by a sortie every other turn.
    fn advance(&mut self, dest: Pos, reach: &[Pos]) -> (Pos, Option<Action>) {
        let origin = self.me.pos;
        // `reach` is already filtered (lord safety, scripted defeats): the destination is
        // entered only when it survives that.
        let dest_open = reach.contains(&dest);
        if origin.manhattan(dest) > GUARD_RADIUS {
            return if dest_open {
                self.act_at(dest)
            } else {
                self.approach_and_act(&[dest], reach)
            };
        }
        let zone: Vec<Pos> = reach
            .iter()
            .copied()
            .filter(|p| p.manhattan(dest) <= GUARD_RADIUS)
            .collect();
        let best = self.best_from(&zone, None).0;
        let at_dest = if dest_open {
            self.best_from(&[dest], None).0
        } else {
            None
        };
        match (best, at_dest) {
            (Some(b), Some(d)) if d.score >= b.score => (dest, Some(d.action)),
            (Some(b), _) => (b.tile, Some(b.action)),
            (None, _) if dest_open => (dest, None),
            // The destination is taken (or unsafe): wait as close as the post's radius allows.
            (None, _) => {
                let near = if zone.is_empty() { reach } else { &zone };
                self.approach_and_act(&[dest], near)
            }
        }
    }

    fn act_at(&mut self, tile: Pos) -> (Pos, Option<Action>) {
        (tile, self.best_from(&[tile], None).0.map(|c| c.action))
    }

    fn approach_and_act(&mut self, goals: &[Pos], reach: &[Pos]) -> (Pos, Option<Action>) {
        let tile = self.approach(goals, reach);
        self.act_at(tile)
    }

    // ----- candidates ----------------------------------------------------------------------

    /// Best action from any of `tiles` (only actions involving `focus` when given), and
    /// whether any hostile unit can be attacked or hit by a strategy from them.
    fn best_from(&mut self, tiles: &[Pos], focus: Option<UnitId>) -> (Option<Choice>, bool) {
        let mut best: Option<Choice> = None;
        let mut hostile_in_reach = false;
        for &tile in tiles {
            let position = self.position_value(tile);
            for oi in 0..self.attack_offsets.len() {
                let o = self.attack_offsets[oi];
                let Some(t) = self.occupant(tile.offset(o.x, o.y), tile) else {
                    continue;
                };
                if !self.is_hostile(t) {
                    continue;
                }
                hostile_in_reach = true;
                if focus.is_some_and(|f| f != t) || (self.me.lord && !self.lord_may_attack(tile, t))
                {
                    continue;
                }
                let score = self.attack_value(tile, t) + position;
                offer(
                    &mut best,
                    Choice {
                        score,
                        key: t,
                        tile,
                        action: Action::Attack {
                            unit: self.id,
                            target: t,
                        },
                    },
                    self.me.pos,
                );
            }
            for si in 0..self.strategies.len() {
                let s = self.strategies[si];
                let aims: Vec<Pos> = match s.area {
                    Area::AllInRange => vec![tile],
                    Area::Single | Area::Cross => self.strategy_offsets[si]
                        .iter()
                        .map(|o| tile.offset(o.x, o.y))
                        .collect(),
                };
                for aim in aims {
                    let Some(av) = self.aim_value(si, tile, aim) else {
                        continue;
                    };
                    hostile_in_reach |= av.hostile;
                    if av.value <= 0 || (focus.is_some() && !av.hits_focus) {
                        continue;
                    }
                    offer(
                        &mut best,
                        Choice {
                            score: av.value - s.mp.max(0) as i64 + position,
                            key: av.key,
                            tile,
                            action: Action::Strategy {
                                unit: self.id,
                                strategy: s.id.clone(),
                                target: aim,
                            },
                        },
                        self.me.pos,
                    );
                }
            }
            if focus.is_none() {
                self.item_choices(tile, position, &mut best);
            }
        }
        (best, hostile_in_reach)
    }

    fn attack_value(&mut self, tile: Pos, target: UnitId) -> i64 {
        let value = self.attack_info(target).value;
        match self.counter_at(tile, target) {
            Some((dmg, chance)) => value - dmg.min(self.me.hp as i64) * chance / 100,
            None => value,
        }
    }

    /// Damage and chance (percent) of the counter-attack provoked by attacking `target` from
    /// `tile`, when there can be one.
    fn counter_at(&mut self, tile: Pos, target: UnitId) -> Option<(i64, i64)> {
        self.attack_info(target);
        let c = self.attack_cache[&target].counter.as_ref()?;
        let t_pos = self.st.units[target].pos;
        let delta = Pos::new(tile.x - t_pos.x, tile.y - t_pos.y);
        if tile.chebyshev(t_pos) != 1 || !c.offsets.contains(&delta) || c.chance <= 0 {
            return None;
        }
        let terrain = self.board.terrain(tile).map_or(0, |t| t.defense);
        let dmg = hit_damage(c.atk, self.def, c.affinity, terrain) as i64;
        Some((
            (dmg * self.pack.rules.counter_damage_pct as i64 / 100).max(1),
            c.chance as i64,
        ))
    }

    /// A lord attacks only from a tile that stays safe after a counter-attack.
    fn lord_may_attack(&mut self, tile: Pos, target: UnitId) -> bool {
        let counter = self.counter_at(tile, target).map_or(0, |(dmg, _)| dmg);
        self.lord_safe(tile, counter)
    }

    /// Tile-independent facts about attacking `target` (computed once per plan).
    fn attack_info(&mut self, target: UnitId) -> &AttackInfo {
        if !self.attack_cache.contains_key(&target) {
            let info = self.compute_attack_info(target);
            self.attack_cache.insert(target, info);
        }
        &self.attack_cache[&target]
    }

    fn compute_attack_info(&self, target: UnitId) -> AttackInfo {
        let (st, pack) = (self.st, self.pack);
        let t = &st.units[target];
        let terrain = self.board.terrain(t.pos).map_or(0, |tt| tt.defense);
        // The joint attack bonus does not depend on the tile attacked from (extended rules).
        let dmg = with_joint_attack(
            hit_damage(
                self.atk,
                st.defense_power(pack, target),
                st.affinity(pack, self.id, target),
                terrain,
            ),
            st.joint_attack_pct(self.id, target),
        );
        let kill = dmg >= t.hp;
        let mut value = damage_value(dmg, t) + self.protect_value(target, dmg, t.hp);
        // The original formulas: a blow leaving little morale confuses too (as strategy damage).
        let left = t.morale - morale_loss(&pack.rules, dmg, t.max_hp).min(t.morale);
        if !kill && !t.has_status(StatusKind::Confused) {
            value += self.fall_confusion_value(target, t.morale, left, t.hp - dmg);
        }
        let mut counter = None;
        let t_class = st.class_of(pack, target);
        // (Under the original formulas a confused target does not counter.)
        let confused_quiet =
            strategy::original_formulas(pack) && t.has_status(StatusKind::Confused);
        if !kill
            && !confused_quiet
            && t_class.can_counter
            && st.class_of(pack, self.id).provokes_counter
        {
            let morale = t.morale - morale_loss(&pack.rules, dmg, t.max_hp).min(t.morale);
            if !(morale == 0 && t.has_status(StatusKind::Confused)) {
                counter = Some(CounterInfo {
                    offsets: t_class.range.offsets().unwrap_or_default(),
                    chance: st.counter_odds(pack, target, t.morale, morale),
                    atk: st.attack_with_morale(pack, target, morale),
                    affinity: st.affinity(pack, target, self.id),
                });
            }
        }
        AttackInfo { value, counter }
    }

    fn aim_value(&mut self, si: usize, tile: Pos, aim: Pos) -> Option<AimValue> {
        let s = self.strategies[si];
        let cacheable = s.target == TargetSide::Enemy && s.area != Area::AllInRange;
        if cacheable {
            if let Some(v) = self.aim_cache.get(&(si, aim)) {
                return *v;
            }
        }
        let v = self.compute_aim(si, tile, aim);
        if cacheable {
            self.aim_cache.insert((si, aim), v);
        }
        v
    }

    fn compute_aim(&self, si: usize, tile: Pos, aim: Pos) -> Option<AimValue> {
        let s = self.strategies[si];
        if !self.board.in_bounds(aim) {
            return None;
        }
        let area: Vec<Pos> = match s.area {
            Area::Single => vec![aim],
            Area::Cross => std::iter::once(aim).chain(aim.neighbors4()).collect(),
            Area::AllInRange => self.strategy_offsets[si]
                .iter()
                .map(|o| tile.offset(o.x, o.y))
                .collect(),
        };
        if s.area != Area::AllInRange && !self.st.element_allows(s, self.board.terrain(aim)) {
            return None;
        }
        let mut out: Option<AimValue> = None;
        for p in area {
            let Some(u) = self.occupant(p, tile) else {
                continue;
            };
            let valid = match s.target {
                TargetSide::Enemy => self.is_hostile(u),
                TargetSide::Ally => !self.is_hostile(u),
            };
            let terrain = self.board.terrain(p);
            if !valid || !self.st.element_allows(s, terrain) {
                continue;
            }
            let value = self.unit_value(s, u, terrain);
            let hostile = self.is_hostile(u);
            let hits_focus = self.focus == Some(u);
            out = Some(match out {
                None => AimValue {
                    value,
                    key: u,
                    hostile,
                    hits_focus,
                },
                Some(a) => AimValue {
                    value: a.value + value,
                    key: a.key.min(u),
                    hostile: a.hostile || hostile,
                    hits_focus: a.hits_focus || hits_focus,
                },
            });
        }
        out
    }

    /// Under the original formulas, the expected value of hostile unit `u`'s morale falling from
    /// `before` to `after` with `hp` left: below 30 it is confused with 60 % (a skipped phase,
    /// and a rout at 0), as `BattleState::morale_set` rolls it. Nothing otherwise.
    fn fall_confusion_value(&self, u: UnitId, before: i32, after: i32, hp: i32) -> i64 {
        let (st, pack) = (self.st, self.pack);
        if !strategy::original_formulas(pack)
            || after >= before
            || after >= strategy::MORALE_DOWN_CONFUSES_BELOW
        {
            return 0;
        }
        let odds = strategy::MORALE_DOWN_CONFUSION as i64;
        let mut v = st.attack_power(pack, u) as i64 / 2 * odds / 100;
        if after == 0 {
            v += hp as i64 * odds / 100;
        }
        v
    }

    /// Expected value of `s`'s effects on unit `u` standing on `terrain`.
    fn unit_value(&self, s: &StrategyDef, u: UnitId, terrain: Option<&TerrainDef>) -> i64 {
        let (st, pack) = (self.st, self.pack);
        let t = &st.units[u];
        let sign: i64 = if self.is_hostile(u) { 1 } else { -1 };
        let chance = match s.target {
            TargetSide::Enemy => st.hit_chance(pack, self.id, s, u),
            TargetSide::Ally => 100,
        } as i64;
        let level_factor = t.level as i64 + 10;
        let (mut hp, mut morale, mut confused) =
            (t.hp, t.morale, t.has_status(StatusKind::Confused));
        let mut v: i64 = 0;
        for e in &s.effects {
            if hp <= 0 {
                break;
            }
            match e {
                Effect::Damage { power } => {
                    let dmg = st.strategy_damage_base(pack, self.id, s, *power, u, terrain);
                    let mut value = dmg as i64;
                    if dmg >= hp {
                        value *= 3;
                    }
                    if t.lord || t.commander {
                        value = value * 3 / 2;
                    }
                    if sign > 0 {
                        value += self.protect_value(u, dmg, hp);
                    }
                    v += sign * value;
                    hp -= dmg.min(hp);
                    let new = morale - morale_loss(&pack.rules, dmg, t.max_hp).min(morale);
                    if sign > 0 && hp > 0 && !confused {
                        v += self.fall_confusion_value(u, morale, new, hp);
                    }
                    morale = new;
                }
                Effect::Heal { power } => {
                    let heal = st
                        .strategy_heal(pack, self.id, *power, u)
                        .min(t.max_hp - hp)
                        .max(0);
                    v -= sign * heal as i64;
                    hp += heal;
                }
                Effect::Morale { amount } => {
                    let delta = st.morale_shift(pack, self.id, u, *amount);
                    let new = morale.saturating_add(delta).clamp(0, 100);
                    // Morale enters ATK/DEF as `(level + 10) * morale / 10`.
                    let change = (new - morale) as i64 * level_factor / 10;
                    v += if sign > 0 { -change } else { change / 2 };
                    if sign > 0 && delta < 0 && !confused {
                        v += self.fall_confusion_value(u, morale, new, hp);
                    }
                    morale = new;
                }
                Effect::Status { .. } => {
                    if !confused {
                        confused = true;
                        // A confused unit skips its phase: roughly the damage it would deal.
                        v += sign * st.attack_power(pack, u) as i64 / 2;
                    }
                }
                Effect::Promote | Effect::ChangeClass { .. } => {}
            }
        }
        if sign > 0 && confused && morale == 0 && hp > 0 {
            v += hp as i64; // routed (§6)
        }
        v * chance / 100
    }

    /// Healing / morale consumables (player side only: the inventory is the player's).
    fn item_choices(&self, tile: Pos, position: i64, best: &mut Option<Choice>) {
        if self.me.side != Side::Player {
            return;
        }
        for (item, &count) in &self.st.inventory {
            let Some(def) = self.pack.item(item) else {
                continue;
            };
            let usable = count > 0
                && def.is_battle_item()
                && def.strategy.is_none()
                && !def.effects.is_empty()
                && def
                    .effects
                    .iter()
                    .all(|e| matches!(e, Effect::Heal { .. } | Effect::Morale { .. }));
            if !usable {
                continue;
            }
            let targets = std::iter::once(tile)
                .chain(tile.neighbors4())
                .filter_map(|p| self.occupant(p, tile))
                .filter(|&u| !self.is_hostile(u));
            for u in targets {
                let value = self.item_value(def, u);
                if value > 0 {
                    offer(
                        best,
                        Choice {
                            score: value + position,
                            key: u,
                            tile,
                            action: Action::UseItem {
                                unit: self.id,
                                item: item.clone(),
                                target: u,
                            },
                        },
                        self.me.pos,
                    );
                }
            }
        }
    }

    /// Items are finite, so they are only worth half their effect and only on units in need
    /// (HP below half, morale in the confusion zone).
    fn item_value(&self, def: &ItemDef, u: UnitId) -> i64 {
        let t = &self.st.units[u];
        let mut v: i64 = 0;
        for e in &def.effects {
            match e {
                Effect::Heal { power } if t.hp * 2 < t.max_hp => {
                    v += (*power).min(t.max_hp - t.hp).max(0) as i64;
                }
                Effect::Morale { amount } if t.morale <= self.pack.rules.confuse_morale => {
                    let gain = (*amount).min(100 - t.morale).max(0) as i64;
                    v += gain * (t.level as i64 + 10) / 10;
                }
                _ => {}
            }
        }
        v / 2
    }

    // ----- positions -----------------------------------------------------------------------

    /// Unit on `p` if this unit stood on `tile` (its real tile is then empty).
    fn occupant(&self, p: Pos, tile: Pos) -> Option<UnitId> {
        if p == tile {
            Some(self.id)
        } else {
            self.board.unit_at(p).filter(|&u| u != self.id)
        }
    }

    fn is_hostile(&self, u: UnitId) -> bool {
        self.st.units[u].side.is_hostile(self.me.side)
    }

    fn hostile_positions(&self) -> Vec<Pos> {
        self.st
            .units
            .iter()
            .filter(|u| u.is_active() && u.side.is_hostile(self.me.side))
            .map(|u| u.pos)
            .collect()
    }

    /// Tiles of the other active units of this unit's side and its friends.
    fn friend_positions(&self) -> Vec<Pos> {
        self.st
            .units
            .iter()
            .filter(|u| u.is_active() && u.id != self.id && !self.is_hostile(u.id))
            .map(|u| u.pos)
            .collect()
    }

    /// Expected damage every hostile unit could deal next phase: to this unit on each tile,
    /// and (per hostile unit) to this unit's lord where the lord stands.
    ///
    /// A hostile unit threatens the tiles its attack range covers from every tile it can move
    /// to, and the tiles its damage strategies can hit (element gate included, weighted by the
    /// hit chance); on a tile both could reach, the stronger one counts. Hostile movement is
    /// blocked by the units that stay where they are, but not by this unit or the units of its
    /// side that can still act this phase, since they may move away. Units still confused
    /// next phase are ignored.
    fn compute_threats(&self) -> (Vec<i64>, Vec<i64>) {
        let (st, pack) = (self.st, self.pack);
        let side = self.me.side;
        let may_leave = |u: UnitId| st.units[u].side == side && st.can_act(u) && !st.units[u].moved;
        let others = Board::without(st, pack, |u| {
            u == self.id || (self.careful() && may_leave(u))
        });
        let n = self.board.len();
        // This unit's lord and the index of its tile.
        let lord = st
            .units
            .iter()
            .filter(|u| u.is_active() && u.lord && u.id != self.id && !self.is_hostile(u.id))
            .find_map(|u| self.board.index(u.pos).map(|i| (u, i)));
        let mut threat = vec![0i64; n];
        let mut lord_threat = vec![0i64; st.units.len()];
        let mut cover = TileSet::new(n);
        // The current hostile unit's strongest damage per tile it threatens.
        let mut touched = TileSet::new(n);
        let mut strongest = vec![0i64; n];
        let keep = |touched: &mut TileSet, strongest: &mut [i64], i: usize, dmg: i64| {
            strongest[i] = if touched.insert(i) {
                dmg
            } else {
                strongest[i].max(dmg)
            };
        };
        // A marching AI unit never attacks (player units are commanded by a human).
        for h in st
            .units
            .iter()
            .filter(|h| h.is_active() && self.is_hostile(h.id))
            .filter(|h| h.side == Side::Player || h.ai != AiMode::March)
        {
            if h.statuses
                .iter()
                // Sure to stay confused next phase (one without a length may recover).
                .any(|s| {
                    s.status == StatusKind::Confused
                        && s.turns >= 2
                        && s.turns != super::UNTIL_RECOVERED
                })
            {
                continue;
            }
            let ends = self.threat_origins(&others, h);
            touched.clear();
            let mut to_lord = 0i64;

            if let Some(offsets) = st.class_of(pack, h.id).range.offsets() {
                cover.clear();
                for e in &ends {
                    for o in offsets.iter().filter(|o| **o != Pos::new(0, 0)) {
                        if let Some(i) = self.board.index(e.offset(o.x, o.y)) {
                            cover.insert(i);
                        }
                    }
                }
                let atk = st.attack_power(pack, h.id);
                // A threat estimate: under extended rules the joint attack bonus depends on
                // where the others end up, so it is counted at its most (a safe tile stays safe).
                let joint = if st.extended_rules {
                    JOINT_ATTACK_MAX
                } else {
                    0
                };
                let hit = |target: UnitId, def: i32, i: usize| {
                    let terrain = self.board.terrain_at_index(i).map_or(0, |t| t.defense);
                    let dmg = hit_damage(atk, def, st.affinity(pack, h.id, target), terrain);
                    with_joint_attack(dmg, joint) as i64
                };
                for &i in &cover.tiles {
                    keep(&mut touched, &mut strongest, i, hit(self.id, self.def, i));
                }
                if let Some((l, li)) = lord.filter(|&(_, li)| cover.contains(li)) {
                    to_lord = to_lord.max(hit(l.id, st.defense_power(pack, l.id), li));
                }
            }

            // Known and affordable (a confusion that ends before its phase does not matter).
            for s in pack
                .known_strategies(&h.class, h.level)
                .iter()
                .filter_map(|s| pack.strategy(s))
            {
                let harmful = s.target == TargetSide::Enemy
                    && s.effects.iter().any(|e| matches!(e, Effect::Damage { .. }));
                if !harmful || h.mp < s.mp {
                    continue;
                }
                cover.clear();
                let offsets = super::strategy::reach_tiles(s, Pos::new(0, 0));
                for e in &ends {
                    for o in &offsets {
                        let aim = e.offset(o.x, o.y);
                        if let Some(i) = self.board.index(aim) {
                            cover.insert(i);
                        }
                        if s.area == Area::Cross {
                            for i in aim
                                .neighbors4()
                                .into_iter()
                                .filter_map(|p| self.board.index(p))
                            {
                                cover.insert(i);
                            }
                        }
                    }
                }
                for &i in &cover.tiles {
                    let terrain = self.board.terrain_at_index(i);
                    if st.element_allows(s, terrain) {
                        let dmg = self.strategy_threat(h.id, s, self.id, terrain);
                        keep(&mut touched, &mut strongest, i, dmg);
                    }
                }
                if let Some((l, li)) = lord.filter(|&(_, li)| cover.contains(li)) {
                    let terrain = self.board.terrain_at_index(li);
                    if st.element_allows(s, terrain) {
                        to_lord = to_lord.max(self.strategy_threat(h.id, s, l.id, terrain));
                    }
                }
            }

            for &i in &touched.tiles {
                threat[i] += strongest[i];
            }
            lord_threat[h.id] = to_lord;
        }
        (threat, lord_threat)
    }

    /// Tiles hostile unit `h` may act from during its next phase: its movement range, except
    /// that an AI-controlled `hold` unit stays on its tile and a `guard` at its post (or an
    /// `advance` near its destination) stays within [`GUARD_RADIUS`] of it. Player units are
    /// commanded by a human, whatever their `ai` field says.
    fn threat_origins(&self, board: &Board, h: &Unit) -> Vec<Pos> {
        let ai = if h.side == Side::Player {
            AiMode::Aggressive
        } else {
            h.ai
        };
        if ai == AiMode::Hold {
            return vec![h.pos];
        }
        let range = self.st.reach(
            self.pack,
            board,
            h.id,
            h.pos,
            self.st.base_move_points(self.pack, h.id),
        );
        let post = match (ai, h.ai_pos) {
            (AiMode::Guard | AiMode::Advance, Some(c)) if h.pos.manhattan(c) <= GUARD_RADIUS => {
                Some(c)
            }
            _ => None,
        };
        range
            .tiles
            .into_keys()
            .filter(|p| post.is_none_or(|c| p.manhattan(c) <= GUARD_RADIUS))
            .collect()
    }

    /// Expected damage of `caster`'s strategy `s` on `target` standing on `terrain`.
    fn strategy_threat(
        &self,
        caster: UnitId,
        s: &StrategyDef,
        target: UnitId,
        terrain: Option<&TerrainDef>,
    ) -> i64 {
        let (st, pack) = (self.st, self.pack);
        let damage: i64 = s
            .effects
            .iter()
            .filter_map(|e| match e {
                Effect::Damage { power } => {
                    Some(st.strategy_damage_base(pack, caster, s, *power, target, terrain) as i64)
                }
                _ => None,
            })
            .sum();
        damage * st.hit_chance(pack, caster, s, target) as i64 / 100
    }

    /// Expected damage taken on `tile` next phase (not capped at the unit's HP).
    fn danger(&self, tile: Pos) -> i64 {
        self.board.index(tile).map_or(0, |i| self.threat[i])
    }

    /// Expected damage taken on `tile` next phase, at most the unit's HP.
    fn threat_at(&self, tile: Pos) -> i64 {
        self.danger(tile).clamp(0, self.me.hp as i64)
    }

    /// Whether the lord can stand on `tile` after taking `extra` damage now: the hostile units
    /// must not be expected to defeat it there within their next two phases, which leaves it
    /// room to get away after one of them.
    fn lord_safe(&self, tile: Pos, extra: i64) -> bool {
        extra + LORD_SAFETY_PHASES * self.danger(tile) < self.me.hp as i64
    }

    /// Lord safety: the safe tiles of `reach`, or the least dangerous ones when none is safe.
    fn lord_tiles(&self, reach: Vec<Pos>) -> Vec<Pos> {
        if reach.iter().any(|&p| self.lord_safe(p, 0)) {
            return reach
                .into_iter()
                .filter(|&p| self.lord_safe(p, 0))
                .collect();
        }
        let least = reach.iter().map(|&p| self.danger(p)).min();
        reach
            .into_iter()
            .filter(|&p| Some(self.danger(p)) == least)
            .collect()
    }

    /// Value of damaging `target` (which has `hp` left) for protecting this unit's lord: the
    /// damage the target could deal to the lord next phase when it is defeated, a share of it
    /// otherwise.
    fn protect_value(&self, target: UnitId, damage: i32, hp: i32) -> i64 {
        let to_lord = self.lord_threat.get(target).copied().unwrap_or(0);
        if to_lord == 0 || hp <= 0 {
            0
        } else if damage >= hp {
            to_lord
        } else {
            to_lord * damage as i64 / (2 * hp as i64)
        }
    }

    /// Terrain defence, expected regeneration if hurt, minus half the expected damage taken.
    /// For careful units being defeated there costs twice their max HP; a lord (which avoids
    /// such tiles altogether) weighs the full expected damage and values friends next to it.
    fn position_value(&self, tile: Pos) -> i64 {
        let terrain = self.board.terrain(tile);
        let mut v = terrain.map_or(0, |t| t.defense) as i64;
        let missing = (self.me.max_hp - self.me.hp) as i64;
        if let (true, Some(t)) = (missing > 0, terrain) {
            v += (self.me.max_hp as i64 * t.heal_hp.max(0) as i64 / 100).min(missing) / 2;
        }
        if self.me.lord {
            let escort = tile
                .neighbors4()
                .into_iter()
                .filter_map(|p| self.occupant(p, tile))
                .filter(|&u| !self.is_hostile(u))
                .count() as i64;
            return v - self.threat_at(tile)
                + escort * self.me.max_hp as i64 * LORD_ESCORT_PCT / 100;
        }
        v -= self.threat_at(tile) / 2;
        if self.careful() && self.danger(tile) >= self.me.hp as i64 {
            // A defeated unit is missing for the rest of the battle.
            v -= 2 * self.me.max_hp as i64;
        }
        v
    }

    /// Tile of `allowed` closest to any goal along the cheapest path for this unit's move
    /// type (ignoring units); manhattan distance when no goal is reachable at all. Ties prefer
    /// less threat, more terrain defence, staying put, then position order. Careful units keep
    /// off tiles where they would be defeated when they can.
    fn approach(&self, goals: &[Pos], allowed: &[Pos]) -> Pos {
        let hp = self.me.hp as i64;
        let survivable: Vec<Pos>;
        let allowed = if self.careful() && allowed.iter().any(|&p| self.danger(p) < hp) {
            survivable = allowed
                .iter()
                .copied()
                .filter(|&p| self.danger(p) < hp)
                .collect();
            &survivable
        } else {
            allowed
        };
        let origin = self.me.pos;
        let dist = self.goal_distance(goals);
        let path_dist = |p: Pos| self.board.index(p).map_or(i32::MAX, |i| dist[i]);
        let reachable = allowed.iter().any(|&p| path_dist(p) < i32::MAX);
        allowed
            .iter()
            .copied()
            .min_by_key(|&p| {
                let d = if reachable {
                    path_dist(p) as i64
                } else {
                    goals.iter().map(|g| g.manhattan(p)).min().unwrap_or(0) as i64
                };
                let defense = self.board.terrain(p).map_or(0, |t| t.defense);
                (d, self.threat_at(p), Reverse(defense), p != origin, p)
            })
            .unwrap_or(origin)
    }

    /// Reverse Dijkstra from the goals: cost of walking from each tile to the nearest goal.
    /// Entering a goal tile is free (the unit only needs to get next to it).
    fn goal_distance(&self, goals: &[Pos]) -> Vec<i32> {
        let n = self.board.len();
        let move_type = &self.st.class_of(self.pack, self.id).move_type;
        let cost: Vec<Option<i32>> = (0..n)
            .map(|i| {
                self.board
                    .terrain_at_index(i)
                    .and_then(|t| t.move_cost(move_type))
                    .map(i32::from)
            })
            .collect();
        let mut dist = vec![i32::MAX; n];
        let mut is_goal = vec![false; n];
        let mut heap = BinaryHeap::new();
        for g in goals {
            if let Some(i) = self.board.index(*g) {
                is_goal[i] = true;
                dist[i] = 0;
                heap.push(Reverse((0i32, i)));
            }
        }
        while let Some(Reverse((d, i))) = heap.pop() {
            if d > dist[i] {
                continue;
            }
            let enter = if is_goal[i] { 0 } else { cost[i].unwrap_or(0) };
            for nb in self.board.pos_of(i).neighbors4() {
                let Some(j) = self.board.index(nb) else {
                    continue;
                };
                if cost[j].is_none() {
                    continue;
                }
                let nd = d.saturating_add(enter);
                if nd < dist[j] {
                    dist[j] = nd;
                    heap.push(Reverse((nd, j)));
                }
            }
        }
        dist
    }

    /// Tile maximising the distance to the nearest hostile unit.
    fn flee_tile(&self, reach: &[Pos]) -> Pos {
        let origin = self.me.pos;
        let hostiles = self.hostile_positions();
        if hostiles.is_empty() {
            return origin;
        }
        reach
            .iter()
            .copied()
            .max_by_key(|&p| {
                let nearest = hostiles.iter().map(|h| h.manhattan(p)).min().unwrap_or(0);
                (nearest, Reverse(self.threat_at(p)), p == origin, Reverse(p))
            })
            .unwrap_or(origin)
    }
}
