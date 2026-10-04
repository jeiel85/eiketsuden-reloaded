//! Campaign flow (`campaign.toml`) and the persistent army state carried between battles.

use crate::battle::{BattleState, Outcome};
use crate::battledef::Side;
use crate::data::{Effect, Equipment, Id, ItemDef, ItemKind, OfficerDef};
use crate::pack::Pack;
use crate::script::{cmp_field, Compare};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Abilities (무력, 지력, 통솔) the forbidden secret gives the lord
/// ([`CampaignState::forbidden_secret`]): the original's value (`MAIN.EXE`, docs/RULES.md).
pub const FORBIDDEN_SECRET_ABILITY: i32 = 100;
/// Gold the forbidden secret gives: the original's 10000.
pub const FORBIDDEN_SECRET_GOLD: i64 = 10_000;

/// Difficulty chosen for a new game (DECISIONS D25). [`Difficulty::Normal`] is the pack as it
/// is: the original mode's numbers stay the original's. The others only move enemy levels
/// ([`Difficulty::enemy_level_offset`]); everything else follows from the level by the usual
/// formulas (RULES.md §7.6).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Difficulty {
    Easy,
    #[default]
    Normal,
    Hard,
}

impl Difficulty {
    pub const ALL: [Difficulty; 3] = [Difficulty::Easy, Difficulty::Normal, Difficulty::Hard];

    /// Levels added to every enemy unit when a battle is set up (clamped to `1..=level_cap`).
    pub fn enemy_level_offset(self) -> i32 {
        match self {
            Difficulty::Easy => -2,
            Difficulty::Normal => 0,
            Difficulty::Hard => 2,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Difficulty::Easy => "쉬움",
            Difficulty::Normal => "기본",
            Difficulty::Hard => "어려움",
        }
    }
}

/// The choices of a new game (D25); the default is the pack as it is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GameOptions {
    pub difficulty: Difficulty,
    pub free_edit: bool,
    pub extended_rules: bool,
}

/// Highest 무력, 지력 or 통솔 that free editing sets (the forbidden secret's value).
pub const ABILITY_MAX: i32 = FORBIDDEN_SECRET_ABILITY;

/// One of an officer's three abilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ability {
    /// 무력
    Strength,
    /// 지력
    Int,
    /// 통솔
    Lead,
}

impl Ability {
    pub const ALL: [Ability; 3] = [Ability::Strength, Ability::Int, Ability::Lead];
}

/// One step of the campaign. Nodes are visited in order of their `next` links.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Node {
    /// Play a drama scene (global scene id), then go to `next`.
    Drama { id: Id, scene: String, next: Id },
    /// Preparation screen before a battle: shop, equipment, deployment, saving.
    Camp {
        id: Id,
        /// Heading shown on the camp screen, e.g. `탁현 — 출진 준비`.
        #[serde(default)]
        title: String,
        /// Items the shop sells here.
        #[serde(default)]
        shop: Vec<Id>,
        /// Battle whose deployment this camp prepares (enables the deploy screen).
        #[serde(default)]
        battle: Option<Id>,
        next: Id,
    },
    /// Fight a battle. Victory goes to `next`; defeat goes to `on_defeat` or game over.
    Battle {
        id: Id,
        battle: Id,
        next: Id,
        #[serde(default)]
        on_defeat: Option<Id>,
    },
    /// Jump on a campaign flag: `flag <cmp> value` ? then : else.
    Branch {
        id: Id,
        flag: String,
        #[serde(
            default = "cmp_field::default",
            deserialize_with = "cmp_field::deserialize"
        )]
        cmp: Compare,
        #[serde(default)]
        value: i64,
        then: Id,
        #[serde(rename = "else")]
        otherwise: Id,
    },
    /// The end of the campaign (optionally after a final scene).
    Ending {
        id: Id,
        #[serde(default)]
        scene: Option<String>,
        /// Ending title shown on the credits screen.
        #[serde(default)]
        title: String,
    },
}

