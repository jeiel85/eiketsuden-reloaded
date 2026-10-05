//! `hero-tools simulate --campaign`: plays the whole campaign from a new game, AI against AI,
//! the way the game's flow does (`hero-game` `flow.rs`), so each battle is fought with the army
//! the earlier ones left: levels, classes, recruits, items and flags carry over.
//!
//! * **Drama** nodes, the scenes battles play (intro, events, outro) and an ending's scene run
//!   headless ([`DramaRunner`]) with their side effects: flags, gold, items, officers joining
//!   or leaving. A choice takes the option `--choose SCENE=N,N,...` names for that scene's
//!   choices in order (1 = the first), else the first option not taken yet at that question
//!   in this play of the scene (so a question that loops back until answered right is left).
//! * **Camp** nodes buy battle items as a careful player would ([`stock_up`]), equip nothing
//!   and deploy what the camp screen selects when the player changes nothing
//!   ([`camp_deployment`]): the first camp the whole army, later camps that same selection
//!   fitted to their battle (an officer who joined since is not added).
//! * A scene or battle that cannot run fails the run, where the game would show an error and
//!   go on: the tool is a check.
//! * **Battle** nodes are fought by the AI on both sides (at most [`MAX_PHASES`] phases, a seed
//!   per battle made from the run's seed and the battle's number), the player's units pointed
//!   at the battle's goal ([`aim_at_victory`]); with `--level-bonus` every army officer is
//!   that many levels up before their first battle. The result is applied as
//!   the game applies it, then a victory goes to `next`, a defeat to
//!   `on_defeat` or ends the run (game over).
//! * The run ends at an **Ending** node.

use crate::simulate::MAX_PHASES;
use crate::Failure;
use hero_core::battle::{normalize_deployment, BattleEvent, BattleState, Outcome};
use hero_core::battledef::{AiMode, BattleDef, Condition, Side};
use hero_core::campaign::{CampaignState, Difficulty, GameOptions, Node};
use hero_core::data::{Id, ItemDef};
use hero_core::drama::{DramaRunner, Step};
use hero_core::pack::{Pack, Severity};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::panic::{self, AssertUnwindSafe};
use std::path::Path;

/// Campaign nodes visited in one run before it counts as looping.
pub const MAX_NODES: usize = 1000;
/// Choices one play of a scene may ask before it counts as looping.
pub const MAX_CHOICES_PER_SCENE: usize = 100;

/// `--choose` options: scene id -> the option (0-based) to take at each choice the scene asks
/// in the run, in order (a scene played again asks again); past them the default rule of
/// [`Sim::play_scene`] applies.
pub type Choices = BTreeMap<String, Vec<usize>>;

/// One battle fought in a run.
#[derive(Debug, Clone, PartialEq)]
pub struct Fought {
    pub battle: String,
    pub won: bool,
    pub turns: u32,
    /// Average level of the player's officers when the battle began.
    pub level: f64,
}

/// How a run ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum End {
    /// Reached the ending node (id).
    Ending(String),
    /// Lost a battle without `on_defeat`.
    GameOver(String),
    /// A battle did not finish within [`MAX_PHASES`] phases.
    Stuck(String),
    /// More than [`MAX_NODES`] nodes.
    Looping,
    /// The campaign could not go on (a missing node or scene, a bad choice).
    Error(String),
    Panicked(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    pub fought: Vec<Fought>,
    /// The choices taken: `scene: option text`.
    pub chose: Vec<String>,
    /// How many choices each scene asked.
    pub asked: BTreeMap<String, usize>,
    pub end: End,
}

/// `Ok(true)` when no run failed (stuck, looping, an error or a panic).
pub fn run(dir: &Path, seeds: u32, choices: &Choices, options: &Options) -> Result<bool, Failure> {
    let pack = crate::load_pack(dir).map_err(Failure::Failed)?;
    let errors = pack
        .validate()
        .iter()
        .filter(|i| i.severity == Severity::Error)
        .count();
    if errors > 0 {
        return Err(Failure::Failed(format!(
            "the pack has {errors} validation error(s); run `hero-tools validate` first"
        )));
    }
    for scene in choices.keys() {
        if pack.scene(scene).is_none() {
            return Err(Failure::Usage(format!(
                "--choose names unknown scene `{scene}`"
            )));
        }
    }
    if let Some(battle) = &options.trace {
        if !pack.battles.contains_key(battle) {
            return Err(Failure::Usage(format!(
                "--trace names unknown battle `{battle}`"
            )));
        }
    }
    print!("Simulating the campaign x {seeds} seed(s), at most {MAX_PHASES} phases per battle");
    if options.level_bonus > 0 {
        print!(
            ", every army officer {} level(s) up before their first battle",
            options.level_bonus
        );
    }
    let tags = options.game_tags();
    if !tags.is_empty() {
        print!(", new game with {}", tags.join(", "));
    }
    println!("\n");
    let _quiet = crate::simulate::QuietPanics::install();
    let runs: Vec<Run> = (1..=seeds)
        .map(|seed| run_seed(&pack, seed, choices, options))
        .collect();
    let (report, failed) = render(&pack, &runs, choices);
    print!("{report}");
    Ok(!failed)
}

