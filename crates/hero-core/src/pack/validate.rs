//! Cross-reference checks behind [`Pack::validate`]. Every check is documented for modders in
//! `docs/MODDING.md` ("Validation"); keep the two in sync.

use super::{Issue, Pack, Severity};
use crate::battledef::{AiMode, BattleDef, Condition, EventAction, Side, Trigger, UnitSpawn};
use crate::campaign::Node;
use crate::data::{
    Area, ClassDef, Effect, Equipment, ItemKind, RangeSpec, StrategyKind, TargetSide,
};
use crate::geom::Pos;
use crate::map::BattleMap;
use crate::script::Cmd;
use std::collections::{BTreeMap, BTreeSet};

/// Move type that every deploy slot must be passable for (ordinary infantry). A pack that has
/// no terrain cost for it instead needs its slots passable for at least one class move type.
pub(super) const FOOT_MOVE_TYPE: &str = "foot";

pub(super) fn validate(pack: &Pack) -> Vec<Issue> {
    let mut v = Validator::new(pack);
    v.rules();
    v.terrain();
    v.classes();
    v.strategies();
    v.items();
    v.officers();
    v.maps();
    for battle in pack.battles.values() {
        v.battle(battle);
    }
    v.dramas();
    v.campaign();
    v.flags();
    v.issues
}

/// A drama speaker that is written like an id (`liu_bei`) must name an officer; free display
/// names (`장비`, `Messenger`) are shown as they are.
pub(super) fn looks_like_id(s: &str) -> bool {
    s.starts_with(|c: char| c.is_ascii_lowercase())
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn kind_name(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::Weapon => "weapon",
        ItemKind::Armor => "armor",
        ItemKind::Accessory => "accessory",
        ItemKind::Consumable => "consumable",
    }
}

fn range_name(r: &RangeSpec) -> String {
    match r {
        RangeSpec::Named(n) => format!("`{n}`"),
        RangeSpec::Offsets(o) => format!("{o:?}"),
    }
}

/// Whether `key` is a media key: `/`-separated non-empty parts of ASCII letters, digits, `_`
/// and `-` (so it cannot leave the media folder it is looked up in).
pub(super) fn is_media_key(key: &str) -> bool {
    !key.is_empty()
        && key.split('/').all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        })
}

fn at(p: Pos) -> String {
    format!("[{}, {}]", p.x, p.y)
}

/// Campaign nodes a node can continue to.
fn successors(node: &Node) -> Vec<&str> {
    match node {
        Node::Drama { next, .. } | Node::Camp { next, .. } => vec![next],
        Node::Battle {
            next, on_defeat, ..
        } => {
            let mut v = vec![next.as_str()];
            v.extend(on_defeat.as_deref());
            v
        }
        Node::Branch {
            then, otherwise, ..
        } => vec![then, otherwise],
        Node::Ending { .. } => vec![],
    }
}

struct Validator<'a> {
    pack: &'a Pack,
    issues: Vec<Issue>,
    /// Class families that exist (`ClassDef::family`).
    families: BTreeSet<&'a str>,
    /// Move types used by classes.
    class_move_types: BTreeSet<&'a str>,
    /// Whether some terrain has a cost for [`FOOT_MOVE_TYPE`].
    foot_defined: bool,
    /// Officers that can be in the player's army: starting officers and `@join` targets.
    player_officers: BTreeSet<&'a str>,
}

impl<'a> Validator<'a> {
    fn new(pack: &'a Pack) -> Self {
        let mut player_officers: BTreeSet<&str> = pack
            .campaign
            .starting_officers
            .iter()
            .map(|s| s.as_str())
            .collect();
        for scene in pack.scenes.values() {
            for cmd in &scene.cmds {
                if let Cmd::Join(o) = cmd {
                    player_officers.insert(o);
                }
            }
        }
        Validator {
            pack,
            issues: Vec::new(),
            families: pack.classes.values().map(|c| c.family.as_str()).collect(),
            class_move_types: pack
                .classes
                .values()
                .map(|c| c.move_type.as_str())
                .collect(),
            foot_defined: pack
                .terrain
                .iter()
                .any(|t| t.cost.contains_key(FOOT_MOVE_TYPE)),
            player_officers,
        }
    }

    fn push(&mut self, severity: Severity, context: &str, msg: String) {
        self.issues.push(Issue {
            severity,
            context: context.to_string(),
            msg,
        });
    }

    fn error(&mut self, context: &str, msg: impl Into<String>) {
        self.push(Severity::Error, context, msg.into());
    }

    fn warn(&mut self, context: &str, msg: impl Into<String>) {
        self.push(Severity::Warning, context, msg.into());
    }

    fn level_ok(&self, level: u32) -> bool {
        (1..=self.pack.rules.level_cap).contains(&level)
    }

    // ----- rules/game.toml -----------------------------------------------------------------

    fn rules(&mut self) {
        let ctx = self.pack.files.game.source_path();
        let r = &self.pack.rules;
        if r.level_cap == 0 {
            self.error(&ctx, "level_cap must be at least 1");
        }
        if r.exp_per_level == 0 {
            self.error(&ctx, "exp_per_level must be at least 1");
        }
        if r.gold_cap < 0 {
            self.error(&ctx, "gold_cap must not be negative");
        }
        if r.mp_cap < 0 {
            self.error(&ctx, "mp_cap must not be negative");
        }
        if !(0..=100).contains(&r.morale_start) {
            self.error(&ctx, "morale_start must be within 0..=100");
        }
        if r.morale_loss_pct < 0 {
            self.error(&ctx, "morale_loss_pct must not be negative");
        }
        if !(0..=100).contains(&r.confuse_morale) {
            self.error(&ctx, "confuse_morale must be within 0..=100");
        }
        for (name, table) in [("exp_attack", &r.exp_attack), ("exp_kill", &r.exp_kill)] {
            if table.is_empty() {
                self.warn(&ctx, format!("{name} is empty, so it never awards EXP"));
            }
            if table.windows(2).any(|w| w[0][0] >= w[1][0]) {
                self.error(
                    &ctx,
                    format!("{name} must be sorted by strictly increasing level difference"),
                );
            }
            if table.iter().any(|p| p[1] < 0) {
                self.error(&ctx, format!("{name} contains a negative EXP value"));
            }
        }
        if r.counter_divisor <= 0 {
            self.error(&ctx, "counter_divisor must be positive");
        }
        if r.counter_damage_pct < 0 {
            self.error(&ctx, "counter_damage_pct must not be negative");
        }
        let w = &r.weather;
        let sum = w.clear + w.cloudy + w.rain;
        if w.clear < 0 || w.cloudy < 0 || w.rain < 0 || sum != 100 {
            self.error(
                &ctx,
                format!("weather chances must be non-negative and sum to 100 (they sum to {sum})"),
            );
        }
        for (attacker, row) in &r.affinity {
            if !self.families.contains(attacker.as_str()) {
                self.warn(
                    &ctx,
                    format!("affinity names unknown class family `{attacker}`"),
                );
            }
            for (defender, pct) in row {
                if !self.families.contains(defender.as_str()) {
                    self.warn(
                        &ctx,
                        format!("affinity names unknown class family `{defender}`"),
                    );
                }
                if *pct <= 0 {
                    self.error(
                        &ctx,
                        format!("affinity {attacker} -> {defender} must be a positive percentage"),
                    );
                }
            }
        }
    }