impl Node {
    pub fn id(&self) -> &str {
        match self {
            Node::Drama { id, .. }
            | Node::Camp { id, .. }
            | Node::Battle { id, .. }
            | Node::Branch { id, .. }
            | Node::Ending { id, .. } => id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CampaignDef {
    pub title: String,
    /// First node of a new game.
    pub start: Id,
    /// Officers in the army at the start of a new game.
    pub starting_officers: Vec<Id>,
    #[serde(default)]
    pub starting_gold: i64,
    /// Starting inventory: item id -> count.
    #[serde(default)]
    pub starting_items: BTreeMap<Id, u32>,
    #[serde(rename = "node")]
    pub nodes: Vec<Node>,
}

impl CampaignDef {
    pub fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id() == id)
    }
}

/// Persistent per-officer progress for officers in the player's army.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfficerState {
    pub id: Id,
    pub class: Id,
    pub level: u32,
    pub exp: u32,
    #[serde(rename = "str")]
    pub strength: i32,
    pub int: i32,
    pub lead: i32,
    pub equip: Equipment,
    /// Away from the army for now (`@away`): kept with all their progress but not deployed,
    /// until `@join` brings them back.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub away: bool,
}

/// What the story gave an officer who was not in the army (`@level`, `@class`), for when they
/// join ([`CampaignState::pending_growth`]).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Growth {
    /// Levels gained so far.
    #[serde(default)]
    pub levels: u32,
    /// The class they were given last.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<Id>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CampaignError {
    #[error("unknown officer `{0}`")]
    UnknownOfficer(Id),
    #[error("officer `{0}` is not in the army")]
    NotInArmy(Id),
    #[error("unknown item `{0}`")]
    UnknownItem(Id),
    #[error("unknown class `{0}`")]
    UnknownClass(Id),
    #[error("item `{0}` is not in the inventory")]
    NotOwned(Id),
    #[error("not enough gold: need {need}, have {have}")]
    NotEnoughGold { need: i64, have: i64 },
    #[error("item `{item}` cannot be equipped by class family `{family}`")]
    CannotEquip { item: Id, family: Id },
    #[error("item `{0}` is not equipment")]
    NotEquipment(Id),
    #[error("item `{0}` cannot be sold")]
    CannotSell(Id),
    #[error("item `{item}` cannot be used on `{officer}`: {reason}")]
    CannotUse {
        item: Id,
        officer: Id,
        reason: String,
    },
    #[error("unknown campaign node `{0}`")]
    UnknownNode(Id),
    /// The item has no price (treasure / event item).
    #[error("item `{0}` is not for sale")]
    CannotBuy(Id),
    /// Branch nodes starting at this node lead back to themselves for the current flags.
    #[error("campaign branches starting at `{0}` loop forever")]
    BranchLoop(Id),
}

impl OfficerState {
    /// Initial state of an officer joining the army (level, class, stats and equipment
    /// from `officers.toml`, no EXP).
    fn from_def(def: &OfficerDef) -> OfficerState {
        OfficerState {
            id: def.id.clone(),
            class: def.class.clone(),
            level: def.level,
            exp: 0,
            strength: def.strength,
            int: def.int,
            lead: def.lead,
            equip: def.equip.clone(),
            away: false,
        }
    }
}

/// The equipment slot an item of `kind` goes into (`None` for consumables).
fn slot_mut(equip: &mut Equipment, kind: ItemKind) -> Option<&mut Option<Id>> {
    match kind {
        ItemKind::Weapon => Some(&mut equip.weapon),
        ItemKind::Armor => Some(&mut equip.armor),
        ItemKind::Accessory => Some(&mut equip.accessory),
        ItemKind::Consumable => None,
    }
}

/// Whether a class family may equip `item` (an empty `families` list allows everyone).
fn family_allows(item: &ItemDef, family: &str) -> bool {
    item.families.is_empty() || item.families.iter().any(|f| f == family)
}

