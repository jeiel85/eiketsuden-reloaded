//! Turn flow (RULES.md §1), morale and confusion upkeep (§6), band aura (§7.5), weather (§8),
//! battle events, conditions and the outcome (§9).

use super::board::Board;
use super::{
    BattleEvent, BattleState, DefeatReason, MapImage, Outcome, UnitId, UnitState, Weather,
};
use crate::battledef::{in_reach, AiMode, Condition, EventAction, Side, Trigger};
use crate::data::StatusKind;
use crate::geom::Pos;
use crate::pack::Pack;

fn next_side(side: Side) -> Side {
    match side {
        Side::Player => Side::Ally,
        Side::Ally => Side::Enemy,
        Side::Enemy => Side::Player,
    }
}

/// `(turn, side)` of the phase that is starting, for `turn_start` triggers.
type PhaseKey = (u32, Side);

impl BattleState {
    pub(super) fn begin_battle(&mut self, pack: &Pack) -> Vec<BattleEvent> {
        let mut ev = Vec::new();
        if self.outcome.is_some() {
            return ev;
        }
        if let Some(scene) = &self.def(pack).intro {
            ev.push(BattleEvent::Drama {
                scene: scene.clone(),
            });
        }
        if !self.start_phase(pack, Side::Player, &mut ev) {
            self.end_phase(pack, &mut ev);
        }
        ev
    }

    /// End the current phase and start the next one that has units (§1.1, §1.4). After the
    /// enemy phase of the last turn the battle is lost unless a victory condition holds.
    pub(super) fn end_phase(&mut self, pack: &Pack, ev: &mut Vec<BattleEvent>) {
        while self.outcome.is_none() {
            if self.phase == Side::Enemy {
                // The turn is complete: `survive_turns` may now hold.
                self.check_outcome(pack, self.turn, ev);
                if self.outcome.is_some() {
                    return;
                }
                if self.turn >= self.turn_limit {
                    self.lose(DefeatReason::TurnLimit, ev);
                    return;
                }
                self.turn += 1;
            }
            if self.start_phase(pack, next_side(self.phase), ev) {
                return;
            }
        }
    }

    fn has_active(&self, side: Side) -> bool {
        self.units.iter().any(|u| u.is_active() && u.side == side)
    }

    /// Phase start (§1.2). Returns whether the phase is played (it has active units and the
    /// battle is not over); a phase without units only fires its `turn_start` triggers.
    fn start_phase(&mut self, pack: &Pack, side: Side, ev: &mut Vec<BattleEvent>) -> bool {
        self.phase = side;
        let key = Some((self.turn, side));
        let played = self.has_active(side);
        if played {
            ev.push(BattleEvent::PhaseStart {
                side,
                turn: self.turn,
            });
        }
        if side == Side::Player {
            self.roll_weather(pack, ev);
        }
        if played {
            self.regenerate(pack, side, ev);
            self.count_down_statuses(pack, side, ev);
            self.low_morale_confusion(pack, side, ev);
            self.clear_flags(side);
            self.settle_with(pack, key, false, ev);
            return self.outcome.is_none();
        }
        self.settle_with(pack, key, true, ev);
        if self.outcome.is_none() && self.has_active(side) {
            // Reinforcements arrived through a `turn_start` event: the phase is played.
            ev.push(BattleEvent::PhaseStart {
                side,
                turn: self.turn,
            });
            self.clear_flags(side);
            return true;
        }
        false
    }

    fn clear_flags(&mut self, side: Side) {
        for u in self.units.iter_mut().filter(|u| u.side == side) {
            u.moved = false;
            u.acted = false;
        }
    }

    /// Weather roll from `game.toml` chances (§8).
    fn roll_weather(&mut self, pack: &Pack, ev: &mut Vec<BattleEvent>) {
        let w = &pack.rules.weather;
        let (clear, cloudy, rain) = (w.clear.max(0), w.cloudy.max(0), w.rain.max(0));
        let total = clear + cloudy + rain;
        let weather = if total <= 0 {
            Weather::Clear
        } else {
            let r = self.rng.below(total as u32) as i32;
            if r < clear {
                Weather::Clear
            } else if r < clear + cloudy {
                Weather::Cloudy
            } else {
                Weather::Rain
            }
        };
        if weather != self.weather {
            self.weather = weather;
            ev.push(BattleEvent::WeatherChanged { weather });
        }
    }