/// `--level-bonus`, `--trace`, `--difficulty` and `--extended-rules`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    /// Levels every army officer gets once, before their first battle.
    pub level_bonus: u32,
    /// A battle whose every phase is written to stderr ([`trace_line`]).
    pub trace: Option<String>,
    /// The new game's choices (D25) the runs start with; the default is the pack as it is.
    pub game: GameOptions,
}

impl Options {
    /// A new game of `pack` with these choices, as the title's new game starts one.
    fn new_game(&self, pack: &Pack) -> CampaignState {
        let mut campaign = CampaignState::new_game(pack);
        campaign.apply_options(self.game);
        campaign
    }

    /// The new game's choices that are on, for the report header.
    fn game_tags(&self) -> Vec<String> {
        let mut tags = Vec::new();
        if self.game.difficulty != Difficulty::Normal {
            tags.push(format!("{:?} difficulty", self.game.difficulty).to_lowercase());
        }
        if self.game.extended_rules {
            tags.push("extended rules".to_string());
        }
        tags
    }
}

/// One run from a new game. Battle `n` of the run is fought with a seed made of `seed` and `n`.
pub fn run_seed(pack: &Pack, seed: u32, choices: &Choices, options: &Options) -> Run {
    let mut sim = Sim {
        pack,
        seed,
        choices,
        options,
        boosted: BTreeSet::new(),
        fought: Vec::new(),
        chose: Vec::new(),
        asked: BTreeMap::new(),
    };
    let end = panic::catch_unwind(AssertUnwindSafe(|| sim.play()))
        .unwrap_or_else(|_| End::Panicked(crate::simulate::take_panic_message()));
    Run {
        fought: sim.fought,
        chose: sim.chose,
        asked: sim.asked,
        end,
    }
}

struct Sim<'a> {
    pack: &'a Pack,
    seed: u32,
    choices: &'a Choices,
    options: &'a Options,
    /// The officers who got the level bonus.
    boosted: BTreeSet<Id>,
    fought: Vec<Fought>,
    chose: Vec<String>,
    asked: BTreeMap<String, usize>,
}