/// Everything that persists between battles; stored in save games.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CampaignState {
    /// Current campaign node id.
    pub node: Id,
    /// Officers in the player's army, in roster order.
    pub roster: Vec<OfficerState>,
    /// Unequipped items: item id -> count.
    pub inventory: BTreeMap<Id, u32>,
    pub gold: i64,
    pub flags: BTreeMap<String, i64>,
    /// Officers chosen on the deploy screen. The list is kept between battles as the player's
    /// last choice; each battle normalises it to its own rules
    /// ([`crate::battle::normalize_deployment`]).
    #[serde(default)]
    pub deployed: Vec<Id>,
    /// Battle ids won so far.
    #[serde(default)]
    pub battles_won: Vec<Id>,
    /// Total play time in seconds (maintained by the frontend).
    #[serde(default)]
    pub play_seconds: u64,
    /// Levels and classes the story gave officers not in the army, applied when they join (the
    /// original raises the officers of other armies, who come over later as they are then).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub pending_growth: BTreeMap<Id, Growth>,
    /// Chosen when the game began; saves from before D25 have none and play as `normal`.
    #[serde(default)]
    pub difficulty: Difficulty,
    /// 능력치 자유 조정, chosen when the game began (D25): the camp may set the army's 무력,
    /// 지력 and 통솔 ([`CampaignState::set_ability`]).
    #[serde(default)]
    pub free_edit: bool,
    /// 확장 규칙, chosen when the game began (D25): battles use the joint attack bonus
    /// (RULES.md §4).
    #[serde(default)]
    pub extended_rules: bool,
}

impl CampaignState {
    /// Fresh state for a new game: starting officers (at their `officers.toml` level/class/equipment),
    /// starting gold and items, positioned at `campaign.start`. Unknown officer and item ids are
    /// skipped; [`Pack::validate`] reports them as errors.
    pub fn new_game(pack: &Pack) -> CampaignState {
        let campaign = &pack.campaign;
        let mut state = CampaignState {
            node: campaign.start.clone(),
            roster: Vec::new(),
            inventory: BTreeMap::new(),
            gold: 0,
            flags: BTreeMap::new(),
            deployed: Vec::new(),
            battles_won: Vec::new(),
            play_seconds: 0,
            pending_growth: BTreeMap::new(),
            difficulty: Difficulty::Normal,
            free_edit: false,
            extended_rules: false,
        };
        for def in campaign
            .starting_officers
            .iter()
            .filter_map(|id| pack.officer(id))
        {
            if state.officer(&def.id).is_none() {
                state.roster.push(OfficerState::from_def(def));
            }
        }
        for (item, count) in &campaign.starting_items {
            if pack.item(item).is_some() {
                state.add_item(item, *count);
            }
        }
        state.add_gold(pack, campaign.starting_gold);
        state
    }

    /// Take the new game's choices.
    pub fn apply_options(&mut self, options: GameOptions) {
        self.difficulty = options.difficulty;
        self.free_edit = options.free_edit;
        self.extended_rules = options.extended_rules;
    }

    /// Whether a choice of the new game moves this campaign away from the pack as it is
    /// (D25): such saves say so and need a game that knows the choices.
    pub fn off_original(&self) -> bool {
        self.difficulty != Difficulty::Normal || self.free_edit || self.extended_rules
    }