    // ----- rules/terrain.toml --------------------------------------------------------------

    fn terrain(&mut self) {
        let pack = self.pack;
        if pack.terrain.is_empty() {
            self.error(&pack.files.terrain.source_path(), "no terrain is defined");
        }
        let strategy_elements: BTreeSet<&str> = pack
            .strategies
            .values()
            .filter_map(|s| s.element.as_deref())
            .collect();
        for t in &pack.terrain {
            let ctx = format!("terrain {}", t.id);
            if t.glyph.is_whitespace() || t.glyph.is_control() {
                self.error(
                    &ctx,
                    format!("glyph {:?} must be a visible character", t.glyph),
                );
            }
            if !(0..=100).contains(&t.defense) {
                self.error(&ctx, "defense must be within 0..=100");
            }
            if !(0..=100).contains(&t.heal_hp) {
                self.error(&ctx, "heal_hp must be within 0..=100");
            }
            if !(0..=100).contains(&t.heal_morale) {
                self.error(&ctx, "heal_morale must be within 0..=100");
            }
            for (move_type, cost) in &t.cost {
                if *cost == 0 {
                    self.error(
                        &ctx,
                        format!("movement cost for `{move_type}` must be at least 1"),
                    );
                }
                if !self.class_move_types.contains(move_type.as_str()) {
                    self.warn(
                        &ctx,
                        format!("cost for move type `{move_type}`, which no class uses"),
                    );
                }
            }
            for e in &t.elements {
                if !strategy_elements.contains(e.as_str()) {
                    self.warn(&ctx, format!("element `{e}` is not used by any strategy"));
                }
            }
            for e in &t.boost {
                if !t.elements.contains(e) {
                    self.warn(
                        &ctx,
                        format!("boosted element `{e}` is not listed in `elements`, so it can never apply"),
                    );
                }
            }
        }
    }

    // ----- rules/classes.toml --------------------------------------------------------------

    fn classes(&mut self) {
        let pack = self.pack;
        let cap = pack.rules.level_cap;
        let terrain_move_types: BTreeSet<&str> = pack
            .terrain
            .iter()
            .flat_map(|t| t.cost.keys().map(|k| k.as_str()))
            .collect();
        for c in pack.classes.values() {
            let ctx = format!("class {}", c.id);
            if c.name.trim().is_empty() {
                self.warn(&ctx, "name is empty");
            }
            if c.family.trim().is_empty() {
                self.error(&ctx, "family must not be empty");
            }
            if !terrain_move_types.contains(c.move_type.as_str()) {
                self.error(
                    &ctx,
                    format!(
                        "move type `{}` has a cost on no terrain, so units of this class can never move",
                        c.move_type
                    ),
                );
            }
            match c.range.offsets() {
                None => self.error(
                    &ctx,
                    format!("unknown attack range {}", range_name(&c.range)),
                ),
                Some(o) if o.contains(&Pos::new(0, 0)) => {
                    self.warn(&ctx, "attack range includes the unit's own tile")
                }
                Some(_) => {}
            }
            if c.hp <= 0 {
                self.error(&ctx, "hp must be positive");
            }
            if c.hp_growth < 0 {
                self.warn(
                    &ctx,
                    "hp_growth is negative: units lose troops when they level up",
                );
            }
            if !(1..=3).contains(&c.tier) {
                self.warn(&ctx, "tier should be 1, 2 or 3");
            }
            if c.sprite.trim().is_empty() {
                self.error(&ctx, "sprite key must not be empty");
            }
            if c.generic.iter().any(|s| !(0..=100).contains(s)) {
                self.warn(&ctx, "generic [str, int, lead] should be within 0..=100");
            }
            for learn in &c.strategies {
                if pack.strategy(&learn.id).is_none() {
                    self.error(&ctx, format!("learns unknown strategy `{}`", learn.id));
                }
                if !(1..=cap).contains(&learn.level) {
                    self.warn(
                        &ctx,
                        format!(
                            "learns `{}` at level {}, outside 1..={cap}",
                            learn.id, learn.level
                        ),
                    );
                }
            }
            if let Some(p) = &c.promote {
                if p.to == c.id {
                    self.error(&ctx, "promotes to itself");
                } else if pack.class(&p.to).is_none() {
                    self.error(&ctx, format!("promotes to unknown class `{}`", p.to));
                }
                match pack.item(&p.item) {
                    None => self.error(&ctx, format!("promotion item `{}` does not exist", p.item)),
                    Some(item) if !item.effects.contains(&Effect::Promote) => self.error(
                        &ctx,
                        format!(
                            "promotion item `{}` has no `promote` effect, so it cannot be used",
                            p.item
                        ),
                    ),
                    Some(_) => {}
                }
                if p.level > cap {
                    self.warn(
                        &ctx,
                        format!("promotion level {} is above level_cap {cap}", p.level),
                    );
                }
            }
        }
        self.promotion_cycles();
        let mut promoted_from: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for c in pack.classes.values() {
            if let Some(p) = &c.promote {
                promoted_from
                    .entry(p.to.as_str())
                    .or_default()
                    .push(c.id.as_str());
            }
        }
        for (to, from) in promoted_from {
            if from.len() > 1 {
                self.warn(
                    &format!("class {to}"),
                    format!(
                        "is the promotion target of several classes ({}); inherited strategies come from `{}` only",
                        from.join(", "),
                        from[0]
                    ),
                );
            }
        }
    }

    /// Report each promotion cycle once (at its alphabetically first class).
    fn promotion_cycles(&mut self) {
        let pack = self.pack;
        for start in pack.classes.values() {
            let mut path = vec![start.id.as_str()];
            let mut cur = start;
            while let Some(next) = cur.promote.as_ref().and_then(|p| pack.class(&p.to)) {
                if next.id == start.id {
                    if path.iter().all(|id| *id >= start.id.as_str()) {
                        path.push(start.id.as_str());
                        self.error(
                            &format!("class {}", start.id),
                            format!("promotion chain loops: {}", path.join(" -> ")),
                        );
                    }
                    break;
                }
                if path.contains(&next.id.as_str()) {
                    break; // a loop that does not include `start`; reported from its own members
                }
                path.push(next.id.as_str());
                cur = next;
            }
        }
    }

    // ----- rules/strategies.toml -----------------------------------------------------------

    fn strategies(&mut self) {
        let pack = self.pack;
        let terrain_elements: BTreeSet<&str> = pack
            .terrain
            .iter()
            .flat_map(|t| t.elements.iter().map(|e| e.as_str()))
            .collect();
        for s in pack.strategies.values() {
            let ctx = format!("strategy {}", s.id);
            if s.name.trim().is_empty() {
                self.warn(&ctx, "name is empty");
            }
            if s.mp < 0 {
                self.error(&ctx, "mp must not be negative");
            }
            if s.range.offsets().is_none() {
                self.error(&ctx, format!("unknown range {}", range_name(&s.range)));
            }
            if let Some(e) = &s.element {
                if !terrain_elements.contains(e.as_str()) {
                    self.error(
                        &ctx,
                        format!("element `{e}` is allowed by no terrain, so the strategy can never be cast"),
                    );
                }
            }
            if s.effects.is_empty() {
                self.error(&ctx, "has no effects");
            }
            for e in &s.effects {
                match e {
                    Effect::Promote | Effect::ChangeClass { .. } => self.error(
                        &ctx,
                        "`promote` and `change_class` effects only work on items",
                    ),
                    Effect::Damage { power } | Effect::Heal { power } if *power < 0 => {
                        self.warn(&ctx, "effect power is negative")
                    }
                    Effect::Status { turns: 0, .. } => self.warn(&ctx, "status lasts 0 turns"),
                    _ => {}
                }
            }
            match (s.kind, s.target) {
                (StrategyKind::Attack, TargetSide::Ally) => {
                    self.warn(&ctx, "attack strategy is aimed at allies")
                }
                (StrategyKind::Heal, TargetSide::Enemy) => {
                    self.warn(&ctx, "heal strategy is aimed at enemies")
                }
                _ => {}
            }
        }
    }