impl Sim<'_> {
    fn traces(&self, battle: &str) -> bool {
        self.options.trace.as_deref() == Some(battle)
    }

    fn play(&mut self) -> End {
        let pack = self.pack;
        let mut campaign = self.options.new_game(pack);
        for _ in 0..MAX_NODES {
            let Some(node) = pack.campaign.node(&campaign.node).cloned() else {
                return End::Error(format!("unknown node `{}`", campaign.node));
            };
            let next = match node {
                Node::Drama { scene, .. } => {
                    if let Err(e) = self.play_scene(&mut campaign, &scene) {
                        return End::Error(e);
                    }
                    campaign.advance(pack)
                }
                Node::Camp { battle, shop, .. } => {
                    stock_up(pack, &mut campaign, &shop);
                    if let Some(def) = battle.as_deref().and_then(|b| pack.battles.get(b)) {
                        campaign.deployed = camp_deployment(pack, def, &campaign);
                    }
                    campaign.advance(pack)
                }
                Node::Battle {
                    battle, on_defeat, ..
                } => {
                    let (state, level) = match self.fight(&mut campaign, &battle) {
                        Ok(fought) => fought,
                        Err(end) => return end,
                    };
                    let won = state.outcome == Some(Outcome::Victory);
                    self.fought.push(Fought {
                        battle: battle.clone(),
                        won,
                        turns: state.turn,
                        level,
                    });
                    campaign.apply_battle_result(pack, &state);
                    match (won, on_defeat) {
                        (true, _) => campaign.advance(pack),
                        (false, Some(node)) => campaign.jump(pack, &node),
                        (false, None) => return End::GameOver(battle),
                    }
                }
                Node::Ending { id, scene, .. } => {
                    if let Some(scene) = scene {
                        if let Err(e) = self.play_scene(&mut campaign, &scene) {
                            return End::Error(e);
                        }
                    }
                    return End::Ending(id);
                }
                Node::Branch { .. } => campaign.advance(pack),
            };
            if let Err(e) = next {
                return End::Error(e.to_string());
            }
        }
        End::Looping
    }

    /// Fight `battle` with the campaign's army; the scenes it plays run on the campaign.
    /// Returns the finished battle and the army's average level at its start.
    fn fight(
        &mut self,
        campaign: &mut CampaignState,
        battle: &str,
    ) -> Result<(BattleState, f64), End> {
        let pack = self.pack;
        let bonus = self.options.level_bonus;
        if bonus > 0 {
            let new: Vec<Id> = campaign
                .roster
                .iter()
                .map(|o| o.id.clone())
                .filter(|id| !self.boosted.contains(id))
                .collect();
            for id in new {
                campaign
                    .add_levels(pack, &id, bonus)
                    .expect("a roster officer is in the army");
                self.boosted.insert(id);
            }
        }
        let seed = (u64::from(self.seed) << 16) | self.fought.len() as u64;
        let mut state = BattleState::new(pack, battle, campaign, seed)
            .map_err(|e| End::Error(format!("battle `{battle}`: {e}")))?;
        let level = average_level(&state);
        if let Some(def) = pack.battles.get(battle) {
            aim_at_victory(pack, &mut state, def);
        }
        let mut events = state.begin(pack);
        let mut phases = 0;
        loop {
            for e in events {
                if let BattleEvent::Drama { scene, .. } = e {
                    // As the game does: the scene sees the flags the battle has set.
                    campaign.merge_battle_flags(&state);
                    self.play_scene(campaign, &scene).map_err(End::Error)?;
                }
            }
            if state.outcome.is_some() {
                if let (true, Some(outcome)) = (self.traces(battle), &state.outcome) {
                    let seed = self.seed;
                    eprintln!("seed {seed} {battle}: {outcome:?} at turn {}", state.turn);
                }
                return Ok((state, level));
            }
            if phases == MAX_PHASES {
                return Err(End::Stuck(battle.to_string()));
            }
            if self.traces(battle) {
                eprintln!("seed {} {}", self.seed, trace_line(&state, phases));
            }
            events = state.run_ai_phase(pack);
            phases += 1;
        }
    }

    /// Play `scene` to its end. At each choice it takes the next `--choose` option of the
    /// scene; past those (or without any), the first option not taken yet at that question
    /// while the scene plays: a question that leads back to itself until the right answer is
    /// given is answered the way a player would, one option after another.
    fn play_scene(&mut self, campaign: &mut CampaignState, scene: &str) -> Result<(), String> {
        let pack = self.pack;
        let at = |e: hero_core::drama::DramaError| format!("scene `{scene}`: {e}");
        let mut runner = DramaRunner::new(pack, scene).map_err(at)?;
        // Options taken at each question (by its position in the scene) during this play.
        let mut taken: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
        let mut asked_here = 0;
        loop {
            match runner.next(pack, campaign).map_err(at)? {
                Step::End => return Ok(()),
                Step::Choice(options) => {
                    asked_here += 1;
                    if asked_here > MAX_CHOICES_PER_SCENE {
                        return Err(format!(
                            "scene `{scene}` asked more than {MAX_CHOICES_PER_SCENE} choices: \
                             --choose it a way out"
                        ));
                    }
                    let asked = self.asked.entry(scene.to_string()).or_default();
                    let tried = taken.entry(runner.pc).or_default();
                    let pick = match self.choices.get(scene).and_then(|list| list.get(*asked)) {
                        Some(&pick) => pick,
                        None => (0..options.len()).find(|i| !tried.contains(i)).unwrap_or(0),
                    };
                    *asked += 1;
                    tried.insert(pick);
                    let Some(text) = options.get(pick) else {
                        return Err(format!(
                            "--choose {scene}: option {} at choice {} of the scene, which offers {} option(s)",
                            pick + 1,
                            *asked,
                            options.len()
                        ));
                    };
                    self.chose.push(format!("{scene}: {text}"));
                    runner.choose(pack, pick).map_err(at)?;
                }
                _ => {}
            }
        }
    }
}

/// Most battle items [`stock_up`] keeps in hand.
pub const STOCK: u32 = 8;

/// What a careful player buys at a camp: the shop's battle items (healing and the like), the
/// cheapest first and one of each in turn, until [`STOCK`] battle items (bought anywhere) are
/// in hand or the gold runs out. The battle AI uses them on units in need.
fn stock_up(pack: &Pack, campaign: &mut CampaignState, shop: &[Id]) {
    let mut wares: Vec<&ItemDef> = shop
        .iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|i| pack.item(i))
        .filter(|d| d.is_battle_item() && d.price > 0)
        .collect();
    wares.sort_by_key(|d| d.price);
    let held = |c: &CampaignState| {
        c.inventory
            .iter()
            .filter(|(id, _)| pack.item(id).is_some_and(ItemDef::is_battle_item))
            .map(|(_, &n)| n)
            .sum::<u32>()
    };
    loop {
        let mut bought = false;
        for d in &wares {
            if held(campaign) >= STOCK {
                return;
            }
            bought |= campaign.buy(pack, &d.id).is_ok();
        }
        if !bought {
            return;
        }
    }
}