    /// Short names of the new game's choices that are on, for save labels: `어려움`, `조정`,
    /// `확장`.
    pub fn option_tags(&self) -> Vec<&'static str> {
        let mut tags = Vec::new();
        if self.difficulty != Difficulty::Normal {
            tags.push(self.difficulty.label());
        }
        if self.free_edit {
            tags.push("조정");
        }
        if self.extended_rules {
            tags.push("확장");
        }
        tags
    }

    /// 능력치 자유 조정: set one of an army officer's abilities, kept in `1..=ABILITY_MAX`.
    /// Returns the value set; `None` when the campaign was not started with free editing or
    /// the officer is not in the army.
    pub fn set_ability(&mut self, officer: &str, ability: Ability, value: i32) -> Option<i32> {
        if !self.free_edit {
            return None;
        }
        let o = self.officer_mut(officer)?;
        let v = value.clamp(1, ABILITY_MAX);
        match ability {
            Ability::Strength => o.strength = v,
            Ability::Int => o.int = v,
            Ability::Lead => o.lead = v,
        }
        Some(v)
    }

    pub fn officer(&self, id: &str) -> Option<&OfficerState> {
        self.roster.iter().find(|o| o.id == id)
    }

    pub fn officer_mut(&mut self, id: &str) -> Option<&mut OfficerState> {
        self.roster.iter_mut().find(|o| o.id == id)
    }

    pub fn flag(&self, name: &str) -> i64 {
        self.flags.get(name).copied().unwrap_or(0)
    }

    /// Number of unequipped copies of `item`.
    pub fn item_count(&self, item: &str) -> u32 {
        self.inventory.get(item).copied().unwrap_or(0)
    }

    /// Add an officer to the army: one not in the roster starts from their `officers.toml`
    /// definition with what the story gave them meanwhile ([`CampaignState::pending_growth`]),
    /// one who is away comes back as they left (no-op for one already present).
    pub fn join(&mut self, pack: &Pack, officer: &str) -> Result<(), CampaignError> {
        if let Some(o) = self.roster.iter_mut().find(|o| o.id == officer) {
            o.away = false;
            return Ok(());
        }
        let def = pack
            .officer(officer)
            .ok_or_else(|| CampaignError::UnknownOfficer(officer.to_string()))?;
        self.roster.push(OfficerState::from_def(def));
        if let Some(growth) = self.pending_growth.remove(officer) {
            if let Some(class) = growth.class.filter(|c| pack.class(c).is_some()) {
                self.change_class(pack, officer, class);
            }
            if growth.levels > 0 {
                // (The officer was just added.)
                let _ = self.add_levels(pack, officer, growth.levels);
            }
        }
        Ok(())
    }

    /// Send an officer of the army away for now (see [`OfficerState::away`]).
    pub fn set_away(&mut self, officer: &str) -> Result<(), CampaignError> {
        let o = self
            .roster
            .iter_mut()
            .find(|o| o.id == officer)
            .ok_or_else(|| CampaignError::NotInArmy(officer.to_string()))?;
        o.away = true;
        self.deployed.retain(|d| d != officer);
        Ok(())
    }

    /// Remove an officer from the army, returning their equipment to the inventory.
    pub fn leave(&mut self, officer: &str) -> Result<(), CampaignError> {
        let index = self
            .roster
            .iter()
            .position(|o| o.id == officer)
            .ok_or_else(|| CampaignError::NotInArmy(officer.to_string()))?;
        let state = self.roster.remove(index);
        for item in state.equip.iter() {
            self.add_item(item, 1);
        }
        self.deployed.retain(|d| d != officer);
        Ok(())
    }

    pub fn add_item(&mut self, item: &str, count: u32) {
        if count > 0 {
            let n = self.inventory.entry(item.to_string()).or_insert(0);
            *n = n.saturating_add(count);
        }
    }

    /// Take one copy of `item` out of the inventory.
    pub fn remove_item(&mut self, item: &str) -> Result<(), CampaignError> {
        match self.inventory.get_mut(item) {
            Some(n) if *n > 0 => {
                *n -= 1;
                if *n == 0 {
                    self.inventory.remove(item);
                }
                Ok(())
            }
            _ => Err(CampaignError::NotOwned(item.to_string())),
        }
    }

    /// Change gold, clamped to `0..=rules.gold_cap`.
    pub fn add_gold(&mut self, pack: &Pack, amount: i64) {
        let cap = pack.rules.gold_cap.max(0);
        self.gold = self.gold.saturating_add(amount).clamp(0, cap);
    }

    /// The original's "forbidden secret" (a hidden command of the PC game): the lord reaches the
    /// level cap with 100 in every ability and no spare EXP, and the army gets
    /// [`FORBIDDEN_SECRET_GOLD`] gold. Returns the lord's id, or `None` when the army has no lord.
    pub fn forbidden_secret(&mut self, pack: &Pack) -> Option<Id> {
        let lord = self
            .roster
            .iter()
            .find(|o| pack.officer(&o.id).is_some_and(|d| d.lord))?
            .id
            .clone();
        let cap = pack.rules.level_cap;
        let o = self.officer_mut(&lord)?;
        o.level = o.level.max(cap);
        o.exp = 0;
        o.strength = FORBIDDEN_SECRET_ABILITY;
        o.int = FORBIDDEN_SECRET_ABILITY;
        o.lead = FORBIDDEN_SECRET_ABILITY;
        self.add_gold(pack, FORBIDDEN_SECRET_GOLD);
        Some(lord)
    }

    /// Buy one copy of `item` for its price (items with price 0 are not for sale). Which
    /// items a camp offers is up to the frontend (`Node::Camp::shop`).
    pub fn buy(&mut self, pack: &Pack, item: &str) -> Result<(), CampaignError> {
        let def = pack
            .item(item)
            .ok_or_else(|| CampaignError::UnknownItem(item.to_string()))?;
        if def.price == 0 {
            return Err(CampaignError::CannotBuy(item.to_string()));
        }
        let price = i64::from(def.price);
        if self.gold < price {
            return Err(CampaignError::NotEnoughGold {
                need: price,
                have: self.gold,
            });
        }
        self.gold -= price;
        self.add_item(item, 1);
        Ok(())
    }

    /// Sell for half the price (items with price 0 cannot be sold).
    pub fn sell(&mut self, pack: &Pack, item: &str) -> Result<(), CampaignError> {
        let def = pack
            .item(item)
            .ok_or_else(|| CampaignError::UnknownItem(item.to_string()))?;
        if def.price == 0 {
            return Err(CampaignError::CannotSell(item.to_string()));
        }
        self.remove_item(item)?;
        self.add_gold(pack, i64::from(def.price / 2));
        Ok(())
    }

    /// Equip an inventory item on an officer; the previously equipped item of that slot
    /// goes back to the inventory. Checks `ItemDef::families` against the officer's class family.
    pub fn equip(&mut self, pack: &Pack, officer: &str, item: &str) -> Result<(), CampaignError> {
        let state = self
            .officer(officer)
            .ok_or_else(|| CampaignError::NotInArmy(officer.to_string()))?;
        let def = pack
            .item(item)
            .ok_or_else(|| CampaignError::UnknownItem(item.to_string()))?;
        if def.kind == ItemKind::Consumable {
            return Err(CampaignError::NotEquipment(item.to_string()));
        }
        if self.item_count(item) == 0 {
            return Err(CampaignError::NotOwned(item.to_string()));
        }
        let family = pack.class(&state.class).map_or("", |c| c.family.as_str());
        if !family_allows(def, family) {
            return Err(CampaignError::CannotEquip {
                item: item.to_string(),
                family: family.to_string(),
            });
        }
        self.remove_item(item)?;
        let previous = self
            .officer_mut(officer)
            .and_then(|o| slot_mut(&mut o.equip, def.kind))
            .and_then(|slot| slot.replace(item.to_string()));
        if let Some(previous) = previous {
            self.add_item(&previous, 1);
        }
        Ok(())
    }

    /// Put the item of `slot` back into the inventory. Unequipping an empty slot (or
    /// `Consumable`, which is not a slot) changes nothing.
    pub fn unequip(&mut self, officer: &str, slot: ItemKind) -> Result<(), CampaignError> {
        let state = self
            .officer_mut(officer)
            .ok_or_else(|| CampaignError::NotInArmy(officer.to_string()))?;
        if let Some(item) = slot_mut(&mut state.equip, slot).and_then(Option::take) {
            self.add_item(&item, 1);
        }
        Ok(())
    }

    /// Use a camp consumable (class-up `Promote` / `ChangeClass` items) on an officer.
    /// Checks the promotion level and item, `OfficerDef::fixed_class`, then consumes the item.
    /// Level, EXP and stats are kept; equipment the new class family may not use goes back to
    /// the inventory.
    pub fn use_item(
        &mut self,
        pack: &Pack,
        officer: &str,
        item: &str,
    ) -> Result<(), CampaignError> {
        let state = self
            .officer(officer)
            .ok_or_else(|| CampaignError::NotInArmy(officer.to_string()))?;
        let def = pack
            .item(item)
            .ok_or_else(|| CampaignError::UnknownItem(item.to_string()))?;
        if self.item_count(item) == 0 {
            return Err(CampaignError::NotOwned(item.to_string()));
        }
        let cannot = |reason: String| CampaignError::CannotUse {
            item: item.to_string(),
            officer: officer.to_string(),
            reason,
        };
        let effect = def
            .effects
            .iter()
            .find(|e| matches!(e, Effect::Promote | Effect::ChangeClass { .. }));
        let new_class = match effect {
            Some(Effect::Promote) => {
                let class = pack
                    .class(&state.class)
                    .ok_or_else(|| cannot(format!("unknown class `{}`", state.class)))?;
                let promotion = class
                    .promote
                    .as_ref()
                    .ok_or_else(|| cannot(format!("class `{}` has no promotion", class.id)))?;
                if promotion.item != item {
                    return Err(cannot(format!(
                        "class `{}` is promoted with `{}`",
                        class.id, promotion.item
                    )));
                }
                if state.level < promotion.level {
                    return Err(cannot(format!(
                        "promotion needs level {} (the officer is level {})",
                        promotion.level, state.level
                    )));
                }
                promotion.to.clone()
            }
            Some(Effect::ChangeClass { to }) => {
                if pack.officer(officer).is_some_and(|o| o.fixed_class) {
                    return Err(cannot("the officer cannot change class".into()));
                }
                if state.class == *to {
                    return Err(cannot(format!("the officer already is `{to}`")));
                }
                to.clone()
            }
            _ => return Err(cannot("the item has no effect outside battle".into())),
        };
        if pack.class(&new_class).is_none() {
            return Err(cannot(format!("unknown class `{new_class}`")));
        }
        self.remove_item(item)?;
        self.change_class(pack, officer, new_class);
        Ok(())
    }

    /// `officer` becomes `class`: equipment the new class family may not use goes back to the
    /// inventory. The class must exist.
    fn change_class(&mut self, pack: &Pack, officer: &str, class: Id) {
        debug_assert!(pack.class(&class).is_some(), "unknown class {class}");
        let family = pack
            .class(&class)
            .map_or_else(String::new, |c| c.family.clone());
        let mut returned = Vec::new();
        if let Some(state) = self.officer_mut(officer) {
            state.class = class;
            let slots = [
                &mut state.equip.weapon,
                &mut state.equip.armor,
                &mut state.equip.accessory,
            ];
            for slot in slots {
                let allowed = slot
                    .as_deref()
                    .and_then(|id| pack.item(id))
                    .is_none_or(|equipped| family_allows(equipped, &family));
                if !allowed {
                    returned.extend(slot.take());
                }
            }
        }
        for id in returned {
            self.add_item(&id, 1);
        }
    }

    /// `@class`: `officer` of the army becomes `class` (the story's change; no item, no
    /// promotion level), as [`CampaignState::use_item`] changes it. One of the pack who is not
    /// in the army has it kept for when they join ([`CampaignState::pending_growth`]).
    pub fn set_class(
        &mut self,
        pack: &Pack,
        officer: &str,
        class: &str,
    ) -> Result<(), CampaignError> {
        if pack.class(class).is_none() {
            return Err(CampaignError::UnknownClass(class.to_string()));
        }
        if self.officer(officer).is_some() {
            self.change_class(pack, officer, class.to_string());
        } else if pack.officer(officer).is_some() {
            self.pending_growth
                .entry(officer.to_string())
                .or_default()
                .class = Some(class.to_string());
        } else {
            return Err(CampaignError::NotInArmy(officer.to_string()));
        }
        Ok(())
    }

    /// `@level`: `officer` of the army gains `levels`, up to the level cap (one already above
    /// it keeps their level). Only the level changes: HP, MP and the strategies known follow
    /// from it in battle. One of the pack who is not in the army has the levels kept for when
    /// they join ([`CampaignState::pending_growth`]).
    pub fn add_levels(
        &mut self,
        pack: &Pack,
        officer: &str,
        levels: u32,
    ) -> Result<(), CampaignError> {
        let cap = pack.rules.level_cap;
        if let Some(state) = self.officer_mut(officer) {
            state.level = state.level.saturating_add(levels).min(cap).max(state.level);
        } else if pack.officer(officer).is_some() {
            // Capped: joining adds them up to the cap anyway, and the save stays small.
            let growth = self.pending_growth.entry(officer.to_string()).or_default();
            growth.levels = growth.levels.saturating_add(levels).min(cap);
        } else {
            return Err(CampaignError::NotInArmy(officer.to_string()));
        }
        Ok(())
    }

    /// Resolve the node after the current one. Branch nodes are evaluated immediately
    /// (following chains of branches); returns the new current node id. On an `Ending` node
    /// nothing changes; when the current node is itself a branch, it is resolved.
    /// Victory continues to a battle's `next`; after a defeat, [`CampaignState::jump`] to the
    /// battle's `on_defeat` node instead.
    pub fn advance(&mut self, pack: &Pack) -> Result<Id, CampaignError> {
        let node = pack
            .campaign
            .node(&self.node)
            .ok_or_else(|| CampaignError::UnknownNode(self.node.clone()))?;
        let next = match node {
            Node::Drama { next, .. } | Node::Camp { next, .. } | Node::Battle { next, .. } => {
                next.as_str()
            }
            Node::Branch { id, .. } => id.as_str(),
            Node::Ending { .. } => return Ok(self.node.clone()),
        };
        let target = self.resolve(pack, next)?;
        self.node = target.clone();
        Ok(target)
    }

    /// Make `node` the current node, resolving branch chains; returns the node that became
    /// current. Used for a battle's `on_defeat` link. The state is unchanged on error.
    pub fn jump(&mut self, pack: &Pack, node: &str) -> Result<Id, CampaignError> {
        let target = self.resolve(pack, node)?;
        self.node = target.clone();
        Ok(target)
    }

    /// Follow branch nodes from `start` to the first non-branch node.
    fn resolve(&self, pack: &Pack, start: &str) -> Result<Id, CampaignError> {
        let nodes = &pack.campaign;
        let mut id = start;
        // A chain of distinct branches is at most as long as the node list.
        for _ in 0..=nodes.nodes.len() {
            match nodes.node(id) {
                None => return Err(CampaignError::UnknownNode(id.to_string())),
                Some(Node::Branch {
                    flag,
                    cmp,
                    value,
                    then,
                    otherwise,
                    ..
                }) => {
                    id = if cmp.eval(self.flag(flag), *value) {
                        then
                    } else {
                        otherwise
                    };
                }
                Some(node) => return Ok(node.id().to_string()),
            }
        }
        Err(CampaignError::BranchLoop(start.to_string()))
    }

    /// Apply a finished battle, won or lost: copy level/exp/class/stat changes of deployed
    /// officers back, take out the consumables the battle used, set any flags the battle set
    /// and, after a victory only, add won gold/items and record the victory.
    ///
    /// * Player units with an officer in the roster copy level, EXP, class, str/int/lead and
    ///   equipment back (HP, MP and morale are per battle). Only the first player unit of an
    ///   officer counts ([`BattleState::new`] builds one per army officer).
    /// * Exactly the consumables the battle recorded in `battle.items_used` are removed from
    ///   the inventory (a count never drops below 0). Everything else in the inventory stays
    ///   as it is now, including items a scene played during the battle gave (`@item`), which
    ///   `battle.inventory` — the battle's own stock — never saw.
    /// * Flags set by battle events are merged in.
    /// * Only a victory adds `gold_found` / `items_found` (RULES.md §10: treasures, drops and
    ///   `give_item` events) and records the battle in `battles_won`.
    ///
    /// Apply each finished battle once: applying it again would take its items out again.
    /// Put the flags `battle`'s events have set so far into the campaign's. A scene the battle
    /// plays (an event's `drama`, the outro) runs on the campaign and sees them; the battle's
    /// result merges them again at the end ([`CampaignState::apply_battle_result`]).
    pub fn merge_battle_flags(&mut self, battle: &BattleState) {
        for (flag, value) in &battle.flags {
            self.flags.insert(flag.clone(), *value);
        }
    }

    pub fn apply_battle_result(&mut self, pack: &Pack, battle: &BattleState) {
        let mut copied: BTreeSet<&str> = BTreeSet::new();
        for unit in battle.units.iter().filter(|u| u.side == Side::Player) {
            let Some(id) = unit.officer.as_deref() else {
                continue;
            };
            if !copied.insert(id) {
                continue;
            }
            // An away officer did not fight: a unit of theirs is a guest built from
            // `officers.toml` (a player spawn naming them), whose values are not theirs.
            let Some(state) = self.officer_mut(id).filter(|o| !o.away) else {
                continue;
            };
            state.level = unit.level;
            state.exp = unit.exp;
            state.class = unit.class.clone();
            state.strength = unit.strength;
            state.int = unit.int;
            state.lead = unit.lead;
            state.equip = unit.equip.clone();
        }

        for (id, used) in &battle.items_used {
            let left = self.item_count(id).saturating_sub(*used);
            if left == 0 {
                self.inventory.remove(id);
            } else {
                self.inventory.insert(id.clone(), left);
            }
        }

        self.merge_battle_flags(battle);

        if battle.outcome == Some(Outcome::Victory) {
            self.add_gold(pack, battle.gold_found);
            for item in &battle.items_found {
                self.add_item(item, 1);
            }
            if !self.battles_won.contains(&battle.battle_id) {
                self.battles_won.push(battle.battle_id.clone());
            }
        }
    }
}