    /// Terrain and equipment regeneration and the band MP aura (§1.2.2, §7.5).
    fn regenerate(&mut self, pack: &Pack, side: Side, ev: &mut Vec<BattleEvent>) {
        let board = Board::new(self, pack);
        for id in 0..self.units.len() {
            let u = &self.units[id];
            if !u.is_active() || u.side != side {
                continue;
            }
            let pct_of_max = |pct: i32| (u.max_hp as i64 * pct.max(0) as i64 / 100) as i32;
            let (mut hp, mut morale, mut mp) = (0i32, 0i32, 0i32);
            if let Some(t) = board.terrain(u.pos) {
                hp = hp.saturating_add(pct_of_max(t.heal_hp));
                morale = morale.saturating_add(t.heal_morale.max(0));
            }
            for item in u.equip.iter().filter_map(|i| pack.item(i)) {
                hp = hp.saturating_add(pct_of_max(item.regen_hp));
                morale = morale.saturating_add(item.regen_morale.max(0));
            }
            for p in u.pos.neighbors4() {
                if let Some(band) = board.unit_at(p).filter(|&b| self.class_of(pack, b).mp_aura) {
                    mp = mp.saturating_add((self.units[band].level / 10 + 1) as i32);
                }
            }
            let hp = hp.min(u.max_hp - u.hp).max(0);
            let mp = mp.min(u.max_mp - u.mp).max(0);
            // The original sets the morale even when it is already full (a recovery roll).
            let morale_regen = morale > 0;
            let morale = morale.min(100 - u.morale).max(0);
            if hp > 0 || mp > 0 || morale > 0 {
                let u = &mut self.units[id];
                u.hp += hp;
                u.mp += mp;
                u.morale += morale;
                ev.push(BattleEvent::Regenerated {
                    unit: id,
                    hp,
                    mp,
                    morale,
                });
            }
            if morale_regen {
                let before = self.units[id].morale - morale;
                let set = self.morale_set(pack, id, before);
                ev.extend(Self::morale_set_event(id, set));
            }
        }
    }

    /// Status countdown (§1.2.3): confusion persists at 1 while morale is low. Under the
    /// original strategy formulas confusion has no length: it ends when
    /// `rand(100) < (LEAD + morale) / 3` (§6).
    fn count_down_statuses(&mut self, pack: &Pack, side: Side, ev: &mut Vec<BattleEvent>) {
        let low = pack.rules.confuse_morale;
        let original = super::strategy::original_formulas(pack);
        for id in 0..self.units.len() {
            let u = &self.units[id];
            if !u.is_active() || u.side != side {
                continue;
            }
            if original {
                if self.recovery_roll(id) {
                    ev.push(BattleEvent::StatusExpired {
                        unit: id,
                        status: StatusKind::Confused,
                    });
                }
                continue;
            }
            let u = &mut self.units[id];
            let keep = u.morale <= low;
            let mut expired = Vec::new();
            u.statuses.retain_mut(|s| {
                s.turns = s.turns.saturating_sub(1);
                if s.turns > 0 {
                    return true;
                }
                if keep {
                    s.turns = 1;
                    return true;
                }
                expired.push(s.status);
                false
            });
            for status in expired {
                ev.push(BattleEvent::StatusExpired { unit: id, status });
            }
        }
    }

    /// Low-morale confusion (§6): chance `(confuse_morale - morale) * 3 + 10` percent. Not
    /// under the original strategy formulas: the original confuses when morale falls
    /// ([`BattleState::morale_set`]), not at a phase start.
    fn low_morale_confusion(&mut self, pack: &Pack, side: Side, ev: &mut Vec<BattleEvent>) {
        if super::strategy::original_formulas(pack) {
            return;
        }
        let low = pack.rules.confuse_morale;
        for id in 0..self.units.len() {
            let u = &self.units[id];
            if !u.is_active()
                || u.side != side
                || u.morale > low
                || u.has_status(StatusKind::Confused)
            {
                continue;
            }
            let chance = (low - u.morale).saturating_mul(3).saturating_add(10);
            if self.rng.chance(chance) {
                self.confuse(id, 1);
                ev.push(BattleEvent::Confused { unit: id });
                self.retreat_if_beaten(id, ev);
            }
        }
    }