/// What the camp screen deploys when the player changes nothing (`initial_selection` in
/// hero-game): the deployment chosen before, fitted to `def`, or the whole army when there is
/// none. The camp stores it, so it carries on to the next camp.
fn camp_deployment(pack: &Pack, def: &BattleDef, campaign: &CampaignState) -> Vec<Id> {
    let chosen: Vec<Id> = if campaign.deployed.is_empty() {
        campaign.roster.iter().map(|o| o.id.clone()).collect()
    } else {
        campaign.deployed.clone()
    };
    normalize_deployment(pack, def, campaign, &chosen)
}

/// Point the player's units at the battle's victory conditions, as a player would: an officer
/// who must reach a tile marches for it (the rest of the army fights on), a battle won by
/// beating one unit sends everyone after it (the last such condition, when there are more:
/// any one wins). Other conditions keep the army's default AI, and a unit the battle gives
/// another AI (a player spawn that holds or marches) keeps it. Before the battle begins, so
/// its opening events can still set AI.
fn aim_at_victory(pack: &Pack, state: &mut BattleState, def: &BattleDef) {
    let mut sent: BTreeSet<usize> = BTreeSet::new();
    for condition in &def.victory {
        if let Condition::Reach {
            who,
            pos,
            radius,
            to,
        } = condition
        {
            for i in 0..state.units.len() {
                let u = &state.units[i];
                let named = who.as_deref().is_none_or(|w| u.matches(w));
                if u.side == Side::Player && u.ai == AiMode::Aggressive && named && sent.insert(i) {
                    let goal = goal_tile(pack, state, i, *pos, *radius, *to);
                    let u = &mut state.units[i];
                    u.ai = AiMode::Advance;
                    u.ai_pos = Some(goal);
                }
            }
        }
    }
    for condition in &def.victory {
        if let Condition::DefeatUnit { target } = condition {
            for (i, u) in state.units.iter_mut().enumerate() {
                let army_default = u.ai == AiMode::Aggressive || u.ai == AiMode::Target;
                if u.side == Side::Player && army_default && !sent.contains(&i) {
                    u.ai = AiMode::Target;
                    u.ai_target = Some(target.clone());
                }
            }
        }
    }
}

/// Where unit `i` heads for a `reach` area: the tile of the area (`pos` with `radius`, or the
/// rectangle to `to`) nearest to it that its class can enter, else `pos`.
fn goal_tile(
    pack: &Pack,
    state: &BattleState,
    i: usize,
    pos: hero_core::geom::Pos,
    radius: i32,
    to: Option<hero_core::geom::Pos>,
) -> hero_core::geom::Pos {
    let u = &state.units[i];
    let move_type = pack.classes.get(&u.class).map(|c| c.move_type.as_str());
    let enterable = |p: hero_core::geom::Pos| {
        let cost = state
            .terrain_at(pack, p)
            .zip(move_type)
            .and_then(|(t, m)| t.move_cost(m));
        cost.is_some_and(|c| c < u8::MAX)
    };
    (0..state.map.height)
        .flat_map(|y| (0..state.map.width).map(move |x| hero_core::geom::Pos::new(x, y)))
        .filter(|&p| hero_core::battledef::in_reach(pos, radius, to, p) && enterable(p))
        .min_by_key(|p| (p.manhattan(u.pos), p.y, p.x))
        .unwrap_or(pos)
}

/// One phase of a traced battle (`--trace`): every unit on the map, its side, tile, HP and AI.
fn trace_line(state: &BattleState, phase: u32) -> String {
    let units: Vec<String> = state
        .units
        .iter()
        .filter(|u| u.is_active())
        .map(|u| {
            let side = match u.side {
                Side::Player => "P",
                Side::Ally => "A",
                Side::Enemy => "E",
            };
            let who = u.officer.as_deref().unwrap_or(&u.name);
            let (x, y, hp, ai) = (u.pos.x, u.pos.y, u.hp, u.ai);
            format!("{side}:{who}@{x},{y} hp{hp} {ai:?}")
        })
        .collect();
    format!(
        "{} turn {} phase {phase}: {}",
        state.battle_id,
        state.turn,
        units.join(" | ")
    )
}