    // ----- rules/items.toml ----------------------------------------------------------------

    fn items(&mut self) {
        let pack = self.pack;
        let promotion_items: BTreeSet<&str> = pack
            .classes
            .values()
            .filter_map(|c| c.promote.as_ref().map(|p| p.item.as_str()))
            .collect();
        for i in pack.items.values() {
            let ctx = format!("item {}", i.id);
            if i.name.trim().is_empty() {
                self.warn(&ctx, "name is empty");
            }
            for f in &i.families {
                if !self.families.contains(f.as_str()) {
                    self.error(&ctx, format!("unknown class family `{f}`"));
                }
            }
            if let Some(s) = &i.strategy {
                if pack.strategy(s).is_none() {
                    self.error(&ctx, format!("casts unknown strategy `{s}`"));
                }
            }
            let mut camp_effect = false;
            for e in &i.effects {
                match e {
                    Effect::ChangeClass { to } => {
                        camp_effect = true;
                        if pack.class(to).is_none() {
                            self.error(&ctx, format!("changes to unknown class `{to}`"));
                        }
                    }
                    Effect::Promote => {
                        camp_effect = true;
                        if !promotion_items.contains(i.id.as_str()) {
                            self.warn(&ctx, "no class is promoted with this item");
                        }
                    }
                    Effect::Damage { .. } | Effect::Status { .. } => self.warn(
                        &ctx,
                        "`damage` and `status` effects do nothing on items (give the item a `strategy` instead)",
                    ),
                    Effect::Heal { .. } | Effect::Morale { .. } => {}
                }
            }
            if i.kind == ItemKind::Consumable {
                if i.effects.is_empty() && i.strategy.is_none() {
                    self.warn(&ctx, "consumable has no effects and casts no strategy");
                }
                if camp_effect && i.battle_use {
                    self.warn(
                        &ctx,
                        "class items are used in camp; `battle_use` has no effect on them",
                    );
                }
                let battle_effect = i.strategy.is_some()
                    || i.effects
                        .iter()
                        .any(|e| matches!(e, Effect::Heal { .. } | Effect::Morale { .. }));
                if battle_effect && !i.battle_use {
                    self.warn(
                        &ctx,
                        "battle effects without `battle_use = true` can never be used",
                    );
                }
                if i.atk_pct != 0
                    || i.def_pct != 0
                    || i.move_bonus != 0
                    || i.regen_hp != 0
                    || i.regen_morale != 0
                {
                    self.warn(&ctx, "equipment bonuses are ignored on consumables");
                }
                if !i.families.is_empty() {
                    self.warn(&ctx, "families are ignored on consumables");
                }
            } else {
                if !i.effects.is_empty() || i.strategy.is_some() {
                    self.warn(&ctx, "effects and strategies are ignored on equipment");
                }
                if i.battle_use {
                    self.warn(&ctx, "`battle_use` is ignored on equipment");
                }
                if i.kind != ItemKind::Weapon && i.atk_pct != 0 {
                    self.warn(&ctx, "atk_pct only counts on weapons");
                }
                if i.kind != ItemKind::Armor && i.def_pct != 0 {
                    self.warn(&ctx, "def_pct only counts on armor");
                }
                if i.kind != ItemKind::Accessory && i.move_bonus != 0 {
                    self.warn(&ctx, "move_bonus only counts on accessories");
                }
            }
        }
    }

    // ----- officers.toml -------------------------------------------------------------------

    fn officers(&mut self) {
        let pack = self.pack;
        let cap = pack.rules.level_cap;
        for o in pack.officers.values() {
            let ctx = format!("officer {}", o.id);
            if o.name.trim().is_empty() {
                self.warn(&ctx, "name is empty");
            }
            let family = match pack.class(&o.class) {
                Some(c) => Some(c.family.as_str()),
                None => {
                    self.error(&ctx, format!("unknown class `{}`", o.class));
                    None
                }
            };
            if !self.level_ok(o.level) {
                self.error(&ctx, format!("level {} is outside 1..={cap}", o.level));
            }
            for (stat, value) in [("str", o.strength), ("int", o.int), ("lead", o.lead)] {
                if !(0..=100).contains(&value) {
                    self.warn(&ctx, format!("{stat} {value} should be within 0..=100"));
                }
            }
            self.equipment(&ctx, &o.equip, family);
        }
    }

    /// Items exist and sit in the slot of their kind. The family restriction is only a warning
    /// here: enemy officers may carry anything, and the camp enforces it for the player.
    fn equipment(&mut self, ctx: &str, equip: &Equipment, family: Option<&str>) {
        let slots = [
            ("weapon", ItemKind::Weapon, &equip.weapon),
            ("armor", ItemKind::Armor, &equip.armor),
            ("accessory", ItemKind::Accessory, &equip.accessory),
        ];
        for (slot, kind, id) in slots {
            let Some(id) = id else { continue };
            match self.pack.item(id) {
                None => self.error(ctx, format!("{slot} `{id}` does not exist")),
                Some(item) if item.kind != kind => self.error(
                    ctx,
                    format!(
                        "`{id}` ({}) cannot go in the {slot} slot",
                        kind_name(item.kind)
                    ),
                ),
                Some(item) => {
                    if let Some(f) = family {
                        if !item.families.is_empty() && !item.families.iter().any(|x| x == f) {
                            self.warn(ctx, format!("`{id}` is not meant for class family `{f}`"));
                        }
                    }
                }
            }
        }
    }

    // ----- battles/*.toml ------------------------------------------------------------------

    fn passable(&self, map: &BattleMap, p: Pos, move_type: &str) -> bool {
        map.terrain_at(p)
            .and_then(|t| self.pack.terrain(t))
            .and_then(|t| t.move_cost(move_type))
            .is_some()
    }

    fn deployable(&self, map: &BattleMap, p: Pos) -> bool {
        if self.foot_defined {
            self.passable(map, p, FOOT_MOVE_TYPE)
        } else {
            self.class_move_types
                .iter()
                .any(|m| self.passable(map, p, m))
        }
    }