    /// After an action: lord check, events, victory/defeat.
    pub(super) fn settle(&mut self, pack: &Pack, ev: &mut Vec<BattleEvent>) {
        self.settle_with(pack, None, false, ev);
    }

    fn settle_with(
        &mut self,
        pack: &Pack,
        phase: Option<PhaseKey>,
        only_turn_start: bool,
        ev: &mut Vec<BattleEvent>,
    ) {
        if self.outcome.is_some() {
            return;
        }
        if self.lord_retreated() {
            self.lose(DefeatReason::LordRetreated, ev);
            return;
        }
        // (A troop fought without the lord is lost only after the events had their turn:
        // one may bring reinforcements or end the battle.)
        self.fire_events(pack, phase, only_turn_start, ev);
        self.check_outcome(pack, self.turn.saturating_sub(1), ev);
    }

    /// Fire every event whose trigger holds (§9). Passes repeat while events keep firing
    /// (their actions may make other triggers true, e.g. spawns); an event fires at most once
    /// per check, and never again after it fired when `once`.
    fn fire_events(
        &mut self,
        pack: &Pack,
        phase: Option<PhaseKey>,
        only_turn_start: bool,
        ev: &mut Vec<BattleEvent>,
    ) {
        let events = &self.def(pack).events;
        if self.fired.len() < events.len() {
            self.fired.resize(events.len(), false);
        }
        let mut this_check = vec![false; events.len()];
        loop {
            let mut any = false;
            for (i, e) in events.iter().enumerate() {
                if self.outcome.is_some() {
                    return;
                }
                if this_check[i]
                    || (e.once && self.fired[i])
                    || e.stage.is_some_and(|s| s != self.stage)
                    || !self.flags_allow(e)
                {
                    continue;
                }
                if only_turn_start && !matches!(e.trigger, Trigger::TurnStart { .. }) {
                    continue;
                }
                if !self.trigger_holds(&e.trigger, phase) {
                    continue;
                }
                this_check[i] = true;
                self.fired[i] = true;
                any = true;
                for a in &e.actions {
                    self.run_action(pack, a, ev);
                }
            }
            if !any {
                return;
            }
        }
    }

    /// Ids of every unit a reference (tag or officer id) names.
    fn matching<'s>(&'s self, reference: &'s str) -> impl Iterator<Item = UnitId> + 's {
        self.units
            .iter()
            .filter(move |u| u.matches(reference))
            .map(|u| u.id)
    }

    /// At least one unit matches and all matching units have retreated.
    fn all_retreated(&self, reference: &str) -> bool {
        let mut any = false;
        for id in self.matching(reference) {
            if self.units[id].state != UnitState::Retreated {
                return false;
            }
            any = true;
        }
        any
    }

    /// An active unit named by `who` (any player unit when `None`) within manhattan `radius` of `pos`.
    fn someone_near(&self, who: Option<&str>, pos: Pos, radius: i32, to: Option<Pos>) -> bool {
        self.units.iter().any(|u| {
            u.is_active()
                && who.map_or(u.side == Side::Player, |w| u.matches(w))
                && in_reach(pos, radius, to, u.pos)
        })
    }

    fn trigger_holds(&self, trigger: &Trigger, phase: Option<PhaseKey>) -> bool {
        match trigger {
            Trigger::TurnStart { turn, side } => phase == Some((*turn, *side)),
            Trigger::UnitDefeated { target } => self.all_retreated(target),
            Trigger::Reach {
                who,
                pos,
                radius,
                to,
            } => self.someone_near(who.as_deref(), *pos, *radius, *to),
            Trigger::Adjacent { a, b } => self.units.iter().any(|ux| {
                ux.is_active()
                    && a.as_deref()
                        .map_or(ux.side == Side::Player, |a| ux.matches(a))
                    && self.matching(b).any(|y| {
                        self.units[y].is_active() && ux.pos.manhattan(self.units[y].pos) == 1
                    })
            }),
            Trigger::HpBelow { target, pct } => self.matching(target).any(|id| {
                let u = &self.units[id];
                u.is_active() && (u.hp as i64) * 100 < (*pct as i64) * u.max_hp as i64
            }),
        }
    }