fn average_level(state: &BattleState) -> f64 {
    let levels: Vec<u32> = state
        .units
        .iter()
        .filter(|u| u.side == Side::Player && u.officer.is_some())
        .map(|u| u.level)
        .collect();
    if levels.is_empty() {
        0.0
    } else {
        f64::from(levels.iter().sum::<u32>()) / levels.len() as f64
    }
}

/// The report, and whether any run failed.
pub fn render(pack: &Pack, runs: &[Run], choices: &Choices) -> (String, bool) {
    let mut out = String::new();
    let mut failed = false;
    for scene in choices.keys() {
        if !runs.iter().any(|r| r.asked.contains_key(scene)) {
            let _ = writeln!(
                out,
                "WARNING: --choose {scene}: no run reached a choice of that scene"
            );
        }
    }
    for (i, run) in runs.iter().enumerate() {
        let won = run.fought.iter().filter(|f| f.won).count();
        let end = match &run.end {
            End::Ending(id) => format!("reached the ending `{id}`"),
            End::GameOver(b) => format!("game over at `{b}`"),
            End::Stuck(b) => {
                failed = true;
                format!("FAILED: `{b}` did not finish within {MAX_PHASES} phases")
            }
            End::Looping => {
                failed = true;
                format!("FAILED: more than {MAX_NODES} campaign nodes")
            }
            End::Error(e) => {
                failed = true;
                format!("FAILED: {e}")
            }
            End::Panicked(e) => {
                failed = true;
                format!("FAILED: panicked: {e}")
            }
        };
        let _ = writeln!(
            out,
            "seed {}: won {won}/{} battle(s), {end}",
            i + 1,
            run.fought.len()
        );
        if !run.chose.is_empty() {
            let _ = writeln!(out, "    choices: {}", run.chose.join(" | "));
        }
    }

    // Per battle, in the order first fought.
    let mut order: Vec<&str> = Vec::new();
    let mut stats: BTreeMap<&str, (u32, u32, f64, f64)> = BTreeMap::new();
    for f in runs.iter().flat_map(|r| &r.fought) {
        if !stats.contains_key(f.battle.as_str()) {
            order.push(&f.battle);
        }
        let s = stats.entry(&f.battle).or_default();
        s.0 += 1;
        s.1 += u32::from(f.won);
        s.2 += f64::from(f.turns);
        s.3 += f.level;
    }
    if !order.is_empty() {
        let _ = writeln!(
            out,
            "\n{:<24} {:>7} {:>6} {:>9} {:>9}",
            "battle", "fought", "won", "turns", "level"
        );
    }
    for id in order {
        let (n, won, turns, level) = stats[id];
        let name = pack.battles.get(id).map_or("", |b| b.name.as_str());
        let _ = writeln!(
            out,
            "{:<24} {n:>7} {:>5.0}% {:>9.1} {:>9.1}  {name}",
            id,
            f64::from(won) * 100.0 / f64::from(n),
            turns / f64::from(n),
            level / f64::from(n),
        );
    }
    let endings = runs
        .iter()
        .filter(|r| matches!(r.end, End::Ending(_)))
        .count();
    let _ = writeln!(
        out,
        "\n{} run(s): {endings} reached an ending{}: {}",
        runs.len(),
        if endings == 0 && !runs.is_empty() {
            " (WARNING: none)"
        } else {
            ""
        },
        if failed { "FAILED" } else { "OK" }
    );
    (out, failed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hero_core::battledef::Condition;
    use hero_core::script::{ChoiceOption, Cmd};

    fn fixture() -> Pack {
        crate::tests::fixture_pack()
    }

    fn choose(scene: &str, options: &[usize]) -> Choices {
        Choices::from([(scene.to_string(), options.to_vec())])
    }

    #[test]
    fn a_run_walks_the_campaign_and_carries_the_army() {
        let mut pack = fixture();
        // Levels come quickly, so the first battle surely raises some.
        pack.rules.exp_per_level = 10;
        let run = run_seed(&pack, 1, &Choices::new(), &Options::default());
        // The fixture campaign: oath, camp1, b01, camp2, b02, the ending (with its scene).
        assert_eq!(run.end, End::Ending("finale".into()), "{run:?}");
        let battles: Vec<&str> = run.fought.iter().map(|f| f.battle.as_str()).collect();
        assert_eq!(battles, ["b01", "b02"]);
        // The second battle starts with the army the first one left: the same officers in a
        // new game (deployed as the camps deploy them) would be at their starting levels.
        let mut fresh = CampaignState::new_game(&pack);
        fresh.join(&pack, "jian_yong").unwrap(); // `oath` recruits him
        fresh.deployed = camp_deployment(&pack, &pack.battles["b01"], &fresh);
        fresh.deployed = camp_deployment(&pack, &pack.battles["b02"], &fresh);
        let fresh = BattleState::new(&pack, "b02", &fresh, 0).unwrap();
        assert!(
            run.fought[1].level > average_level(&fresh),
            "{} vs {}",
            run.fought[1].level,
            average_level(&fresh)
        );
        // Runs are deterministic for a seed.
        assert_eq!(
            run,
            run_seed(&pack, 1, &Choices::new(), &Options::default())
        );
    }

    #[test]
    fn a_defeat_follows_on_defeat_or_ends_the_run() {
        let mut pack = fixture();
        // b01 cannot be won: it would take surviving longer than its turn limit.
        let b01 = pack.battles.get_mut("b01").unwrap();
        b01.victory = vec![Condition::SurviveTurns { turns: 99 }];
        b01.turn_limit = 1;
        let run = run_seed(&pack, 1, &Choices::new(), &Options::default());
        assert!(!run.fought[0].won, "{run:?}");
        // `on_defeat = "retreat"` goes on to camp2 and b02.
        assert_eq!(run.fought.len(), 2, "{run:?}");
        assert_eq!(run.end, End::Ending("finale".into()));
        // Without `on_defeat` the run ends there.
        for node in &mut pack.campaign.nodes {
            if let Node::Battle { on_defeat, .. } = node {
                *on_defeat = None;
            }
        }
        let run = run_seed(&pack, 1, &Choices::new(), &Options::default());
        assert_eq!(run.end, End::GameOver("b01".into()));
        assert_eq!(run.fought.len(), 1);
    }

    #[test]
    fn the_camp_stocks_battle_items_while_the_gold_lasts() {
        let pack = fixture();
        // 500 gold and 3 beans; the first camp sells bean (20), wine (50) and two weapons.
        let mut campaign = CampaignState::new_game(&pack);
        let shop: Vec<Id> = ["bean", "wine", "bronze_sword", "long_spear"]
            .map(String::from)
            .to_vec();
        stock_up(&pack, &mut campaign, &shop);
        // The cheapest first, one of each in turn, until 8 battle items are in hand.
        assert_eq!(
            (campaign.item_count("bean"), campaign.item_count("wine")),
            (6, 2)
        );
        assert_eq!(campaign.gold, 500 - 3 * 20 - 2 * 50);
        assert_eq!(campaign.item_count("bronze_sword"), 0);
        // With the stock full nothing more is bought; without gold neither.
        stock_up(&pack, &mut campaign, &shop);
        assert_eq!(campaign.gold, 340);
        let mut poor = CampaignState::new_game(&pack);
        poor.gold = 10;
        stock_up(&pack, &mut poor, &shop);
        assert_eq!((poor.gold, poor.item_count("bean")), (10, 3));
    }

    #[test]
    fn the_army_goes_for_the_battles_goal() {
        let mut pack = fixture();
        let goal = hero_core::geom::Pos::new(1, 1);
        pack.battles.get_mut("b01").unwrap().victory = vec![
            Condition::Reach {
                who: Some("liu_bei".into()),
                pos: goal,
                radius: 0,
                to: None,
            },
            Condition::DefeatUnit {
                target: "deng_mao".into(),
            },
        ];
        let mut campaign = CampaignState::new_game(&pack);
        campaign.deployed = camp_deployment(&pack, &pack.battles["b01"], &campaign);
        let mut state = BattleState::new(&pack, "b01", &campaign, 1).unwrap();
        aim_at_victory(&pack, &mut state, &pack.battles["b01"]);
        let unit = |id: &str| &state.units[state.find_unit(id).unwrap()];
        // Liu Bei marches for his tile; the others go after the unit to beat.
        assert_eq!(
            (unit("liu_bei").ai, unit("liu_bei").ai_pos),
            (AiMode::Advance, Some(goal))
        );
        assert_eq!(
            (unit("guan_yu").ai, unit("guan_yu").ai_target.as_deref()),
            (AiMode::Target, Some("deng_mao"))
        );
        // The enemy is left as the battle has it.
        assert_ne!(unit("deng_mao").ai, AiMode::Target);

        // A rectangle: the tile of it nearest to Liu Bei, not its `pos` corner.
        let from = unit("liu_bei").pos;
        let (far, near) = (
            hero_core::geom::Pos::new(from.x + 6, from.y),
            hero_core::geom::Pos::new(from.x + 3, from.y),
        );
        pack.battles.get_mut("b01").unwrap().victory = vec![Condition::Reach {
            who: Some("liu_bei".into()),
            pos: far,
            radius: 0,
            to: Some(near),
        }];
        let mut state = BattleState::new(&pack, "b01", &campaign, 1).unwrap();
        aim_at_victory(&pack, &mut state, &pack.battles["b01"]);
        let liu = &state.units[state.find_unit("liu_bei").unwrap()];
        let goal = liu.ai_pos.unwrap();
        assert!(
            hero_core::battledef::in_reach(far, 0, Some(near), goal),
            "{goal:?}"
        );
        assert!(goal.manhattan(from) <= 3 + 1, "{goal:?} from {from:?}");
    }

    #[test]
    fn the_level_bonus_comes_once_per_officer() {
        let pack = fixture();
        let bonus = Options {
            level_bonus: 2,
            ..Options::default()
        };
        let plain = run_seed(&pack, 1, &Choices::new(), &Options::default());
        let boosted = run_seed(&pack, 1, &Choices::new(), &bonus);
        assert_eq!(boosted.fought[0].level, plain.fought[0].level + 2.0);
        // Not again at the second battle: no more than the two levels plus what was earned.
        assert!(
            boosted.fought[1].level < plain.fought[1].level + 4.0,
            "{boosted:?} vs {plain:?}"
        );
    }

    #[test]
    fn the_runs_start_with_the_new_game_choices() {
        let pack = fixture();
        assert!(!Options::default().new_game(&pack).off_original());
        assert!(Options::default().game_tags().is_empty());
        let hard = Options {
            game: GameOptions {
                difficulty: Difficulty::Hard,
                free_edit: false,
                extended_rules: true,
            },
            ..Options::default()
        };
        let campaign = hard.new_game(&pack);
        assert_eq!(campaign.difficulty, Difficulty::Hard);
        assert!(campaign.extended_rules);
        assert_eq!(hard.game_tags(), ["hard difficulty", "extended rules"]);
        // Enemies set up from that campaign are the difficulty's levels up.
        let def = pack.battles.keys().next().unwrap().clone();
        let level = |c: &CampaignState| -> Vec<i32> {
            BattleState::new(&pack, &def, c, 1)
                .unwrap()
                .units
                .iter()
                .filter(|u| u.side == Side::Enemy)
                .map(|u| u.level as i32)
                .collect()
        };
        let normal = level(&Options::default().new_game(&pack));
        assert!(!normal.is_empty());
        let offset = Difficulty::Hard.enemy_level_offset();
        let cap = pack.rules.level_cap as i32;
        // As set-up does: never past the cap, but a pack level above it stays.
        let expected: Vec<i32> = normal
            .iter()
            .map(|&l| (l + offset).min(cap.max(l)))
            .collect();
        assert_eq!(level(&campaign), expected);
    }

    #[test]
    fn the_camp_keeps_the_deployment_like_the_game() {
        let pack = fixture();
        let def = &pack.battles["b02"];
        let mut campaign = CampaignState::new_game(&pack);
        campaign.join(&pack, "jian_yong").unwrap();
        let all: Vec<Id> = campaign.roster.iter().map(|o| o.id.clone()).collect();
        // Nothing chosen yet: the whole army, fitted to the battle.
        assert_eq!(
            camp_deployment(&pack, def, &campaign),
            normalize_deployment(&pack, def, &campaign, &all)
        );
        // Chosen before: that choice, fitted again (an officer who joined since is not added).
        campaign.deployed = vec!["liu_bei".into()];
        let kept = camp_deployment(&pack, def, &campaign);
        assert_eq!(
            kept,
            normalize_deployment(&pack, def, &campaign, &campaign.deployed)
        );
        assert!(kept.contains(&"liu_bei".to_string()));
        assert_ne!(kept, normalize_deployment(&pack, def, &campaign, &all));
    }

    #[test]
    fn choices_pick_their_option_per_visit() {
        let pack = fixture();
        // The prologue's scene `oath` asks whether to pursue.
        let first = run_seed(&pack, 1, &Choices::new(), &Options::default());
        assert_eq!(first.chose[0], "oath: 적을 끝까지 쫓는다");
        assert_eq!(first.asked["oath"], 1);
        let second = run_seed(&pack, 1, &choose("oath", &[1]), &Options::default());
        assert_eq!(second.chose[0], "oath: 마을을 지킨다");
        // An option the choice does not have ends the run with an error that says so.
        let bad = run_seed(&pack, 1, &choose("oath", &[8]), &Options::default());
        assert_eq!(
            bad.end,
            End::Error(
                "--choose oath: option 9 at choice 1 of the scene, which offers 2 option(s)".into()
            )
        );
        assert!(bad.fought.is_empty());

        // A scene that asks again takes the options in order, then the default: two more
        // choices right after the label every path of `oath` passes.
        let mut pack = fixture();
        let oath = pack.scenes.get_mut("oath").unwrap();
        let at = oath.labels["recruit"] + 1;
        for i in oath.labels.values_mut() {
            if *i >= at {
                *i += 2;
            }
        }
        oath.labels.insert("second".into(), at + 1);
        oath.labels.insert("after_again".into(), at + 2);
        let ask = |to: &str| {
            Cmd::Choice(vec![
                ChoiceOption {
                    text: "하나".into(),
                    label: to.into(),
                },
                ChoiceOption {
                    text: "둘".into(),
                    label: to.into(),
                },
            ])
        };
        oath.cmds.insert(at, ask("after_again"));
        oath.cmds.insert(at, ask("second"));
        let run = run_seed(&pack, 1, &choose("oath", &[0, 1]), &Options::default());
        assert_eq!(
            run.chose[..3],
            ["oath: 적을 끝까지 쫓는다", "oath: 둘", "oath: 하나"],
            "{run:?}"
        );
        assert_eq!(run.asked["oath"], 3);
    }

    #[test]
    fn a_question_that_leads_back_to_itself_is_left() {
        // After `@label recruit` of `oath`: a question whose first answer asks it again.
        let with_loop = |exit: bool| {
            let mut pack = fixture();
            let oath = pack.scenes.get_mut("oath").unwrap();
            let at = oath.labels["recruit"] + 1;
            for i in oath.labels.values_mut() {
                if *i >= at {
                    *i += 1;
                }
            }
            oath.labels.insert("again".into(), at);
            oath.labels.insert("out".into(), at + 1);
            let out = if exit { "out" } else { "again" };
            oath.cmds.insert(
                at,
                Cmd::Choice(vec![
                    ChoiceOption {
                        text: "다시".into(),
                        label: "again".into(),
                    },
                    ChoiceOption {
                        text: "나간다".into(),
                        label: out.into(),
                    },
                ]),
            );
            pack
        };
        let run = run_seed(&with_loop(true), 1, &Choices::new(), &Options::default());
        assert_eq!(run.chose[1..3], ["oath: 다시", "oath: 나간다"], "{run:?}");
        assert_eq!(run.end, End::Ending("finale".into()));
        // A question with no way out is reported, not played forever.
        let run = run_seed(&with_loop(false), 1, &Choices::new(), &Options::default());
        assert!(
            matches!(&run.end, End::Error(e) if e.contains("asked more than 100 choices")),
            "{:?}",
            run.end
        );
    }

    #[test]
    fn the_base_packs_question_is_asked_again_until_it_goes_on() {
        // `test_opening` explains the controls and asks again until the player sets out.
        let pack = crate::load_pack(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/base"))
            .unwrap();
        let choices = Choices::new();
        let mut sim = Sim {
            pack: &pack,
            seed: 1,
            choices: &choices,
            options: &Options::default(),
            boosted: BTreeSet::new(),
            fought: Vec::new(),
            chose: Vec::new(),
            asked: BTreeMap::new(),
        };
        let mut campaign = CampaignState::new_game(&pack);
        sim.play_scene(&mut campaign, "test_opening").unwrap();
        assert_eq!(
            sim.chose,
            [
                "test_opening: 조작 방법을 듣는다",
                "test_opening: 사수관으로 출진한다"
            ]
        );
    }

    #[test]
    fn the_report_counts_battles_and_failures() {
        let pack = fixture();
        let battle = pack.battles.keys().next().unwrap().clone();
        let fought = |won| Fought {
            battle: battle.clone(),
            won,
            turns: 4,
            level: 5.0,
        };
        let runs = [
            Run {
                fought: vec![fought(true)],
                chose: vec![],
                asked: BTreeMap::from([("oath".to_string(), 1)]),
                end: End::Ending("finale".into()),
            },
            Run {
                fought: vec![fought(false)],
                chose: vec!["s: a".into()],
                asked: BTreeMap::new(),
                end: End::GameOver(battle.clone()),
            },
        ];
        let (report, failed) = render(&pack, &runs, &choose("oath", &[0]));
        assert!(!failed, "{report}");
        assert!(report.contains("50%"), "{report}");
        assert!(report.contains("1 reached an ending"), "{report}");
        assert!(!report.contains("WARNING: --choose"), "{report}");
        // A --choose whose scene never asked is reported.
        let (report, _) = render(&pack, &runs, &choose("mercy", &[0]));
        assert!(report.contains("WARNING: --choose mercy"), "{report}");
        let stuck = [Run {
            fought: vec![],
            chose: vec![],
            asked: BTreeMap::new(),
            end: End::Stuck(battle.clone()),
        }];
        let (report, failed) = render(&pack, &stuck, &Choices::new());
        assert!(failed && report.contains("WARNING: none"), "{report}");
    }
}