    fn unit_class(&self, u: &UnitSpawn) -> Option<&'a ClassDef> {
        let pack = self.pack;
        let class = match &u.class {
            Some(c) => c.as_str(),
            None => pack.officer(u.officer.as_deref()?)?.class.as_str(),
        };
        pack.class(class)
    }

    /// Maps of the map files, used by a battle or not (a pack may ship maps ahead of the
    /// battles that play on them).
    fn maps(&mut self) {
        for m in self.pack.maps.values() {
            let ctx = format!("map {}", m.id);
            self.map_legend(&ctx, &m.legend);
            self.map_image(&ctx, m.image.as_deref());
        }
    }

    fn map_legend(&mut self, ctx: &str, legend: &BTreeMap<String, String>) {
        for (glyph, terrain) in legend {
            if glyph.chars().count() != 1 {
                self.error(
                    ctx,
                    format!("map legend key `{glyph}` must be exactly one character"),
                );
            }
            if self.pack.terrain(terrain).is_none() {
                self.error(
                    ctx,
                    format!("map legend maps `{glyph}` to unknown terrain `{terrain}`"),
                );
            }
        }
    }

    /// The picture layer key names `gfx/maps/<key>.png`, so it must stay a plain relative
    /// name (no `..`, no drive or absolute path) on every platform and over HTTP.
    fn map_image(&mut self, ctx: &str, image: Option<&str>) {
        if let Some(key) = image {
            if !is_media_key(key) {
                self.error(
                    ctx,
                    format!("map image `{key}` must be a media key: `/`-separated parts of letters, digits, `_` and `-`"),
                );
            }
        }
    }

    fn battle(&mut self, b: &'a BattleDef) {
        let pack = self.pack;
        let ctx = format!("battle {}", b.id);
        if b.name.trim().is_empty() {
            self.warn(&ctx, "name is empty");
        }
        if b.turn_limit == 0 {
            self.error(&ctx, "turn_limit must be at least 1");
        }
        // A map taken from a map file was checked as `map <id>` (once for all its battles).
        if b.map.use_map.is_none() {
            self.map_legend(&ctx, &b.map.legend);
            self.map_image(&ctx, b.map.image.as_deref());
        }
        let map = match BattleMap::parse(&b.map.rows, &b.map.legend, &pack.terrain) {
            Ok(m) => Some(m),
            Err(e) => {
                self.error(&ctx, format!("map: {e}"));
                None
            }
        };
        let map = map.as_ref();

        // Names that conditions, events and AI targets may use.
        let mut names: BTreeSet<&str> = self.player_officers.clone();
        names.extend(b.deploy.required.iter().map(|s| s.as_str()));
        for u in &b.units {
            names.extend(u.tag.as_deref());
            names.extend(u.officer.as_deref());
        }

        let mut occupied: BTreeSet<Pos> = BTreeSet::new();
        self.deploy(&ctx, b, map, &mut occupied);
        self.units(&ctx, b, map, &names, &mut occupied);
        // Only when defeating every enemy is the one way to win: with another victory
        // condition or a `victory` event, an enemy out of reach does not block the win.
        let other_win = b.victory.iter().any(|c| *c != Condition::DefeatAll)
            || b.events
                .iter()
                .flat_map(|e| e.all_actions())
                .any(|a| matches!(a, EventAction::Victory));
        if let (Some(map), true, false) =
            (map, b.victory.contains(&Condition::DefeatAll), other_win)
        {
            self.reachability(&ctx, b, map);
        }

        for c in &b.victory {
            self.condition(&ctx, "victory", c, b, map, &names);
        }
        for c in &b.defeat {
            self.condition(&ctx, "defeat", c, b, map, &names);
        }
        if let Some(bonus) = &b.bonus {
            self.condition(&ctx, "bonus", &bonus.condition, b, map, &names);
        }
        let event_victory = b
            .events
            .iter()
            .any(|e| e.all_actions().contains(&&EventAction::Victory));
        if b.victory.is_empty() && !event_victory {
            self.error(&ctx, "no victory condition and no event grants victory");
        }
        if !b.units.iter().any(|u| u.side == Side::Enemy) {
            self.warn(&ctx, "has no enemy units");
        }

        let groups: BTreeSet<&str> = b.units.iter().filter_map(|u| u.group.as_deref()).collect();
        let mut spawned: BTreeSet<&str> = BTreeSet::new();
        for (i, e) in b.events.iter().enumerate() {
            let ectx = format!("{ctx} event #{}", i + 1);
            self.trigger(&ectx, &e.trigger, b, map, &names);
            if e.actions.is_empty() {
                self.warn(&ectx, "has no actions");
            }
            for a in e.all_actions() {
                if let EventAction::Spawn { group } = a {
                    spawned.insert(group);
                    if !groups.contains(group.as_str()) {
                        self.error(
                            &ectx,
                            format!("spawns group `{group}`, but no unit belongs to it"),
                        );
                    }
                }
                self.action(&ectx, a, map, &names);
            }
        }
        for g in groups.difference(&spawned) {
            self.warn(
                &ctx,
                format!("units of group `{g}` never appear: no event spawns the group"),
            );
        }
        let stages: BTreeSet<u32> = b
            .events
            .iter()
            .flat_map(|e| e.all_actions())
            .filter_map(|a| match a {
                EventAction::SetStage { stage } => Some(*stage),
                _ => None,
            })
            .chain([0])
            .collect();
        for (i, e) in b.events.iter().enumerate() {
            if let Some(stage) = e.stage.filter(|s| !stages.contains(s)) {
                self.warn(
                    &format!("{ctx} event #{}", i + 1),
                    format!("fires only at stage {stage}, which no event's set_stage reaches"),
                );
            }
        }

        let mut treasure_tiles = BTreeSet::new();
        for t in &b.treasures {
            let tctx = format!("{ctx} treasure {}", at(t.pos));
            if let Some(map) = map {
                if !map.in_bounds(t.pos) {
                    self.error(&tctx, "is outside the map");
                }
            }
            if !treasure_tiles.insert(t.pos) {
                self.error(&tctx, "another treasure is on the same tile");
            }
            if let Some(item) = &t.item {
                if pack.item(item).is_none() {
                    self.error(&tctx, format!("gives unknown item `{item}`"));
                }
            }
            if t.gold < 0 {
                self.error(&tctx, "gold must not be negative");
            }
            if t.item.is_none() && t.gold == 0 {
                self.warn(&tctx, "gives neither an item nor gold");
            }
        }
        if b.reward_gold < 0 {
            self.error(&ctx, "reward_gold must not be negative");
        }
        for (what, scene) in [("intro", &b.intro), ("outro", &b.outro)] {
            if let Some(scene) = scene {
                if pack.scene(scene).is_none() {
                    self.error(&ctx, format!("{what} scene `{scene}` does not exist"));
                }
            }
        }
        self.battle_scene_items(&ctx, b);
    }

    /// `@item` of a battle item in a scene played while the battle runs (intro, `drama`
    /// actions) goes to the army's inventory, not to the battle's own stock, which was copied
    /// when the battle started: the item cannot be used in this battle, only from the next one
    /// on (`CampaignState::apply_battle_result` keeps it). The outro plays after the battle has
    /// ended, so it is not checked.
    fn battle_scene_items(&mut self, ctx: &str, b: &BattleDef) {
        let pack = self.pack;
        let mut scenes: Vec<&str> = b.intro.iter().map(|s| s.as_str()).collect();
        for e in &b.events {
            for a in e.all_actions() {
                if let EventAction::Drama { scene } = a {
                    scenes.push(scene);
                }
            }
        }
        scenes.sort_unstable();
        scenes.dedup();
        for id in scenes {
            let Some(scene) = pack.scene(id) else {
                continue; // reported above
            };
            for cmd in &scene.cmds {
                let Cmd::Item(item) = cmd else { continue };
                if pack.item(item).is_some_and(|d| d.is_battle_item()) {
                    self.warn(
                        &format!("{ctx} scene {id}"),
                        format!(
                            "@item `{item}` cannot be used in this battle: the scene plays during the battle and gives the item to the army's inventory, not to the battle's stock (it is usable from the next battle on)"
                        ),
                    );
                }
            }
        }
    }

    fn deploy(
        &mut self,
        ctx: &str,
        b: &BattleDef,
        map: Option<&BattleMap>,
        occupied: &mut BTreeSet<Pos>,
    ) {
        let pack = self.pack;
        let d = &b.deploy;
        if d.max == 0 {
            self.error(ctx, "deploy.max must be at least 1");
        }
        if d.max as usize > d.slots.len() {
            self.error(
                ctx,
                format!(
                    "deploy.max is {} but only {} deploy slots exist",
                    d.max,
                    d.slots.len()
                ),
            );
        }
        let mut required = BTreeSet::new();
        for o in &d.required {
            match pack.officer(o) {
                None => self.error(ctx, format!("required officer `{o}` does not exist")),
                Some(_) if !required.insert(o.as_str()) => {
                    self.warn(ctx, format!("required officer `{o}` is listed twice"))
                }
                Some(_) => {}
            }
            if d.forbidden.contains(o) {
                self.error(ctx, format!("officer `{o}` is both required and forbidden"));
            }
        }
        for o in &d.forbidden {
            if pack.officer(o).is_none() {
                self.error(ctx, format!("forbidden officer `{o}` does not exist"));
            }
        }
        // The lord is deployed implicitly (unless the battle is fought without them: forbidden),
        // so it needs a place next to the required officers.
        let mut must_deploy = required.clone();
        for o in &pack.campaign.starting_officers {
            if pack.officer(o).is_some_and(|def| def.lord) && !d.forbidden.contains(o) {
                must_deploy.insert(o.as_str());
            }
        }
        if must_deploy.len() > d.max as usize {
            self.error(
                ctx,
                format!(
                    "{} officers must be deployed (required officers and the lord) but deploy.max is {}",
                    must_deploy.len(),
                    d.max
                ),
            );
        }
        for slot in &d.slots {
            let sctx = format!("{ctx} deploy slot {}", at(*slot));
            if let Some(map) = map {
                if !map.in_bounds(*slot) {
                    self.error(
                        &sctx,
                        format!("is outside the {}x{} map", map.width, map.height),
                    );
                } else if !self.deployable(map, *slot) {
                    let terrain = map.terrain_at(*slot).unwrap_or("?");
                    self.error(
                        &sctx,
                        format!("is on `{terrain}`, which foot units cannot enter"),
                    );
                }
            }
            if !occupied.insert(*slot) {
                self.error(&sctx, "is listed twice");
            }
        }
    }

    fn units(
        &mut self,
        ctx: &str,
        b: &BattleDef,
        map: Option<&BattleMap>,
        names: &BTreeSet<&str>,
        occupied: &mut BTreeSet<Pos>,
    ) {
        let pack = self.pack;
        let mut tags = BTreeSet::new();
        let mut officers = BTreeSet::new();
        for (i, u) in b.units.iter().enumerate() {
            let label = u
                .tag
                .as_deref()
                .or(u.officer.as_deref())
                .or(u.name.as_deref())
                .unwrap_or("generic");
            let uctx = format!("{ctx} unit #{} ({label})", i + 1);
            match &u.officer {
                Some(o) => {
                    if pack.officer(o).is_none() {
                        self.error(&uctx, format!("unknown officer `{o}`"));
                    }
                    if !officers.insert(o.as_str()) {
                        self.error(
                            &uctx,
                            format!("officer `{o}` appears more than once in this battle"),
                        );
                    }
                    if b.deploy.required.contains(o) {
                        self.error(
                            &uctx,
                            format!("officer `{o}` is also a required player officer"),
                        );
                    } else if u.side == Side::Player
                        && pack.campaign.starting_officers.contains(o)
                        // (Only when it sets what is ignored: a spawn that just places the
                        // army's officer, such as one arriving later, is what it means.)
                        && (u.class.is_some() || u.level.is_some() || u.equip.is_some())
                    {
                        self.warn(
                            &uctx,
                            format!(
                                "officer `{o}` is in the starting army: the battle places the army's `{o}` here instead of on a deploy slot and ignores this unit's class, level and equipment"
                            ),
                        );
                    }
                    if u.stats.is_some() {
                        self.warn(&uctx, "stats are ignored for named officers");
                    }
                }
                None => {
                    if u.class.is_none() {
                        self.error(&uctx, "a generic unit needs a class");
                    }
                    if u.level.is_none() {
                        self.error(&uctx, "a generic unit needs a level");
                    }
                    if u.name.is_none() {
                        self.warn(&uctx, "a generic unit should have a display name");
                    }
                }
            }
            if let Some(c) = &u.class {
                if pack.class(c).is_none() {
                    self.error(&uctx, format!("unknown class `{c}`"));
                }
            }
            if let Some(level) = u.level {
                if !self.level_ok(level) {
                    self.error(
                        &uctx,
                        format!("level {level} is outside 1..={}", pack.rules.level_cap),
                    );
                }
            }
            let class = self.unit_class(u);
            if let Some(map) = map {
                if !map.in_bounds(u.pos) {
                    self.error(
                        &uctx,
                        format!(
                            "position {} is outside the {}x{} map",
                            at(u.pos),
                            map.width,
                            map.height
                        ),
                    );
                } else if let Some(class) = class {
                    if !self.passable(map, u.pos, &class.move_type) {
                        let terrain = map.terrain_at(u.pos).unwrap_or("?");
                        let msg = format!(
                            "position {} is on `{terrain}`, which move type `{}` cannot enter",
                            at(u.pos),
                            class.move_type
                        );
                        if u.group.is_some() {
                            self.warn(&uctx, format!("{msg}; the reinforcement will be shifted"));
                        } else {
                            self.error(&uctx, msg);
                        }
                    }
                }
                if let Some(p) = u.ai_pos {
                    if !map.in_bounds(p) {
                        self.error(&uctx, format!("ai_pos {} is outside the map", at(p)));
                    }
                }
            }
            if u.group.is_none() && !occupied.insert(u.pos) {
                self.error(
                    &uctx,
                    format!(
                        "position {} is already taken by another unit or a deploy slot",
                        at(u.pos)
                    ),
                );
            }
            if let Some(tag) = &u.tag {
                if tag.trim().is_empty() {
                    self.error(&uctx, "tag must not be empty");
                } else if !tags.insert(tag.as_str()) {
                    self.error(&uctx, format!("duplicate tag `{tag}`"));
                }
                if pack.officer(tag).is_some() {
                    self.warn(
                        &uctx,
                        format!(
                            "tag `{tag}` is also an officer id; references to it are ambiguous"
                        ),
                    );
                }
            }
            if u.ai == AiMode::Target && u.ai_target.is_none() {
                self.error(&uctx, "ai = \"target\" needs an ai_target");
            }
            if u.ai == AiMode::March && u.ai_target.is_none() && u.ai_pos.is_none() {
                self.warn(
                    &uctx,
                    "ai = \"march\" without ai_target or ai_pos has nowhere to go; the unit waits",
                );
            }
            if let Some(t) = &u.ai_target {
                if !names.contains(t.as_str()) {
                    self.error(
                        &uctx,
                        format!("ai_target `{t}` names no unit of this battle"),
                    );
                }
            }
            if let Some(equip) = &u.equip {
                self.equipment(&uctx, equip, class.map(|c| c.family.as_str()));
            }
            if let Some(item) = &u.drop {
                if pack.item(item).is_none() {
                    self.error(&uctx, format!("drops unknown item `{item}`"));
                }
            }
        }
    }

    fn reference(&mut self, ctx: &str, names: &BTreeSet<&str>, what: &str, name: &str) {
        if !names.contains(name) {
            self.error(
                ctx,
                format!(
                    "{what} `{name}` matches no unit tag, officer of this battle or player officer"
                ),
            );
        }
    }

    /// The area of a `reach`: `pos` (and `to`) inside the map, a radius of at least 0 and none
    /// with `to`.
    fn position(
        &mut self,
        ctx: &str,
        map: Option<&BattleMap>,
        p: Pos,
        radius: i32,
        to: Option<Pos>,
    ) {
        if let Some(map) = map {
            for p in std::iter::once(p).chain(to) {
                if !map.in_bounds(p) {
                    self.error(ctx, format!("position {} is outside the map", at(p)));
                }
            }
        }
        if radius < 0 {
            self.error(ctx, "radius must not be negative");
        }
        if to.is_some() && radius != 0 {
            self.error(ctx, "a reach with `to` is a rectangle and takes no radius");
        }
    }

    fn condition(
        &mut self,
        ctx: &str,
        what: &str,
        c: &Condition,
        b: &BattleDef,
        map: Option<&BattleMap>,
        names: &BTreeSet<&str>,
    ) {
        let cctx = format!("{ctx} {what}");
        for (field, name) in c.unit_refs() {
            self.reference(&cctx, names, field, name);
        }
        match c {
            Condition::DefeatAll => {
                if !b
                    .units
                    .iter()
                    .any(|u| u.side == Side::Enemy && u.group.is_none())
                {
                    self.error(&cctx, "defeat_all, but no enemy unit starts on the map");
                }
            }
            Condition::DefeatCommander => {
                if !b.units.iter().any(|u| u.side == Side::Enemy && u.commander) {
                    self.error(&cctx, "defeat_commander, but no enemy unit is a commander");
                }
            }
            Condition::DefeatUnit { .. } | Condition::UnitRetreated { .. } => {}
            Condition::Reach {
                pos, radius, to, ..
            } => {
                self.position(&cctx, map, *pos, *radius, *to);
            }
            Condition::SurviveTurns { turns } => {
                if *turns == 0 {
                    self.error(&cctx, "survive_turns needs at least 1 turn");
                } else if *turns > b.turn_limit {
                    self.warn(
                        &cctx,
                        format!(
                            "survive_turns {turns} can never be met: turn_limit is {}",
                            b.turn_limit
                        ),
                    );
                }
            }
        }
    }

    /// In a battle won only by `defeat_all`, every enemy on the map from the start must be
    /// attackable from somewhere the player's side can walk to from the deployment slots (or
    /// the start tiles of its own and allied units), or the battle cannot be won. Generous on
    /// purpose: a tile counts as walkable when any class of the pack can enter it, before or
    /// after a `set_terrain` event changes it; the enemy may walk too; an enemy counts as
    /// reached when any class's attack range or any damage strategy's reach (with its area)
    /// touches it; enemies a `retreat` event removes are left out.
    fn reachability(&mut self, ctx: &str, b: &BattleDef, map: &BattleMap) {
        let pack = self.pack;
        let move_types: BTreeSet<&str> = pack
            .classes
            .values()
            .map(|c| c.move_type.as_str())
            .collect();
        let walkable = |terrain: &str| {
            pack.terrain(terrain)
                .is_some_and(|t| move_types.iter().any(|m| t.move_cost(m).is_some()))
        };
        let changed: Vec<(Pos, &str)> = b
            .events
            .iter()
            .flat_map(|e| e.all_actions())
            .filter_map(|a| match a {
                EventAction::SetTerrain { pos, terrain, .. } => Some((*pos, terrain.as_str())),
                _ => None,
            })
            .collect();
        let open = |p: Pos| {
            map.terrain_at(p).is_some_and(walkable)
                || changed.iter().any(|&(q, t)| q == p && walkable(t))
        };
        let flood = |start: Vec<Pos>| {
            let mut seen: BTreeSet<Pos> = start.iter().copied().collect();
            let mut queue = start;
            while let Some(p) = queue.pop() {
                for n in p.neighbors4() {
                    if map.in_bounds(n) && open(n) && seen.insert(n) {
                        queue.push(n);
                    }
                }
            }
            seen
        };
        // Only the slots the army can fill: setup places at most `deploy.max` officers, in
        // slot order.
        let starts: Vec<Pos> = b
            .deploy
            .slots
            .iter()
            .take(crate::battle::deploy_max(b))
            .copied()
            .chain(
                b.units
                    .iter()
                    .filter(|u| u.side != Side::Enemy)
                    .map(|u| u.pos),
            )
            .filter(|&p| map.in_bounds(p))
            .collect();
        let reached = flood(starts);
        if reached.is_empty() {
            return; // no deployment: reported elsewhere
        }
        let mut offsets: BTreeSet<Pos> = BTreeSet::new();
        for c in pack.classes.values() {
            offsets.extend(c.range.offsets().unwrap_or_default());
        }
        for s in pack.strategies.values() {
            if !s.effects.iter().any(|e| matches!(e, Effect::Damage { .. })) {
                continue;
            }
            for o in s.range.offsets().unwrap_or_default() {
                offsets.insert(o);
                if s.area == Area::Cross {
                    offsets.extend(o.neighbors4());
                }
            }
        }
        // Tiles from which the player's side can hit something.
        let hit: BTreeSet<Pos> = reached
            .iter()
            .flat_map(|p| offsets.iter().map(move |o| p.offset(o.x, o.y)))
            .collect();
        let removed: BTreeSet<&str> = b
            .events
            .iter()
            .flat_map(|e| e.all_actions())
            .filter_map(|a| match a {
                EventAction::Retreat { target } => Some(target.as_str()),
                _ => None,
            })
            .collect();
        for u in b
            .units
            .iter()
            .filter(|u| u.side == Side::Enemy && u.group.is_none())
        {
            let ids = [u.tag.as_deref(), u.officer.as_deref()];
            if ids.iter().flatten().any(|id| removed.contains(id)) {
                continue;
            }
            if hit.contains(&u.pos) || flood(vec![u.pos]).iter().any(|p| hit.contains(p)) {
                continue;
            }
            let who = u
                .officer
                .as_deref()
                .or(u.tag.as_deref())
                .or(u.name.as_deref())
                .unwrap_or("enemy");
            self.warn(
                ctx,
                format!(
                    "defeat_all is the only way to win, but {who} at ({}, {}) can never be attacked: no tile it can walk to is in reach of anywhere the player's units can walk to from the deployment slots",
                    u.pos.x, u.pos.y
                ),
            );
        }
    }

    fn trigger(
        &mut self,
        ctx: &str,
        t: &Trigger,
        b: &BattleDef,
        map: Option<&BattleMap>,
        names: &BTreeSet<&str>,
    ) {
        for (field, name) in t.unit_refs() {
            self.reference(ctx, names, field, name);
        }
        match t {
            Trigger::TurnStart { turn, .. } => {
                if *turn == 0 {
                    self.error(ctx, "turn_start needs a turn of at least 1");
                } else if *turn > b.turn_limit {
                    self.warn(
                        ctx,
                        format!(
                            "turn {turn} is after turn_limit {}; it never fires",
                            b.turn_limit
                        ),
                    );
                }
            }
            Trigger::UnitDefeated { .. } => {}
            Trigger::Reach {
                pos, radius, to, ..
            } => {
                self.position(ctx, map, *pos, *radius, *to);
            }
            Trigger::Adjacent { a, b } => {
                if a.as_ref() == Some(b) {
                    self.warn(ctx, "adjacent trigger names the same unit twice");
                }
            }
            Trigger::HpBelow { pct, .. } => {
                if !(1..=100).contains(pct) {
                    self.error(ctx, "hp_below pct must be within 1..=100");
                }
            }
        }
    }

    fn action(
        &mut self,
        ctx: &str,
        a: &EventAction,
        map: Option<&BattleMap>,
        names: &BTreeSet<&str>,
    ) {
        let pack = self.pack;
        for (field, name) in a.unit_refs() {
            self.reference(ctx, names, field, name);
        }
        match a {
            EventAction::Drama { scene } => {
                if pack.scene(scene).is_none() {
                    self.error(ctx, format!("plays unknown scene `{scene}`"));
                }
            }
            EventAction::Spawn { .. } => {} // checked against the battle's groups by the caller
            // Its actions are checked one by one by the caller ([`EventAction::all`]).
            EventAction::When { actions, .. } => {
                if actions.is_empty() {
                    self.warn(ctx, "has a `when` action without actions");
                }
            }
            EventAction::SetAi {
                ai,
                ai_target,
                ai_pos,
                ..
            } => {
                if let Some(p) = ai_pos {
                    self.position(ctx, map, *p, 0, None);
                }
                if *ai == AiMode::Advance && ai_pos.is_none() {
                    self.warn(
                        ctx,
                        "set_ai to `advance` without an ai_pos clears the destination; the unit then behaves as `aggressive`",
                    );
                }
                if *ai == AiMode::March && ai_target.is_none() && ai_pos.is_none() {
                    self.warn(
                        ctx,
                        "set_ai to `march` without ai_target or ai_pos has nowhere to go; the unit waits",
                    );
                }
            }
            EventAction::Retreat { .. } => {}
            EventAction::LevelUp { amount, .. } => {
                if *amount == 0 {
                    self.warn(ctx, "level_up by 0 levels does nothing");
                }
            }
            EventAction::GiveItem { item } => {
                if pack.item(item).is_none() {
                    self.error(ctx, format!("gives unknown item `{item}`"));
                }
            }
            EventAction::SetFlag { flag, .. } => {
                if flag.trim().is_empty() {
                    self.error(ctx, "set_flag needs a flag name");
                }
            }
            EventAction::SetTerrain {
                pos,
                terrain,
                image,
            } => {
                if pack.terrain(terrain).is_none() {
                    self.error(ctx, format!("set_terrain to unknown terrain `{terrain}`"));
                }
                self.position(ctx, map, *pos, 0, None);
                self.map_image(ctx, image.as_deref());
            }
            EventAction::SetObjective { text } => {
                if text.trim().is_empty() {
                    self.error(ctx, "set_objective with an empty text".to_string());
                }
            }
            EventAction::GiveGold { .. }
            | EventAction::SetStage { .. }
            | EventAction::Halve { .. }
            | EventAction::Victory
            | EventAction::Defeat => {}
        }
    }

    // ----- dramas --------------------------------------------------------------------------

    fn dramas(&mut self) {
        let pack = self.pack;
        for scene in pack.scenes.values() {
            let ctx = format!("scene {}", scene.id);
            // A duel is open from its `@duel` to its `@duel_end` in the order the lines are
            // written. Jumps are not followed: a move before any `@duel` is an error, one after
            // an `@duel_end` only a warning (a jump may lead there with the duel still open).
            let (mut duel_open, mut duel_seen) = (false, false);
            for cmd in &scene.cmds {
                match cmd {
                    Cmd::Duel { left, right, .. } => {
                        (duel_open, duel_seen) = (true, true);
                        for o in [left, right] {
                            if pack.officer(o).is_none() {
                                self.error(&ctx, format!("@duel names unknown officer `{o}`"));
                            }
                        }
                    }
                    Cmd::DuelAct { .. } | Cmd::DuelEnd if !duel_open => {
                        let word = if matches!(cmd, Cmd::DuelEnd) {
                            "duel_end"
                        } else {
                            "duel_act"
                        };
                        if duel_seen {
                            self.warn(
                                &ctx,
                                format!(
                                    "@{word} after the duel's @duel_end (fails when played there;                                      fine if a jump reaches it with the duel open)"
                                ),
                            );
                        } else {
                            self.error(&ctx, format!("@{word} before any @duel"));
                        }
                    }
                    Cmd::DuelEnd => duel_open = false,
                    Cmd::Join(o) | Cmd::Leave(o) | Cmd::Away(o) => {
                        if pack.officer(o).is_none() {
                            let word = match cmd {
                                Cmd::Join(_) => "join",
                                Cmd::Leave(_) => "leave",
                                _ => "away",
                            };
                            self.error(&ctx, format!("@{word} names unknown officer `{o}`"));
                        }
                    }
                    Cmd::Item(item) => {
                        if pack.item(item).is_none() {
                            self.error(&ctx, format!("@item names unknown item `{item}`"));
                        }
                    }
                    Cmd::Level { officer, .. } => {
                        if pack.officer(officer).is_none() {
                            self.error(&ctx, format!("@level names unknown officer `{officer}`"));
                        }
                    }
                    Cmd::Class { officer, class } => {
                        if pack.officer(officer).is_none() {
                            self.error(&ctx, format!("@class names unknown officer `{officer}`"));
                        }
                        if pack.class(class).is_none() {
                            self.error(&ctx, format!("@class names unknown class `{class}`"));
                        }
                    }
                    Cmd::Say { speaker, .. }
                        if looks_like_id(speaker) && pack.officer(speaker).is_none() =>
                    {
                        self.error(
                            &ctx,
                            format!("speaker `{speaker}` looks like an officer id, but no such officer exists"),
                        );
                    }
                    _ => {}
                }
            }
        }
        let mut used: BTreeSet<&str> = BTreeSet::new();
        for node in &pack.campaign.nodes {
            match node {
                Node::Drama { scene, .. } => {
                    used.insert(scene);
                }
                Node::Ending {
                    scene: Some(scene), ..
                } => {
                    used.insert(scene);
                }
                _ => {}
            }
        }
        for b in pack.battles.values() {
            used.extend(b.intro.as_deref());
            used.extend(b.outro.as_deref());
            for e in &b.events {
                for a in e.all_actions() {
                    if let EventAction::Drama { scene } = a {
                        used.insert(scene);
                    }
                }
            }
        }
        // A parent pack's scene that the top pack no longer plays (it replaced the battle that
        // did) is the parent's business, not a flaw of the pack being checked.
        for id in pack
            .scenes
            .keys()
            .filter(|id| !pack.parent_scenes.contains(*id))
        {
            if !used.contains(id.as_str()) {
                self.warn(
                    &format!("scene {id}"),
                    "is never played: no campaign node or battle refers to it",
                );
            }
        }
    }

    // ----- campaign.toml -------------------------------------------------------------------

    fn campaign(&mut self) {
        let pack = self.pack;
        let c = &pack.campaign;
        let ctx = "campaign";
        if c.title.trim().is_empty() {
            self.warn(ctx, "title is empty");
        }
        match c.node(&c.start) {
            None => self.error(ctx, format!("start node `{}` does not exist", c.start)),
            Some(Node::Branch { .. }) => self.error(
                ctx,
                "the start node must not be a branch (every flag is 0 when a game starts)",
            ),
            Some(_) => {}
        }
        let mut seen = BTreeSet::new();
        let mut has_lord = false;
        for o in &c.starting_officers {
            match pack.officer(o) {
                None => self.error(ctx, format!("starting officer `{o}` does not exist")),
                Some(def) => has_lord |= def.lord,
            }
            if !seen.insert(o.as_str()) {
                self.warn(ctx, format!("starting officer `{o}` is listed twice"));
            }
        }
        if !has_lord {
            self.error(ctx, "no starting officer is a lord (`lord = true`)");
        }
        for (item, count) in &c.starting_items {
            if pack.item(item).is_none() {
                self.error(ctx, format!("starting item `{item}` does not exist"));
            }
            if *count == 0 {
                self.warn(ctx, format!("starting item `{item}` has a count of 0"));
            }
        }
        if !(0..=pack.rules.gold_cap).contains(&c.starting_gold) {
            self.warn(
                ctx,
                "starting_gold is outside 0..=gold_cap and will be clamped",
            );
        }

        for node in &c.nodes {
            let nctx = format!("campaign node {}", node.id());
            match node {
                Node::Drama { scene, .. } => {
                    if pack.scene(scene).is_none() {
                        self.error(&nctx, format!("scene `{scene}` does not exist"));
                    }
                }
                Node::Camp { shop, battle, .. } => {
                    let mut listed = BTreeSet::new();
                    for item in shop {
                        match pack.item(item) {
                            None => self.error(&nctx, format!("shop item `{item}` does not exist")),
                            Some(def) if def.price == 0 => self.warn(
                                &nctx,
                                format!("shop item `{item}` has price 0 and cannot be bought"),
                            ),
                            Some(_) => {}
                        }
                        if !listed.insert(item.as_str()) {
                            self.warn(&nctx, format!("shop item `{item}` is listed twice"));
                        }
                    }
                    if let Some(battle) = battle {
                        if !pack.battles.contains_key(battle) {
                            self.error(&nctx, format!("battle `{battle}` does not exist"));
                        }
                    }
                }
                Node::Battle { battle, .. } => {
                    if !pack.battles.contains_key(battle) {
                        self.error(&nctx, format!("battle `{battle}` does not exist"));
                    }
                }
                Node::Branch { flag, .. } => {
                    if flag.trim().is_empty() {
                        self.error(&nctx, "branch needs a flag name");
                    }
                }
                Node::Ending { scene, .. } => {
                    if let Some(scene) = scene {
                        if pack.scene(scene).is_none() {
                            self.error(&nctx, format!("scene `{scene}` does not exist"));
                        }
                    }
                }
            }
            for next in successors(node) {
                if c.node(next).is_none() {
                    self.error(&nctx, format!("continues to unknown node `{next}`"));
                }
            }
        }

        // Reachability from the start node.
        let mut reached: BTreeSet<&str> = BTreeSet::new();
        let mut queue = vec![c.start.as_str()];
        while let Some(id) = queue.pop() {
            let Some(node) = c.node(id) else { continue };
            if reached.insert(node.id()) {
                queue.extend(successors(node));
            }
        }
        if c.node(&c.start).is_some() {
            for node in &c.nodes {
                if !reached.contains(node.id()) {
                    self.warn(
                        &format!("campaign node {}", node.id()),
                        "is unreachable from the start node",
                    );
                }
            }
            let ending_reachable = c
                .nodes
                .iter()
                .any(|n| matches!(n, Node::Ending { .. }) && reached.contains(n.id()));
            if !ending_reachable {
                self.warn(ctx, "no ending node is reachable from the start node");
            }
        }
        self.branch_loops();

        let used: BTreeSet<&str> = c
            .nodes
            .iter()
            .filter_map(|n| match n {
                Node::Battle { battle, .. } => Some(battle.as_str()),
                _ => None,
            })
            .collect();
        for id in pack.battles.keys() {
            if !used.contains(id.as_str()) {
                self.warn(
                    &format!("battle {id}"),
                    "is not used by any campaign battle node",
                );
            }
        }
    }

    /// Branch nodes that can lead back to themselves through other branch nodes only.
    /// Flags do not change while branches are resolved, so such a loop is only safe if the
    /// flag tests make it impossible to go all the way round; `advance` reports the loop at run
    /// time otherwise.
    fn branch_loops(&mut self) {
        let c = &self.pack.campaign;
        let branch_next = |id: &str| -> Vec<&str> {
            match c.node(id) {
                Some(n @ Node::Branch { .. }) => successors(n),
                _ => Vec::new(),
            }
        };
        let mut looping = Vec::new();
        for node in &c.nodes {
            if !matches!(node, Node::Branch { .. }) {
                continue;
            }
            let mut seen = BTreeSet::new();
            let mut stack = branch_next(node.id());
            while let Some(id) = stack.pop() {
                if id == node.id() {
                    looping.push(node.id());
                    break;
                }
                if seen.insert(id) {
                    stack.extend(branch_next(id));
                }
            }
        }
        if !looping.is_empty() {
            self.warn(
                "campaign",
                format!(
                    "branch nodes {} can lead back to themselves; `advance` fails if the flags ever send it round the loop",
                    looping.join(", ")
                ),
            );
        }
    }

    // ----- flags ---------------------------------------------------------------------------

    /// Flags that are tested somewhere but never set anywhere are always 0 (usually a typo).
    fn flags(&mut self) {
        let pack = self.pack;
        let mut set: BTreeSet<&str> = BTreeSet::new();
        let mut read: Vec<(String, &str)> = Vec::new();
        for scene in pack.scenes.values() {
            for cmd in &scene.cmds {
                match cmd {
                    Cmd::Set { flag, .. } => {
                        set.insert(flag);
                    }
                    Cmd::If { cond, .. } => read.push((format!("scene {}", scene.id), &cond.flag)),
                    _ => {}
                }
            }
        }
        for b in pack.battles.values() {
            for (i, e) in b.events.iter().enumerate() {
                for a in e.all_actions() {
                    match a {
                        EventAction::SetFlag { flag, .. } => {
                            set.insert(flag);
                        }
                        EventAction::When { when, unless, .. } => {
                            for c in when.iter().chain(unless) {
                                read.push((format!("battle {} event #{}", b.id, i + 1), &c.flag));
                            }
                        }
                        _ => {}
                    }
                }
                for c in e.when.iter().chain(&e.unless) {
                    read.push((format!("battle {} event #{}", b.id, i + 1), &c.flag));
                }
            }
        }
        for node in &pack.campaign.nodes {
            if let Node::Branch { id, flag, .. } = node {
                read.push((format!("campaign node {id}"), flag));
            }
        }
        let mut reported = BTreeSet::new();
        for (ctx, flag) in read {
            if !set.contains(flag) && reported.insert((ctx.clone(), flag)) {
                self.warn(
                    &ctx,
                    format!("flag `{flag}` is tested but never set by any scene or battle event"),
                );
            }
        }
    }
}