    fn run_action(&mut self, pack: &Pack, action: &EventAction, ev: &mut Vec<BattleEvent>) {
        match action {
            EventAction::Drama { scene } => ev.push(BattleEvent::Drama {
                scene: scene.clone(),
            }),
            EventAction::Spawn { group } => self.spawn_group(pack, group, ev),
            EventAction::SetAi {
                target,
                ai,
                ai_target,
                ai_pos,
            } => {
                let ids: Vec<UnitId> = self.matching(target).collect();
                for id in ids {
                    let u = &mut self.units[id];
                    u.ai = *ai;
                    u.ai_target = ai_target.clone();
                    u.ai_pos = match (ai_pos, ai) {
                        (Some(p), _) => Some(*p),
                        (None, AiMode::Guard) => Some(u.pos),
                        (None, _) => None,
                    };
                }
            }
            EventAction::Retreat { target } => {
                let ids: Vec<UnitId> = self.matching(target).collect();
                for id in ids {
                    // A unit still waiting in its reinforcement group is taken out too: once
                    // an event has removed it (a duel's loser who was to come later), a `spawn`
                    // must not bring it in. It was never on the map, so nothing is shown.
                    if self.units[id].state == UnitState::Hidden {
                        self.units[id].state = UnitState::Retreated;
                    } else {
                        self.retreat(id, false, ev);
                    }
                }
            }
            EventAction::LevelUp { target, amount } => {
                let ids: Vec<UnitId> = self
                    .matching(target)
                    .filter(|&id| self.units[id].state != UnitState::Retreated)
                    .collect();
                for id in ids {
                    self.gain_levels(pack, id, *amount, ev);
                }
            }
            EventAction::GiveItem { item } => self.items_found.push(item.clone()),
            EventAction::GiveGold { amount } => {
                self.gold_found = self.gold_found.saturating_add(*amount)
            }
            EventAction::SetFlag { flag, value } => {
                self.flags.insert(flag.clone(), *value);
            }
            EventAction::SetStage { stage } => self.stage = *stage,
            EventAction::SetObjective { text } => {
                self.objective = Some(text.clone());
                ev.push(BattleEvent::ObjectiveChanged { text: text.clone() });
            }
            EventAction::SetTerrain {
                pos,
                terrain,
                image,
            } => {
                if pack.terrain(terrain).is_some() && self.map.set_terrain(*pos, terrain) {
                    self.map_images.retain(|m| m.pos != *pos);
                    if let Some(image) = image {
                        self.map_images.push(MapImage {
                            pos: *pos,
                            image: image.clone(),
                        });
                    }
                    ev.push(BattleEvent::TerrainChanged { pos: *pos });
                }
            }
            EventAction::Victory => {
                if self.outcome.is_none() {
                    self.update_bonus(pack, self.turn.saturating_sub(1), ev);
                    self.win(pack, ev);
                }
            }
            EventAction::Defeat => {
                if self.outcome.is_none() {
                    self.lose(DefeatReason::Event, ev);
                }
            }
        }
    }

    /// Place every hidden unit of `group` on its tile or the nearest free passable one.
    fn spawn_group(&mut self, pack: &Pack, group: &str, ev: &mut Vec<BattleEvent>) {
        let waiting: Vec<UnitId> = self
            .units
            .iter()
            .filter(|u| u.state == UnitState::Hidden && u.group.as_deref() == Some(group))
            .map(|u| u.id)
            .collect();
        let mut placed = Vec::new();
        for id in waiting {
            // Only a full map leaves no tile; the unit then stays hidden.
            if let Some(pos) = self.free_tile_near(pack, id, self.units[id].pos) {
                let u = &mut self.units[id];
                u.pos = pos;
                u.state = UnitState::Active;
                u.moved = false;
                u.acted = false;
                placed.push(id);
            }
        }
        if !placed.is_empty() {
            ev.push(BattleEvent::Spawned { units: placed });
        }
    }

    /// Nearest tile to `want` (manhattan distance, then row-major) that is inside the map,
    /// passable for the unit and not occupied by an active unit.
    fn free_tile_near(&self, pack: &Pack, id: UnitId, want: Pos) -> Option<Pos> {
        let board = Board::new(self, pack);
        let move_type = &self.class_of(pack, id).move_type;
        self.map
            .positions()
            .filter(|&p| board.unit_at(p).is_none())
            .filter(|&p| {
                board
                    .terrain(p)
                    .and_then(|t| t.move_cost(move_type))
                    .is_some()
            })
            .min_by_key(|&p| (p.manhattan(want), p.y, p.x))
    }

    fn lord_retreated(&self) -> bool {
        self.units
            .iter()
            .any(|u| u.lord && u.state == UnitState::Retreated)
    }

    /// A battle fought without the lord (another troop's) is lost when all of its player units
    /// have retreated and none is still to arrive (a hidden reinforcement).
    fn army_retreated(&self) -> bool {
        let player = || self.units.iter().filter(|u| u.side == Side::Player);
        !player().any(|u| u.lord)
            && player().any(|u| u.state == UnitState::Retreated)
            && player().all(|u| u.state == UnitState::Retreated)
    }

    fn condition_holds(&self, cond: &Condition, completed_turns: u32) -> bool {
        match cond {
            Condition::DefeatAll => !self.has_active(Side::Enemy),
            Condition::DefeatUnit { target } | Condition::UnitRetreated { target } => {
                self.all_retreated(target)
            }
            Condition::DefeatCommander => self
                .units
                .iter()
                .any(|u| u.side == Side::Enemy && u.commander && u.state == UnitState::Retreated),
            Condition::Reach {
                who,
                pos,
                radius,
                to,
            } => self.someone_near(who.as_deref(), *pos, *radius, *to),
            Condition::SurviveTurns { turns } => completed_turns >= *turns,
        }
    }

    fn update_bonus(&mut self, pack: &Pack, completed_turns: u32, ev: &mut Vec<BattleEvent>) {
        if self.bonus_done {
            return;
        }
        if let Some(bonus) = &self.def(pack).bonus {
            if self.condition_holds(&bonus.condition, completed_turns) {
                self.bonus_done = true;
                ev.push(BattleEvent::BonusAchieved { exp: bonus.exp });
            }
        }
    }

    /// Victory / defeat check (§9): the lord retreating always loses; the bonus objective is
    /// updated first; victory is checked before defeat.
    pub(super) fn check_outcome(
        &mut self,
        pack: &Pack,
        completed_turns: u32,
        ev: &mut Vec<BattleEvent>,
    ) {
        if self.outcome.is_some() {
            return;
        }
        if self.lord_retreated() {
            self.lose(DefeatReason::LordRetreated, ev);
            return;
        }
        self.update_bonus(pack, completed_turns, ev);
        let def = self.def(pack);
        if def
            .victory
            .iter()
            .any(|c| self.condition_holds(c, completed_turns))
        {
            self.win(pack, ev);
        } else if self.army_retreated() {
            self.lose(DefeatReason::ArmyRetreated, ev);
        } else if def
            .defeat
            .iter()
            .any(|c| self.condition_holds(c, completed_turns))
        {
            self.lose(DefeatReason::Condition, ev);
        }
    }

    /// Victory: reward gold, bonus EXP for surviving player units, `Victory`, then the outro.
    fn win(&mut self, pack: &Pack, ev: &mut Vec<BattleEvent>) {
        let def = self.def(pack);
        self.outcome = Some(Outcome::Victory);
        self.gold_found = self.gold_found.saturating_add(def.reward_gold);
        if let (true, Some(bonus)) = (self.bonus_done, &def.bonus) {
            let survivors: Vec<UnitId> = self
                .units
                .iter()
                .filter(|u| u.is_active() && u.side == Side::Player)
                .map(|u| u.id)
                .collect();
            for id in survivors {
                self.gain_exp(pack, id, bonus.exp, ev);
            }
        }
        ev.push(BattleEvent::Victory);
        if let Some(scene) = &def.outro {
            ev.push(BattleEvent::Drama {
                scene: scene.clone(),
            });
        }
    }

    fn lose(&mut self, reason: DefeatReason, ev: &mut Vec<BattleEvent>) {
        self.outcome = Some(Outcome::Defeat(reason));
        ev.push(BattleEvent::Defeat(reason));
    }
}
