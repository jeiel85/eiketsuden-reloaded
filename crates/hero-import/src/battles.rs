//! The **original battles** of the original mode: the base pack's battles re-staged the way the
//! original stages them, on the original battle maps.
//!
//! The base pack's prologue and chapter 1 retell the original's battles ([`ORIGINAL_BATTLES`] pairs
//! each base battle with the original battle it follows). For each pair the converter keeps the
//! base battle's story (name, objective text, intro and outro scenes, music, rewards, the events
//! that still make sense) and takes from the original:
//!
//! * the map: `[map] use = "hexz_NN"`, the converted `HEXZMAP` entry the scenario loads
//!   (`load_map` `0x3NNN`, FORMATS §13.3);
//! * the turn limit (header of `battle_setup`, `0x03`);
//! * the deployment tiles: the player slots of `battle_setup`, in order (slots that need a flag are
//!   left out). A named officer of the setup whom the base battle has as an allied or guest unit
//!   keeps that side at the original tile; one the setup marks as AI-controlled (third unknown
//!   byte 1: Zhao Yun at Jieqiao, Cao Cao's officers at Xiapi) becomes an allied unit;
//! * the units: the enemy and allied rosters (`battle_roster`, `0x22`) loaded with the map —
//!   officer, tile, class, level and AI; where the scenario picks a roster with `if_flags`, the
//!   one for the route of the base battle ([`Pairing::flags`]). A `BAKDATA` person with a base-pack
//!   officer of the same name plays as that officer; any other person as a generic unit with the
//!   `BAKDATA` name. A base unit of the same officer lends its `tag`, `drop`, `equip` and
//!   `commander`. Units the roster keeps off the map (second unknown byte 1; the original's events
//!   bring them in with `join_battle`) are left out until those events are converted;
//! * the treasures: trigger records `unit_at_cell` (kind 6) for any unit whose script gives gold
//!   (`data` kind 2) or an item;
//! * a `reach` victory or bonus condition of Liu Bei: the tile (`unit_at_cell`) or rectangle
//!   (`unit_in_area`) of the record for Liu Bei whose script runs the battle routine (`data`
//!   kind 4) in the battle's first phase — the original's "Liu Bei reaches the gate" objectives;
//! * the AI the opening script gives (`set_ai` in group 2);
//! * the opening's lines of a chapter's battle (group 2, [`OPENING_GROUP`]): its dialogue, duels,
//!   retreats and flags play when the battle begins, before the phases' own opening scripts (the
//!   setup's own changes — AI, arrivals, objective — are already in the battle's units);
//! * the mid-battle events: every trigger record of the battle's phases ([`FIRST_PHASE_GROUP`]
//!   on) becomes an event ([`convert`]): its trigger ([`trigger_of`]), its dialogue as a drama
//!   scene written from the player's copy (duels included, as dialogue with sound), and its
//!   actions — units joining (`spawn`), AI changes, levels, retreats, gold and items, map cells
//!   that change (a gate opens, a drawbridge comes down: `set_terrain` with the new chips'
//!   picture) and the end of the battle. The original's trigger groups are phases (FORMATS
//!   §13.2): a parallel group watches all its records until one leaves parallel control, any
//!   other group runs the first record whose trigger holds; a battle with more than one phase
//!   gets `stage`s. Where the base battle keeps an event with the same trigger (the duels the
//!   base pack tells in its own words), the base event stays and the record is left out.
//!
//! * the civilians of a setup ([`Names::civilians`]: the people of Changban): a setup slot that
//!   names one is an allied `civilian` unit on that tile (not a deploy tile), which the opening
//!   marches to its village (`march`) and enemies hunt by its tag;
//! * a battle that goes on with another battle map ([`MapLeg`]: the record that sets the next
//!   battle up and ends this one with `battle_end`) is two battles, [`battle_leg`] 0 and 1.
//!
//! What cannot follow is left out and listed as a note: base units whose officer the original
//! roster does not have, events and conditions that name them or a tile of the base map,
//! reinforcement groups, and the parts of the original's scripts the engine has no counterpart
//! for (music, campaign flags, changes of allegiance). Objective texts a script changes become
//! `set_objective` actions.
//!
//! # Mapping rules
//!
//! * **Which setup.** A battle's `battle_setup` precedes its rosters in the scene; where a scene
//!   offers two battles (a choice), the right one is the latest setup whose victory officer is in
//!   the battle's enemy roster, else the latest without a victory officer ([`find_battle`]).
//! * **Coordinates.** In rosters and setups the first coordinate byte is the column: every tile
//!   lies inside the map only this way round (verified on all 19 battles of the prologue and
//!   chapter 1). Two-byte AI tile parameters hold the column in the low byte. Trigger records
//!   store **row, column** (record bytes 4 and 5): read that way, every tile whose script gives
//!   gold or an item is a granary or treasury on the map, and the "reach" objectives are the
//!   granary (Jieqiao) and forts (Julu, Huainan) their objective texts name.
//! * **AI** ([`ai_mode`]): `MAIN.EXE` maps each mode to an AI routine and names them (FORMATS
//!   §13.4): 0 대기 waits until an enemy can be reached (`defensive`), 1 최단 적공격 attacks the
//!   nearest enemy (`aggressive`), 2 부동 never moves (`hold`), 3 and 4 이동 head for an officer
//!   or a tile and fight on the way (`target`, `advance`), 5 and 6 무공격이동 head there without
//!   attacking (`march`).
//! * **Cast.** Where the base pack gives an original person's part to another officer (the bandit
//!   chiefs Liu Bei wins over), [`Pairing::roles`] names the officer who plays it.
//! * **Classes** follow the game's class order ([`crate::pack::CLASS_SPRITES`]); **items** match
//!   the base pack's items by name.

use crate::bakdata::{Item, Officer};
use crate::scenario::{BattleHeader, Block, Instr, Operands, Record, RosterUnit, Scene};
use hero_core::battledef::{
    AiMode, BattleDef, Condition, EventAction, EventDef, FlagCond, MapDef, Side, TreasureDef,
    Trigger, UnitSpawn,
};
use hero_core::geom::Pos;
use hero_core::script::Compare;
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

/// A base-pack battle and the original battle it follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pairing {
    /// Base-pack battle id.
    pub battle: &'static str,
    /// `SNRnD.R3` file number.
    pub file: usize,
    pub scene: usize,
    /// Battle map number (`HEXZMAP.R3` entry).
    pub map: u8,
    /// Scenario flags set on the way to the base battle (the others count as clear), for rosters
    /// the scenario picks with `if_flags`.
    pub flags: &'static [u8],
    /// `(BAKDATA person, base officer id)`: the base battle gives this person's part to another
    /// officer (its story, events and campaign flags name that officer).
    pub roles: &'static [(u16, &'static str)],
}

const fn pair(battle: &'static str, file: usize, scene: usize, map: u8) -> Pairing {
    Pairing {
        battle,
        file,
        scene,
        map,
        flags: &[],
        roles: &[],
    }
}

/// Flag 133: set when Liu Bei chooses the road through Julu (`SNR1D` scene 0 block 10); Jieqiao
/// then gets the enemy roster for that route.
const JULU_ROUTE: u8 = 133;

/// Base-pack battles and the original battle each one follows.
pub const ORIGINAL_BATTLES: &[Pairing] = &[
    pair("p1_sishui", 0, 0, 0),
    pair("p2_hulao", 0, 0, 1),
    pair("c1_guangchuan", 1, 0, 2),
    pair("c1_xindu", 1, 0, 3),
    pair("c1_qinghe", 1, 0, 5),
    pair("c1_julu", 1, 0, 4),
    // Jieqiao after Julu (a) and after Qinghe (b): one battle, two enemy rosters.
    Pairing {
        flags: &[JULU_ROUTE],
        ..pair("c1_jieqiao_a", 1, 0, 6)
    },
    pair("c1_jieqiao_b", 1, 0, 6),
    pair("c1_beihai", 1, 1, 7),
    pair("c1_xuzhou1", 1, 1, 8),
    pair("c1_xiaopei", 1, 1, 9),
    // The bandit chiefs Liu Bei wins over: the base pack casts Chang Xi, Xia Kun and Shi Meng in
    // the parts of the original's 이명 (375), 조하 (223) and 동량 (228).
    Pairing {
        roles: &[(375, "chang_xi")],
        ..pair("c1_taishan", 1, 2, 10)
    },
    Pairing {
        roles: &[(223, "xia_kun")],
        ..pair("c1_pengcheng1", 1, 2, 12)
    },
    Pairing {
        roles: &[(228, "shi_meng")],
        ..pair("c1_xiaqiu1", 1, 2, 11)
    },
    pair("c1_huainan", 1, 2, 13),
    pair("c1_xiaqiu2", 1, 3, 11),
    pair("c1_pengcheng2", 1, 3, 12),
    // With and without Gao Shun: the same battle.
    pair("c1_xiapi", 1, 3, 14),
    pair("c1_xiapi_b", 1, 3, 14),
    pair("c1_guangling", 1, 4, 15),
    pair("c1_xuzhou2", 1, 4, 8),
];

/// Upper nibble of a `load_map` value that loads a battle map.
pub(crate) const BATTLE_MAP: u16 = 0x3000;
/// Trigger kind `unit_at_cell`.
const UNIT_AT_CELL: u8 = 6;
/// Person value of trigger records meaning "any unit".
const ANY_UNIT: u16 = 0x400;
/// `BAKDATA` person of Liu Bei.
pub(crate) const LIU_BEI: u16 = 0;
/// Person value of setup slots that any deployed officer may take.
pub(crate) const ANY_OFFICER: u16 = 0x400;
/// `data` kinds: add gold / run the battle routine.
const DATA_GOLD: u16 = 2;
const DATA_ROUTINE: u16 = 4;

/// A trigger record for a unit on a tile (`unit_at_cell`) and what its script gives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellRecord {
    /// `BAKDATA` person, or [`ANY_UNIT`].
    pub person: u16,
    pub x: u8,
    pub y: u8,
    pub gold: u16,
    pub item: Option<u8>,
    /// The script runs the battle routine (`data` kind 4): an objective tile.
    pub routine: bool,
}

/// A trigger record whose script brings units onto the map (`join_battle`, `0x1A`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinRecord {
    /// Record number in the battle's block.
    pub record: usize,
    /// Trigger group: below [`FIRST_PHASE_GROUP`], the opening of the battle.
    pub group: u8,
    /// Trigger kind (0 = runs when its group's turn comes, right after the battle starts).
    pub kind: u8,
    pub inverted: bool,
    pub args: [u8; 6],
    /// `BAKDATA` persons it brings in.
    pub persons: Vec<u16>,
}

/// A battle of the original scenario.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OriginalBattle {
    /// Block of the scene that loads the battle map.
    pub block: usize,
    pub map: u8,
    pub header: BattleHeader,
    /// Player slots of `battle_setup`.
    pub player: Vec<RosterUnit>,
    /// Rosters loaded with the map for the route: `(friendly, units)`.
    pub rosters: Vec<(bool, Vec<RosterUnit>)>,
    /// Rosters loaded with the map for another route (skipped by `if_flags`).
    pub other_route_rosters: usize,
    /// The scenario flags the battle's setup, rosters and flagged slots depend on: the flags
    /// of the `if_flags` guards around them and the flags slots and units need
    /// (`requires_flag`). Another value of one of them may give another battle.
    pub route_flags: BTreeSet<u8>,
    /// Player slots left out because the flag they need is not set.
    pub flagged_slots_left_out: usize,
    /// Roster units left out because the flag they need is not set.
    pub flagged_units_left_out: Vec<u16>,
    /// Rosters loaded later in the battle (reinforcements; not converted).
    pub later_rosters: usize,
    pub cells: Vec<CellRecord>,
    pub joins: Vec<JoinRecord>,
    /// The trigger records of the battle's block.
    pub records: Vec<Record>,
}

/// Whether `block` starts a battle (`begin_battle`).
fn starts_battle(block: &Block) -> bool {
    block
        .records
        .iter()
        .any(|r| r.code.iter().any(|c| c.mnemonic == "begin_battle"))
}

/// The scenario flags `scene`'s battle blocks set (`set_flag` in a block that starts a battle or
/// loads a battle map): a battle's events may set them, whatever way the story took.
pub fn battle_set_flags(scene: &Scene) -> BTreeSet<u8> {
    scene
        .blocks
        .iter()
        .filter(|b| starts_battle(b) || loads_battle_map(b))
        .flat_map(|b| &b.records)
        .flat_map(|r| &r.code)
        .filter(|c| c.mnemonic == "set_flag")
        .filter_map(|c| c.operands.get("flag"))
        .map(|f| f as u8)
        .collect()
}

/// Whether `block` loads a battle map.
fn loads_battle_map(block: &Block) -> bool {
    block.records.iter().flat_map(|r| &r.code).any(|c| {
        c.mnemonic == "load_map"
            && c.operands
                .get("map")
                .is_some_and(|m| m & 0xf000 == BATTLE_MAP)
    })
}

/// Whether block `index` of `scene` only sets a battle up (map and rosters) and the block after
/// it fights it (starts it and holds its triggers, without loading a map of its own): SNR3's
/// Maicheng.
pub fn fought_in_next_block(scene: &Scene, index: usize) -> bool {
    let (Some(this), Some(next)) = (scene.blocks.get(index), scene.blocks.get(index + 1)) else {
        return false;
    };
    loads_battle_map(this) && !starts_battle(this) && starts_battle(next) && !loads_battle_map(next)
}

/// Record kinds a battle watches (FORMATS §13.2): contact, a unit on a cell, won, lost, a turn,
/// a unit in an area, a unit defeated.
const WATCHED: [u8; 7] = [4, 6, 7, 8, 9, 11, 12];

/// Whether block `index` of `scene` goes on with a battle that another block started: it loads
/// no map and has nothing but the battle's triggers (stages and `run` scripts), and a battle
/// block's script jumps to it (SNR3's Jiangling: at turn 8 the battle goes on in the next
/// block).
fn continues_battle(scene: &Scene, index: usize) -> bool {
    let Some(block) = scene.blocks.get(index) else {
        return false;
    };
    !loads_battle_map(block)
        && block
            .records
            .iter()
            .any(|r| WATCHED.contains(&r.trigger.kind))
        && block
            .records
            .iter()
            .all(|r| r.trigger.kind == RUN || WATCHED.contains(&r.trigger.kind))
}

/// The block of `scene` whose battle block `index` goes on with, if any
/// ([`continues_battle`]).
fn continuation(scene: &Scene, block: &Block, index: usize) -> Option<usize> {
    block
        .records
        .iter()
        .filter(|r| WATCHED.contains(&r.trigger.kind))
        .flat_map(|r| &r.code)
        .filter(|c| c.mnemonic == "goto_block")
        .filter_map(|c| c.operands.get("block").map(usize::from))
        .find(|&t| t != index && continues_battle(scene, t))
}

/// Whether block `index` of `scene` is part of a battle started in an earlier block: the block
/// after a setup ([`fought_in_next_block`]) or a battle's continuation ([`continues_battle`]).
pub fn part_of_earlier_battle(scene: &Scene, index: usize) -> bool {
    (index > 0 && fought_in_next_block(scene, index - 1))
        || (0..index).any(|b| {
            loads_battle_map(&scene.blocks[b])
                && continuation(scene, &setup_and_battle(scene, b), b) == Some(index)
        })
}

/// The flag a battle sets as it goes on in its continuation block ([`continues_battle`]): a
/// scenario flag no script of the original uses (they go up to 219; the pack's conversion checks
/// that none uses it and that one battle at most needs it). The story after the battle tells by
/// it whether the battle got that far (its outro is the continuation's).
pub const CONTINUATION_FLAG: u8 = 255;

/// [`CONTINUATION_FLAG`] (for a battle of `scene`).
pub fn continuation_flag(_scene: &Scene) -> u8 {
    CONTINUATION_FLAG
}

/// The scenario flags `scene`'s scripts set or test.
pub fn flags_used(scene: &Scene) -> BTreeSet<u8> {
    scene
        .instructions()
        .flat_map(|c| match &c.operands {
            Operands::Condition {
                all_set, all_clear, ..
            } => all_set.iter().chain(all_clear).copied().collect::<Vec<_>>(),
            _ if c.mnemonic == "set_flag" => c
                .operands
                .get("flag")
                .map(|f| f as u8)
                .into_iter()
                .collect(),
            _ => Vec::new(),
        })
        .collect()
}

/// Whether battle block `index` of `scene` goes on in a continuation block
/// ([`continues_battle`]).
pub fn has_continuation(scene: &Scene, index: usize) -> bool {
    continuation(scene, &setup_and_battle(scene, index), index).is_some()
}

/// Battle block `index` of `scene`, joined with the block after it when that one fights it
/// ([`fought_in_next_block`]; its groups one up, the setup block holding group 0).
fn setup_and_battle(scene: &Scene, index: usize) -> Cow<'_, Block> {
    let block = &scene.blocks[index];
    if !fought_in_next_block(scene, index) {
        return Cow::Borrowed(block);
    }
    let mut joined = block.clone();
    joined.records.extend(
        scene.blocks[index + 1]
            .records
            .iter()
            .cloned()
            .map(|mut r| {
                r.trigger.group += 1;
                r
            }),
    );
    Cow::Owned(joined)
}

/// The battle block `index` of `scene` with its triggers: the block itself, or, when the block
/// after fights it ([`fought_in_next_block`]), the two joined, the second's groups one up (the
/// setup block holds group 0), as one battle block; and a block its battle goes on with
/// ([`continues_battle`]) after it, its groups after the battle's, the jump there moving the
/// battle on to them (`leave_parallel`).
pub fn battle_block(scene: &Scene, index: usize) -> Cow<'_, Block> {
    let mut joined = setup_and_battle(scene, index);
    if let Some(next) = continuation(scene, &joined, index) {
        let last = joined
            .records
            .iter()
            .map(|r| r.trigger.group)
            .max()
            .unwrap_or(0);
        let flag = continuation_flag(scene);
        let mut owned = joined.into_owned();
        for r in &mut owned.records {
            let mut code = Vec::with_capacity(r.code.len() + 1);
            for c in r.code.drain(..) {
                if c.mnemonic == "goto_block" && c.operands.get("block") == Some(next as u16) {
                    // It sets the continuation's flag and moves the battle on.
                    code.push(Instr {
                        offset: c.offset,
                        opcode: 0x14,
                        mnemonic: "set_flag",
                        operands: Operands::Fields {
                            args: vec![
                                crate::scenario::Arg {
                                    name: "flag",
                                    kind: crate::scenario::ArgKind::Flag,
                                    value: u16::from(flag),
                                },
                                crate::scenario::Arg {
                                    name: "clear",
                                    kind: crate::scenario::ArgKind::Number,
                                    value: 0,
                                },
                            ],
                        },
                    });
                    code.push(Instr {
                        offset: c.offset,
                        opcode: 0x13,
                        mnemonic: "leave_parallel",
                        operands: Operands::Fields { args: Vec::new() },
                    });
                } else {
                    code.push(c);
                }
            }
            r.code = code;
        }
        owned
            .records
            .extend(scene.blocks[next].records.iter().cloned().map(|mut r| {
                r.trigger.group += last + 1;
                r
            }));
        joined = Cow::Owned(owned);
    }
    joined
}

/// Where a battle block goes on with another battle map: the battle's script sets the next
/// battle up (`battle_setup`, `battle_roster`) and ends this one (`battle_end` to a battle map),
/// and the battle that follows starts right after (SNR2's Changban: the people cross the
/// first map, then the second is fought). Its groups are those of a battle block of their own,
/// from the setup record on: the setup and roster, the start, the opening, the phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapLeg {
    /// The record of the battle block that sets the next map's battle up.
    pub record: usize,
    /// The battle map the battle goes on with.
    pub next_map: u8,
}

/// [`MapLeg`] of the battle block `block`, if it has one: a `run` record of a phase group that
/// ends the battle for another battle map, followed by the next battle's start.
fn map_leg(block: &Block) -> Option<MapLeg> {
    block.records.iter().enumerate().find_map(|(i, r)| {
        let next_map = r
            .code
            .iter()
            .filter(|c| c.mnemonic == "battle_end")
            .filter_map(|c| c.operands.get("next_map"))
            .find(|m| m & 0xf000 == BATTLE_MAP)?;
        let follows = block.records.get(i + 1).is_some_and(|next| {
            next.trigger.group == r.trigger.group + 1
                && next.code.iter().any(|c| c.mnemonic == "begin_battle")
        });
        (r.trigger.kind == RUN && r.trigger.group >= FIRST_PHASE_GROUP && follows).then_some(
            MapLeg {
                record: i,
                next_map: (next_map & 0xff) as u8,
            },
        )
    })
}

/// [`MapLeg`] of battle block `index` of `scene` (see [`battle_block`]).
pub fn battle_map_leg(scene: &Scene, index: usize) -> Option<MapLeg> {
    map_leg(&battle_block(scene, index))
}

/// Leg `leg` of the battle of block `index` of `scene`: 0 is the battle up to where it goes on
/// with another map ([`MapLeg`]), 1 is the battle on that map, as a battle block of its own
/// (groups counted from the setup record; the `battle_end` that leads there left out). A battle
/// without a second map has one leg, [`battle_block`].
pub fn battle_leg(scene: &Scene, index: usize, leg: u8) -> Cow<'_, Block> {
    let whole = battle_block(scene, index);
    let Some(split) = map_leg(&whole) else {
        return whole;
    };
    let mut block = Block {
        offset: whole.offset,
        records: Vec::new(),
    };
    if leg == 0 {
        block.records = whole.records[..split.record].to_vec();
    } else {
        let first = whole.records[split.record].trigger.group;
        block.records = whole.records[split.record..]
            .iter()
            .cloned()
            .map(|mut r| {
                r.trigger.group = r.trigger.group.saturating_sub(first);
                r.code.retain(|c| {
                    !(c.mnemonic == "battle_end"
                        && c.operands
                            .get("next_map")
                            .is_some_and(|m| m & 0xf000 == BATTLE_MAP))
                });
                r
            })
            .collect();
    }
    Cow::Owned(block)
}

/// Each instruction of `code` with whether it runs when the scenario flags `flags` are set
/// (every `if_flags` guard before it that still covers it holds) and the flags those guards
/// test.
fn guarded<'a>(code: &'a [Instr], flags: &[u8]) -> Vec<(&'a Instr, bool, BTreeSet<u8>)> {
    // (instructions still guarded, the condition holds, the flags it tests)
    let mut guards: Vec<(u8, bool, Vec<u8>)> = Vec::new();
    let mut out = Vec::new();
    for instr in code {
        let runs = guards.iter().all(|g| g.1);
        let tested = guards.iter().flat_map(|g| g.2.iter().copied()).collect();
        for g in guards.iter_mut() {
            g.0 -= 1;
        }
        guards.retain(|g| g.0 > 0);
        if let Operands::Condition {
            skip,
            all_set,
            all_clear,
        } = &instr.operands
        {
            if *skip > 0 {
                let holds = all_set.iter().all(|f| flags.contains(f))
                    && all_clear.iter().all(|f| !flags.contains(f));
                let tests = all_set.iter().chain(all_clear).copied().collect();
                guards.push((*skip, holds, tests));
            }
        }
        out.push((instr, runs, tested));
    }
    out
}

/// Find the battle of `scene` fought on battle map `map` with the scenario flags `flags` set (see
/// the module docs for which `battle_setup` and rosters belong to it): the first block that
/// loads the map, or block `only` (a map fought in several blocks, one per route).
pub fn find_battle(
    scene: &Scene,
    map: u8,
    flags: &[u8],
    only: Option<usize>,
) -> Result<OriginalBattle, String> {
    find_battle_leg(scene, map, flags, only, 0)
}

/// [`find_battle`] for leg `leg` of the battle ([`battle_leg`]): leg 1 is the battle block
/// `only` goes on with (on `map`) after the map it loaded.
pub fn find_battle_leg(
    scene: &Scene,
    map: u8,
    flags: &[u8],
    only: Option<usize>,
    leg: u8,
) -> Result<OriginalBattle, String> {
    let wanted = BATTLE_MAP | u16::from(map);
    let is_load = |op: &Operands| op.get("map") == Some(wanted);
    let (block_index, record_index) = if leg == 0 {
        scene
            .blocks
            .iter()
            .enumerate()
            .filter(|(b, _)| only.is_none_or(|o| o == *b))
            .find_map(|(b, block)| {
                block.records.iter().enumerate().find_map(|(r, rec)| {
                    rec.code
                        .iter()
                        .any(|i| i.mnemonic == "load_map" && is_load(&i.operands))
                        .then_some((b, r))
                })
            })
            .ok_or_else(|| format!("no block loads battle map {map}"))?
    } else {
        // The battle block goes on with the map (its second leg starts at the first record).
        let b = only
            .filter(|&b| battle_map_leg(scene, b).is_some_and(|l| l.next_map == map))
            .ok_or_else(|| format!("no battle goes on with battle map {map}"))?;
        (b, 0)
    };
    let block = &*battle_leg(scene, block_index, leg);
    let split = battle_map_leg(scene, block_index).is_some();

    let (mut rosters, mut later_rosters, mut other_route_rosters) = (Vec::new(), 0, 0);
    let mut route_flags: BTreeSet<u8> = BTreeSet::new();
    for (r, rec) in block.records.iter().enumerate() {
        for (instr, runs, tested) in guarded(&rec.code, flags) {
            match &instr.operands {
                Operands::Roster { friendly, units } if r == record_index => {
                    route_flags.extend(tested);
                    route_flags.extend(units.iter().filter_map(|u| u.requires_flag));
                    if runs {
                        rosters.push((*friendly, units.clone()));
                    } else {
                        other_route_rosters += 1;
                    }
                }
                Operands::Roster { .. } => later_rosters += 1,
                _ => {}
            }
        }
    }
    if rosters.is_empty() {
        return Err(format!(
            "block {block_index} loads battle map {map} without a roster"
        ));
    }
    let enemies: BTreeSet<u16> = rosters
        .iter()
        .filter(|(friendly, _)| !friendly)
        .flat_map(|(_, units)| units.iter().map(|u| u.person))
        .collect();

    // The setups before the battle: those of the earlier blocks and its own up to the record
    // that loads its map (a later record's setup is for the battle after it, as Xuchang's
    // after its victory). (A battle with more than one map has the setup of its later leg in
    // its own block, past the record that ends the first: the leg's block holds only its own
    // records.)
    let own: &[Record] = if split {
        &block.records
    } else {
        &scene.blocks[block_index].records[..=record_index]
    };
    // A setup an `if_flags` guard skips for `flags` is another route's.
    let mut setups: Vec<(&BattleHeader, &Vec<RosterUnit>)> = Vec::new();
    // Whether some route's setup names one of the battle's enemies to beat.
    let mut fits_a_route = false;
    // The last block before the battle (or its own) with a setup that fits it on some route
    // (names one of its enemies to beat), and whether such a setup runs on this route.
    let mut last_fitting: Option<(usize, bool)> = None;
    for (b, rec) in scene.blocks[..block_index]
        .iter()
        .enumerate()
        .flat_map(|(b, block)| block.records.iter().map(move |r| (b, r)))
        .chain(own.iter().map(|r| (block_index, r)))
    {
        for (instr, runs, tested) in guarded(&rec.code, flags) {
            if let Operands::BattleSetup { header, units } = &instr.operands {
                route_flags.extend(tested);
                route_flags.extend(units.iter().filter_map(|u| u.requires_flag));
                let fits = header.defeat_to_win.is_some_and(|p| enemies.contains(&p));
                if fits {
                    last_fitting = match last_fitting {
                        Some((at, ran)) if at == b => Some((b, ran || runs)),
                        _ => Some((b, runs)),
                    };
                }
                fits_a_route |= fits;
                if runs {
                    setups.push((header, units));
                }
            }
        }
    }
    // A route that skips the battle's setups of the last block that has one does not fight it
    // (the story fights another there: Xuchang's town sets up another battle on flag 89).
    if let Some((b, false)) = last_fitting {
        return Err(format!(
            "block {block_index}: the route skips the battle's setups in block {b}"
        ));
    }
    // The latest that names an enemy to beat, else (when no route's does: a battle won some
    // other way) the latest without one. A route whose setups do not fit is not this battle's.
    let (header, player) = setups
        .iter()
        .rev()
        .find(|(h, _)| h.defeat_to_win.is_some_and(|p| enemies.contains(&p)))
        .or_else(|| {
            (!fits_a_route)
                .then(|| setups.iter().rev().find(|(h, _)| h.defeat_to_win.is_none()))
                .flatten()
        })
        .ok_or_else(|| {
            format!("no battle_setup before block {block_index} fits battle map {map}")
        })?;

    let cells = block
        .records
        .iter()
        .filter(|r| r.trigger.kind == UNIT_AT_CELL && !r.trigger.inverted)
        .map(|r| {
            let mut cell = CellRecord {
                person: r.trigger.word(0),
                // Row, then column.
                x: r.trigger.args[3],
                y: r.trigger.args[2],
                gold: 0,
                item: None,
                routine: false,
            };
            for instr in &r.code {
                match instr.mnemonic {
                    "data" => match (instr.operands.get("kind"), instr.operands.get("value")) {
                        (Some(DATA_GOLD), Some(v)) => cell.gold = cell.gold.saturating_add(v),
                        (Some(DATA_ROUTINE), _) => cell.routine = true,
                        _ => {}
                    },
                    "add_item" => cell.item = instr.operands.get("item").map(|v| v as u8),
                    _ => {}
                }
            }
            cell
        })
        .collect();

    let joins = block
        .records
        .iter()
        .enumerate()
        .filter_map(|(i, r)| {
            let persons: Vec<u16> = r
                .code
                .iter()
                .filter(|c| c.mnemonic == "join_battle")
                .filter_map(|c| c.operands.get("person"))
                .collect();
            (!persons.is_empty()).then_some(JoinRecord {
                record: i,
                group: r.trigger.group,
                kind: r.trigger.kind,
                inverted: r.trigger.inverted,
                args: r.trigger.args,
                persons,
            })
        })
        .collect();

    // Slots and units that need a flag (officers who join for this battle only, when talked
    // to): there when it is set.
    let mut player: Vec<RosterUnit> = (*player).clone();
    let mut flagged_slots_left_out = 0;
    player.retain_mut(|u| match u.requires_flag.take() {
        None => true,
        Some(f) => {
            route_flags.insert(f);
            let kept = flags.contains(&f);
            flagged_slots_left_out += usize::from(!kept);
            kept
        }
    });
    let mut flagged_units_left_out = Vec::new();
    for (_, units) in &mut rosters {
        units.retain_mut(|u| match u.requires_flag.take() {
            None => true,
            Some(f) => {
                route_flags.insert(f);
                let kept = flags.contains(&f);
                if !kept {
                    flagged_units_left_out.push(u.person);
                }
                kept
            }
        });
    }

    Ok(OriginalBattle {
        block: block_index,
        map,
        header: (*header).clone(),
        player,
        rosters,
        other_route_rosters,
        route_flags,
        flagged_slots_left_out,
        flagged_units_left_out,
        later_rosters,
        cells,
        joins,
        records: block.records.clone(),
    })
}

/// The engine AI of an original AI mode and parameter: `(mode, target person, tile)`.
pub fn ai_mode(mode: u8, param: u16) -> (AiMode, Option<u16>, Option<Pos>) {
    let tile = || {
        Some(Pos::new(
            i32::from(param as u8),
            i32::from((param >> 8) as u8),
        ))
    };
    match mode {
        1 => (AiMode::Aggressive, None, None),
        2 => (AiMode::Hold, None, None),
        3 => (AiMode::Target, Some(param), None),
        4 => (AiMode::Advance, None, tile()),
        5 => (AiMode::March, Some(param), None),
        6 => (AiMode::March, None, tile()),
        _ => (AiMode::Defensive, None, None),
    }
}

/// The AI of a unit whose target officer could not be resolved: `target` attacks the nearest
/// enemy instead, `march` (which never attacks) stays where it is.
fn without_target(ai: AiMode) -> AiMode {
    match ai {
        AiMode::March => AiMode::Hold,
        _ => AiMode::Aggressive,
    }
}

fn fallback_note(ai: AiMode) -> &'static str {
    match ai {
        AiMode::Hold => "holds its ground instead",
        _ => "attacks instead",
    }
}

/// The engine trigger of an original trigger record, with `unit` naming a person (`None` for
/// [`ANY_UNIT`], any player unit). Kinds: 9 turn, 6 unit on a tile, 11 unit in a rectangle, 12 unit
/// defeated, 4 two units next to each other; tiles of trigger records are row first.
pub fn trigger_of(
    kind: u8,
    inverted: bool,
    args: [u8; 6],
    unit: &mut dyn FnMut(u16) -> Result<Option<String>, String>,
) -> Result<Trigger, String> {
    if inverted {
        return Err(format!("inverted trigger kind {kind} is not converted"));
    }
    let word = |i: usize| u16::from_le_bytes([args[2 * i], args[2 * i + 1]]);
    let tile = |row: usize| Pos::new(i32::from(args[row + 1]), i32::from(args[row]));
    let named = |p: u16, unit: &mut dyn FnMut(u16) -> Result<Option<String>, String>| {
        unit(p)?.ok_or_else(|| format!("trigger kind {kind} needs a unit, not any unit"))
    };
    Ok(match kind {
        9 => Trigger::TurnStart {
            turn: u32::from(word(0)),
            side: Side::Player,
        },
        6 => Trigger::Reach {
            who: unit(word(0))?,
            pos: tile(2),
            radius: 0,
            to: None,
        },
        11 => Trigger::Reach {
            who: unit(word(0))?,
            pos: tile(2),
            radius: 0,
            to: Some(tile(4)),
        },
        12 => Trigger::UnitDefeated {
            target: named(word(0), unit)?,
        },
        4 => Trigger::Adjacent {
            a: unit(word(0))?,
            b: named(word(1), unit)?,
        },
        other => return Err(format!("trigger kind {other} is not converted")),
    })
}

/// What the converter knows about the pack chain and the release.
#[derive(Debug, Clone, Default)]
pub struct Names {
    /// `BAKDATA` person → base-pack officer id.
    pub officers: BTreeMap<u16, String>,
    /// `BAKDATA` person → name (for generic units).
    pub person_names: BTreeMap<u16, String>,
    /// `BAKDATA` person → 무력·지력·통솔 in the order of [`UnitSpawn::stats`] (`[str, int, lead]`,
    /// clamped to 0–100 as the loader does): a generic unit of the original fights with its
    /// person's stats, not the class's generic ones (every unit of the original is a person).
    pub stats: BTreeMap<u16, [i32; 3]>,
    /// Original class number → base-pack class id.
    pub classes: BTreeMap<u8, String>,
    /// Original item number → base-pack item id.
    pub items: BTreeMap<u8, String>,
    /// Officers of the player's army in the pack chain (the campaign's starting officers and
    /// those that join in its scenes): events may name them in any battle.
    pub player_officers: BTreeSet<String>,
    /// The original's civilians (`BAKDATA` persons of the civilian class that no base-pack
    /// officer plays: the people of Changban): person → the civilian class of the pack and the
    /// person's level. A setup slot naming one is a fixed allied unit, not a place to deploy.
    pub civilians: BTreeMap<u16, (String, u32)>,
}

impl Names {
    /// Build the lookups. `officer_of` gives the `BAKDATA` records a base officer matches.
    pub fn new(
        people: &[Officer],
        items: &[Item],
        officer_of: impl Fn(&Officer) -> Option<String>,
        class_sprites: &[&str],
        base_classes: &[(String, String)],
        base_items: &[(String, String)],
    ) -> Names {
        let officers = people
            .iter()
            .filter_map(|o| Some((o.index as u16, officer_of(o)?)))
            .collect();
        let person_names = people
            .iter()
            .map(|o| (o.index as u16, o.name.clone()))
            .collect();
        let stat = |v: u8| i32::from(v.min(100));
        let stats = people
            .iter()
            .map(|o| {
                (
                    o.index as u16,
                    [stat(o.war), stat(o.intelligence), stat(o.leadership)],
                )
            })
            .collect();
        let classes: BTreeMap<u8, String> = class_sprites
            .iter()
            .enumerate()
            .filter_map(|(i, sprite)| {
                let (id, _) = base_classes.iter().find(|(_, s)| s == sprite)?;
                Some((i as u8, id.clone()))
            })
            .collect();
        let items = items
            .iter()
            .filter_map(|it| {
                let mut ids = base_items.iter().filter(|(_, name)| *name == it.name);
                match (ids.next(), ids.next()) {
                    (Some((id, _)), None) => Some((it.index as u8, id.clone())),
                    _ => None,
                }
            })
            .collect();
        let civilians = class_sprites
            .iter()
            .position(|sprite| *sprite == "civilian")
            .and_then(|number| {
                let class = classes.get(&(number as u8))?;
                Some(
                    people
                        .iter()
                        .filter(|o| usize::from(o.class) == number && officer_of(o).is_none())
                        .map(|o| (o.index as u16, (class.clone(), u32::from(o.level))))
                        .collect(),
                )
            })
            .unwrap_or_default();
        Names {
            officers,
            person_names,
            stats,
            classes,
            items,
            player_officers: BTreeSet::new(),
            civilians,
        }
    }

    fn person_label(&self, person: u16) -> String {
        match self.person_names.get(&person) {
            Some(name) => format!("{name} ({person})"),
            None => format!("person {person}"),
        }
    }
}

/// A converted battle and what did not carry over.
#[derive(Debug, Clone)]
pub struct Converted {
    pub battle: BattleDef,
    /// Drama scenes of the original's mid-battle events, as `.drama` text (empty without any).
    pub drama: String,
    pub notes: Vec<String>,
    /// Officers a chapter's battle moves in or out of Liu Bei's army (`set_country`): the
    /// officer and whether they join. The battle sets [`army_flag`]; the campaign acts on it
    /// after the battle.
    pub army: Vec<(String, bool)>,
}

/// The campaign flag a chapter's battle sets when `officer` joins Liu Bei's army (`joins`) or
/// leaves it (`set_country`).
pub fn army_flag(officer: &str, joins: bool) -> String {
    if joins {
        format!("orig_join_{officer}")
    } else {
        format!("orig_away_{officer}")
    }
}

/// Whether a trigger record is an objective of Liu Bei's: he stands on a tile or in an area and
/// the script runs the battle routine (`data` kind 4).
fn is_routine(r: &Record) -> bool {
    !r.trigger.inverted
        && matches!(r.trigger.kind, UNIT_AT_CELL | UNIT_IN_AREA)
        && r.trigger.word(0) == LIU_BEI
        && r.code
            .iter()
            .any(|c| c.mnemonic == "data" && c.operands.get("kind") == Some(DATA_ROUTINE))
}

/// Whether `r` is a treasure: any unit on a tile (the trigger the treasure list reads), a script
/// that gives gold or an item. Any other script on a tile is an event (Xuchang 2's wall: whoever
/// stands there brings Huang Zhong and Yan Yan in; camps to capture; a bridge let down).
fn is_treasure(r: &Record) -> bool {
    r.trigger.kind == UNIT_AT_CELL
        && r.trigger.word(0) == ANY_UNIT
        && !r.trigger.inverted
        && r.code.iter().any(|c| {
            c.mnemonic == "add_item"
                || (c.mnemonic == "data" && c.operands.get("kind") == Some(DATA_GOLD))
        })
}

/// Whether a record of the battle becomes an event that can end the battle by its script, when
/// the battle's last stage has a victory script ([`ended_flag`]). In the last stage that is a
/// record that leaves the phase (`leave_parallel` or the end of the battle: the records of a
/// group that is not watched in parallel end it by running); in an earlier stage one that ends
/// the battle itself (`battle_end`, `goto_block`: Tucai's four forts taken), which the later
/// stage's victory script never follows. The objective of Liu Bei of a single-stage battle is the
/// battle's victory condition instead, and treasures are no events.
pub fn events_end_battle(records: &[Record]) -> bool {
    let phases = phases(records);
    let Some(last) = phases.iter().rposition(|p| !p.runs_only) else {
        return false;
    };
    let first = phases.iter().position(|p| !p.runs_only);
    let single = phases.iter().filter(|p| !p.runs_only).count() == 1;
    let phase = &phases[last];
    let has_victory_script = phase
        .records
        .iter()
        .any(|&r| records[r].trigger.kind == BATTLE_WON);
    let event = |r: &Record| {
        !matches!(r.trigger.kind, BATTLE_WON | BATTLE_LOST)
            && !is_treasure(r)
            && !(single && is_routine(r))
    };
    let ends_itself = |r: &Record| {
        r.code
            .iter()
            .any(|c| matches!(c.mnemonic, "battle_end" | "goto_block"))
    };
    has_victory_script
        && (phase.records.iter().map(|&r| &records[r]).any(|r| {
            event(r)
                && (!phase.parallel
                    || r.code.iter().any(|c| {
                        matches!(c.mnemonic, "leave_parallel" | "battle_end" | "goto_block")
                    }))
        }) || phases[..last]
            .iter()
            .enumerate()
            .filter(|(_, p)| !p.runs_only)
            .flat_map(|(i, p)| p.records.iter().map(move |&r| (i, &records[r])))
            .any(|(i, r)| {
                // (Liu Bei's objective of the first stage is the battle's victory condition.)
                event(r) && ends_itself(r) && !(Some(i) == first && is_routine(r))
            }))
}

/// The campaign flag an event of a chapter's battle sets when it ends the battle (the original
/// leaves the phase by that record's script, and the phase's victory script does not run): the
/// battle's outro plays the victory script only while it is clear.
pub fn ended_flag(battle: &str) -> String {
    format!("orig_{battle}_ended")
}

/// The text section of a scene (`SNRnM`), decoded.
pub trait TextSource {
    /// Lines `(speaker person, text)` of the dialogue at `offset`.
    fn dialogue(&self, offset: u16) -> Result<Vec<(u16, String)>, String>;
    /// The string at `offset`.
    fn string(&self, offset: u16) -> Result<String, String>;
}

/// A cell after a map-cell operation (`set_map_chip`): its new terrain id and the key of its tile
/// picture, or `None` when the operation leaves the cell alone.
pub type CellChange = Option<(String, Option<String>)>;

/// What the event conversion needs beyond the scenario block.
pub struct EventSources<'a> {
    /// The text of the battle's scene.
    pub text: &'a dyn TextSource,
    /// The cell at a tile after a `set_map_chip` operation.
    pub cell_change: &'a mut dyn FnMut(Pos, u8) -> Result<CellChange, String>,
}

/// First trigger group of a battle block that plays during the battle: group 0 loads the map,
/// 1 starts the battle, 2 is the opening.
pub const FIRST_PHASE_GROUP: u8 = 3;
/// The trigger group of a battle block that opens the battle: the first lines, the AI it starts
/// with, those who are on the field from the start.
pub const OPENING_GROUP: u8 = 2;
/// Trigger kinds: runs when its group's turn comes / the battle is won / lost / unit in an area.
const RUN: u8 = 0;
const BATTLE_WON: u8 = 7;
const BATTLE_LOST: u8 = 8;
const UNIT_IN_AREA: u8 = 11;
/// `@duel` background of the converted duels: the terrain under the fighters, as the original
/// picks it (`pack::duel_pictures` writes one per terrain).
pub const DUEL_BACKGROUND: &str = hero_core::script::DUEL_TERRAIN;

/// The `@duel_act` moves of a `duel_action` code (MAIN.EXE's duel routine, FORMATS §13.6): 0, 1,
/// 6 and 7 charge and strike (attack frames 4, 6, 4, 6), 2 strikes with frames 8, 3 falls, 4
/// flees, 5 waits (no move), 8 is an effect taken for a strike with frames 10 [inferred], 9 rides
/// back and 10 charges. `None`: an unknown code.
fn duel_moves(action: u16) -> Option<&'static [&'static str]> {
    Some(match action {
        0 | 6 => &["charge", "strike 4"],
        1 | 7 => &["charge", "strike 6"],
        2 => &["strike 8"],
        3 => &["fall"],
        4 => &["flee"],
        5 => &[],
        8 => &["strike 10"],
        9 => &["back"],
        10 => &["charge"],
        _ => return None,
    })
}

/// A trigger group of a battle block from [`FIRST_PHASE_GROUP`] on (FORMATS §13.2).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Phase {
    /// The group flag of its first record: all records are watched until one leaves parallel
    /// control; otherwise the first record whose trigger holds runs, then the next phase follows.
    parallel: bool,
    records: Vec<usize>,
    /// Only `run` records: the script after the battle (or between two phases).
    runs_only: bool,
    /// One of its scripts ends the battle (`battle_end`).
    ends_battle: bool,
}

/// The phases of a battle block, in order.
fn phases(records: &[Record]) -> Vec<Phase> {
    let mut out: Vec<(u8, Phase)> = Vec::new();
    for (i, r) in records.iter().enumerate() {
        let group = r.trigger.group;
        if group < FIRST_PHASE_GROUP {
            continue;
        }
        if out.last().is_none_or(|(g, _)| *g != group) {
            out.push((
                group,
                Phase {
                    parallel: r.trigger.group_flag,
                    records: Vec::new(),
                    runs_only: true,
                    ends_battle: false,
                },
            ));
        }
        let phase = &mut out.last_mut().expect("pushed above").1;
        phase.records.push(i);
        phase.runs_only &= r.trigger.kind == RUN;
        phase.ends_battle |= r.code.iter().any(|c| c.mnemonic == "battle_end");
    }
    out.into_iter().map(|(_, p)| p).collect()
}

/// What follows the end of phase `i`: victory, or the stage of the next phase to watch with the
/// `run` records to play on the way.
enum Next {
    Victory,
    Stage(u32, Vec<usize>),
}

fn next_after(phases: &[Phase], stages: &[Option<u32>], i: usize) -> Next {
    let mut on_the_way = Vec::new();
    for (j, p) in phases.iter().enumerate().skip(i + 1) {
        if !p.runs_only {
            return Next::Stage(stages[j].expect("watched phases have a stage"), on_the_way);
        }
        if p.ends_battle {
            return Next::Victory;
        }
        on_the_way.extend(&p.records);
    }
    Next::Victory
}

/// Whether two triggers fire on the same occasion: the same turn (either side's phase), the same
/// pair of adjacent units, the same unit defeated or the same area. Unit references must be
/// canonical ([`EventWriter::canonical`]).
fn same_occasion(a: &Trigger, b: &Trigger) -> bool {
    match (a, b) {
        (Trigger::TurnStart { turn: x, .. }, Trigger::TurnStart { turn: y, .. }) => x == y,
        (Trigger::Adjacent { a: a1, b: b1 }, Trigger::Adjacent { a: a2, b: b2 }) => {
            (a1 == a2 && b1 == b2) || (a1.as_ref() == Some(b2) && a2.as_ref() == Some(b1))
        }
        _ => a == b,
    }
}

/// Every unit reference of a condition.
fn condition_refs(c: &Condition) -> Vec<&str> {
    match c {
        Condition::DefeatUnit { target } | Condition::UnitRetreated { target } => vec![target],
        Condition::Reach { who, .. } => who.iter().map(String::as_str).collect(),
        Condition::DefeatAll | Condition::DefeatCommander | Condition::SurviveTurns { .. } => {
            Vec::new()
        }
    }
}

/// Why an event cannot follow onto the original map, if it cannot.
fn event_problem(e: &EventDef, gone: &BTreeSet<String>) -> Option<String> {
    let mut refs: Vec<&str> = Vec::new();
    match &e.trigger {
        Trigger::Reach { .. } => return Some("it fires on a tile of the base map".into()),
        Trigger::UnitDefeated { target } | Trigger::HpBelow { target, .. } => refs.push(target),
        Trigger::Adjacent { a, b } => refs.extend(a.iter().map(String::as_str).chain([b.as_str()])),
        Trigger::TurnStart { .. } => {}
    }
    for action in &e.actions {
        match action {
            EventAction::Spawn { group } => {
                return Some(format!(
                    "it brings in reinforcement group `{group}` of the base map"
                ))
            }
            EventAction::SetAi {
                ai_pos: Some(_), ..
            } => return Some("it sends a unit to a tile of the base map".into()),
            EventAction::SetTerrain { .. } => {
                return Some("it changes a tile of the base map".into())
            }
            EventAction::SetAi {
                target, ai_target, ..
            } => {
                refs.push(target);
                refs.extend(ai_target.iter().map(String::as_str));
            }
            EventAction::Retreat { target } | EventAction::LevelUp { target, .. } => {
                refs.push(target)
            }
            _ => {}
        }
    }
    refs.into_iter()
        .find(|r| gone.contains(*r))
        .map(|r| format!("it names `{r}`, who is not in the original roster"))
}

/// A drama speaker without an officer id: the name as the game shows it, without spaces
/// (drama speakers are one word of at most 24 characters, and one that looks like an id must be
/// an officer, which a free name is not).
pub(crate) fn free_speaker(name: &str) -> String {
    let name: String = name
        .chars()
        .filter(|c| !c.is_whitespace() && *c != ':')
        .take(24)
        .collect();
    let id_like = name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if name.is_empty() || id_like {
        "???".to_string()
    } else {
        name
    }
}

/// The ids of the scenes of a drama (its `== id` lines).
///
/// Input: drama text as the converter writes it. Output: each scene id, in order.
pub(crate) fn scene_ids(drama: &str) -> impl Iterator<Item = &str> {
    drama
        .lines()
        .filter_map(|l| l.strip_prefix("== "))
        .map(str::trim)
}

/// Remove scene `id` (its `== id` line and the lines up to the next scene) from a drama.
pub(crate) fn remove_scene(drama: &mut String, id: &str) {
    let head = format!("\n== {id}\n");
    let Some(start) = drama.find(&head) else {
        return;
    };
    let end = drama[start + head.len()..]
        .find("\n== ")
        .map_or(drama.len(), |i| start + head.len() + i);
    drama.replace_range(start..end, "");
}

/// Append `text` to a drama as `head` (`speaker:` or `@narr`) and indented continuation lines;
/// a line the drama parser would read as something else starts a new `head` line instead.
pub(crate) fn push_text(out: &mut String, head: &str, text: &str) {
    let mut lines = text
        .split('\n')
        .map(|l| l.trim_end_matches('\r').trim())
        .filter(|l| !l.is_empty());
    let Some(first) = lines.next() else {
        return;
    };
    let _ = writeln!(out, "{head} {first}");
    for line in lines {
        if line.starts_with(['@', '-', '#']) || line.starts_with("==") {
            let _ = writeln!(out, "{head} {line}");
        } else {
            let _ = writeln!(out, "    {line}");
        }
    }
}

/// How a record's script ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScriptEnd {
    /// Ran to its end.
    Done,
    /// `leave_parallel`: the phase is over.
    LeavesPhase,
    /// `battle_end` (the battle is won) or `goto_block` (the scenario moves on).
    EndsBattle,
    /// A part guarded by flags other records set ended the script ([`Branch`]); what follows
    /// it runs only while the flags do not hold.
    Branched,
}

/// A part of a record's script that runs only while flags other records set hold (an `if_flags`
/// on shared flags): it becomes an event of its own with the record's trigger and these
/// conditions.
#[derive(Debug, Clone, PartialEq)]
struct Branch {
    when: Vec<FlagCond>,
    /// The part runs only while not all of these hold (what a script does past a guarded part
    /// that ends it).
    unless: Vec<FlagCond>,
    actions: Vec<EventAction>,
    end: ScriptEnd,
    /// Where the script reached the part: the number of the record's own actions before it.
    /// `None` for a part that is not at the top level of the script (the rest past a guarded
    /// part, a test inside a guarded part).
    at: Option<usize>,
}

/// The converter of one battle's records into events and drama scenes.
struct EventWriter<'a, 'b> {
    battle_id: &'a str,
    /// A battle of the original's chapters: its flags are the campaign's
    /// (`orig_f<n>`), which the chapter's story reads after it.
    chapter: bool,
    names: &'a Names,
    roles: &'a [(u16, &'a str)],
    /// Scenario flags set for the route (`if_flags` holds for them).
    route: &'a [u8],
    /// Flags the block's scripts set: battle-local, clear when the battle starts.
    local_flags: BTreeSet<u8>,
    /// Battle-local flags one record sets and another tests: they become battle flags
    /// ([`EventWriter::flag_name`]) and event conditions.
    shared_flags: BTreeSet<u8>,
    /// Branches of the scripts converted since they were last taken.
    branches: Vec<Branch>,
    /// Drama scenes written per record, so every scene id stays unique.
    scenes: BTreeMap<usize, usize>,
    /// Scripts of `run` records played on the way between phases, converted once.
    on_the_way: BTreeMap<usize, (Vec<EventAction>, ScriptEnd)>,
    units: &'a mut Vec<UnitSpawn>,
    persons: &'a [u16],
    /// Person → reinforcement group of its unit (`None`: on the map from the start).
    arrival: &'a BTreeMap<u16, Option<String>>,
    groups: &'a BTreeSet<String>,
    sources: &'a mut EventSources<'b>,
    drama: String,
    notes: Vec<String>,
    /// Kinds of things left out, reported once per battle.
    skipped: BTreeSet<&'static str>,
    /// Officers moved in or out of the army ([`Converted::army`]).
    army: Vec<(String, bool)>,
    /// A chapter's battle whose last stage has a victory script: the flag an event sets when it
    /// ends the battle itself, for the outro to leave that script out ([`ended_flag`]).
    ended: Option<String>,
    /// Writing the opening ([`OPENING_GROUP`]): what the battle's units already have from its
    /// setup (AI, arrivals, the objective) is not done again.
    opening: bool,
}

/// The reference that names the unit of `person` on the map (`persons` runs alongside `units`):
/// its officer, else its tag, which it is given when it has none (`person_<number>`).
fn person_tag(units: &mut [UnitSpawn], persons: &[u16], person: u16) -> Option<String> {
    let i = persons.iter().position(|&q| q == person)?;
    let unit = &mut units[i];
    Some(
        unit.officer
            .clone()
            .or_else(|| unit.tag.clone())
            .unwrap_or_else(|| {
                let tag = format!("person_{person}");
                unit.tag = Some(tag.clone());
                tag
            }),
    )
}

impl EventWriter<'_, '_> {
    /// Whether `instr` of a chapter battle's opening is one `chapters::before_scene` already
    /// writes for the camp before the battle (`@level`, `@join`, `@away`).
    ///
    /// Input: an instruction of an opening record (group 2). Output: `true` when the opening
    /// must leave it out.
    ///
    /// Why: the opening's records are part of the battle's setup that the story reads too, so
    /// without this an officer would gain the levels twice, or join before the camp and then
    /// once more by the outro, with a `retreat` by officer id that would also take a deployed
    /// copy of them off the field ([`EventWriter::others_unit`] keeps only the enemy unit's
    /// retreat). The tests mirror that function's: the
    /// army's officers' levels, and an allegiance that brings an officer in or sends one of
    /// the army away (the enemies the setup assigns are the battle's own).
    fn changed_before_camp(&self, instr: &Instr) -> bool {
        let get = |name: &str| instr.operands.get(name).unwrap_or(0);
        let Some(id) = self.names.officers.get(&get("person")) else {
            return false;
        };
        let in_army = self.names.player_officers.contains(id);
        match instr.mnemonic {
            "add_levels" => in_army,
            "set_country" => get("country") == 0 || in_army,
            "set_allegiance" => get("army") == 0 || in_army,
            _ => false,
        }
    }

    /// A reference to the unit of `person` on a side other than the player's, which names that
    /// unit alone: its tag, or `person_<number>` given to it when it has none.
    ///
    /// Input: an original person number. Output: the reference, or `None` when the person has
    /// no such unit (or only one whose tag other units share).
    ///
    /// Why not the officer id: an officer who joins before the camp may be deployed as well,
    /// and a `retreat` by officer id would take that player unit off the field too.
    fn others_unit(&mut self, person: u16) -> Option<String> {
        let i = self
            .persons
            .iter()
            .zip(self.units.iter())
            .position(|(&p, u)| p == person && u.side != Side::Player)?;
        match self.units[i].tag.clone() {
            Some(tag) => {
                let shared = self
                    .units
                    .iter()
                    .filter(|u| u.tag.as_deref() == Some(tag.as_str()))
                    .count()
                    > 1;
                (!shared).then_some(tag)
            }
            None => {
                let tag = format!("person_{person}");
                self.units[i].tag = Some(tag.clone());
                Some(tag)
            }
        }
    }

    fn officer_ref(&self, person: u16) -> Option<String> {
        self.roles
            .iter()
            .find(|(p, _)| *p == person)
            .map(|(_, id)| id.to_string())
            .or_else(|| self.names.officers.get(&person).cloned())
    }

    /// A person as a unit reference: an officer on the map or in the player's army, else a tag
    /// given to the person's (first) unit on the map. `Ok(None)` is [`ANY_UNIT`].
    fn unit_ref(&mut self, person: u16) -> Result<Option<String>, String> {
        if person == ANY_UNIT {
            return Ok(None);
        }
        if let Some(id) = self.officer_ref(person) {
            if self.names.player_officers.contains(&id)
                || self.units.iter().any(|u| u.officer.as_deref() == Some(&id))
            {
                return Ok(Some(id));
            }
        }
        person_tag(self.units, self.persons, person)
            .map(Some)
            .ok_or_else(|| format!("{} is not on the map", self.names.person_label(person)))
    }

    /// `trigger` with each unit reference replaced by one that names its unit alone (`#<index>`
    /// for a unit of the battle), so references by tag and by officer id compare equal.
    fn canonical(&self, trigger: &Trigger) -> Trigger {
        let canon = |r: &String| {
            self.units
                .iter()
                .position(|u| u.officer.as_ref() == Some(r) || u.tag.as_ref() == Some(r))
                .map_or_else(|| r.clone(), |i| format!("#{i}"))
        };
        match trigger {
            Trigger::Adjacent { a, b } => Trigger::Adjacent {
                a: a.as_ref().map(canon),
                b: canon(b),
            },
            Trigger::UnitDefeated { target } => Trigger::UnitDefeated {
                target: canon(target),
            },
            Trigger::HpBelow { target, pct } => Trigger::HpBelow {
                target: canon(target),
                pct: *pct,
            },
            Trigger::Reach {
                who,
                pos,
                radius,
                to,
            } => Trigger::Reach {
                who: who.as_ref().map(canon),
                pos: *pos,
                radius: *radius,
                to: *to,
            },
            other => other.clone(),
        }
    }

    /// A named unit reference (not [`ANY_UNIT`]).
    fn named(&mut self, person: u16) -> Result<String, String> {
        self.unit_ref(person)?
            .ok_or_else(|| "needs a unit, not any unit".to_string())
    }

    fn speaker(&self, person: u16) -> String {
        match self.officer_ref(person) {
            Some(id) => id,
            None => free_speaker(
                self.names
                    .person_names
                    .get(&person)
                    .map_or("???", String::as_str),
            ),
        }
    }

    /// The battle flag of a shared scenario flag.
    fn flag_name(&self, flag: u8) -> String {
        if self.chapter {
            crate::chapters::flag(flag)
        } else {
            format!("orig_{}_{flag}", self.battle_id)
        }
    }

    /// Whether an `if_flags` condition holds when the battle's events run: the route's flags are
    /// set (and the other route flags of [`ORIGINAL_BATTLES`] clear), the block's own flags
    /// clear; any other flag is taken as clear and noted.
    fn holds(&mut self, record: usize, all_set: &[u8], all_clear: &[u8]) -> bool {
        let value = |f: u8, this: &mut Self| {
            let known = this.local_flags.contains(&f)
                || ORIGINAL_BATTLES.iter().any(|p| p.flags.contains(&f));
            if !known {
                let note = format!(
                    "record {record}: scenario flag {f} is taken as clear (set outside the battle)"
                );
                if !this.notes.contains(&note) {
                    this.notes.push(note);
                }
            }
            this.route.contains(&f)
        };
        let mut ok = true;
        for &f in all_set {
            ok &= value(f, self);
        }
        for &f in all_clear {
            ok &= !value(f, self);
        }
        ok
    }

    /// Convert the script of record `record` into actions (with its text as drama scenes named
    /// after the record, unless `with_text` is off); returns how it ended. Parts guarded by
    /// shared flags go to [`EventWriter::branches`].
    fn script(
        &mut self,
        record: usize,
        code: &[Instr],
        actions: &mut Vec<EventAction>,
        with_text: bool,
    ) -> ScriptEnd {
        self.script_part(record, code, actions, with_text, &[])
    }

    /// The script of a `run` record played on the way from one phase to the next, converted
    /// once and reused by every record that leaves the phase.
    fn on_the_way(
        &mut self,
        record: usize,
        code: &[Instr],
        actions: &mut Vec<EventAction>,
    ) -> ScriptEnd {
        if let Some((cached, end)) = self.on_the_way.get(&record) {
            actions.extend(cached.iter().cloned());
            return *end;
        }
        let taken = std::mem::take(&mut self.branches);
        let mut converted = Vec::new();
        let end = self.script(record, code, &mut converted, true);
        let dropped = std::mem::replace(&mut self.branches, taken);
        if !dropped.is_empty() {
            self.notes.push(format!(
                "record {record}: its flag-guarded parts are left out (it runs between phases)"
            ));
            // Their lines were written already: nothing plays them now.
            for action in dropped.iter().flat_map(|b| &b.actions) {
                if let EventAction::Drama { scene } = action {
                    remove_scene(&mut self.drama, scene);
                }
            }
        }
        actions.extend(converted.iter().cloned());
        self.on_the_way.insert(record, (converted, end));
        end
    }

    /// The part of a script past a flag-guarded part that ends it: it runs while `when` holds
    /// but not all of `tested`. A test of shared flags inside it would need both at once, so
    /// such a part is left out with a note.
    /// The event is `once`: the original checks its records again and again (FORMATS §13.2),
    /// but a trigger such as `reach` holds as long as the unit stands there.
    fn unless_part(
        &mut self,
        record: usize,
        code: &[Instr],
        with_text: bool,
        when: &[FlagCond],
        tested: Vec<FlagCond>,
    ) {
        // What converting the part writes besides its events, undone if it is left out.
        let (drama_len, scenes, units) = (
            self.drama.len(),
            self.scenes.get(&record).copied(),
            self.units.clone(),
        );
        let taken = std::mem::take(&mut self.branches);
        let mut actions = Vec::new();
        let end = self.script_part(record, code, &mut actions, with_text, when);
        if !std::mem::replace(&mut self.branches, taken).is_empty() {
            self.drama.truncate(drama_len);
            match scenes {
                Some(n) => self.scenes.insert(record, n),
                None => self.scenes.remove(&record),
            };
            *self.units = units;
            self.notes.push(format!(
                "record {record}: what its script does while its flags do not hold is left out \
                 (it tests shared flags again)"
            ));
            return;
        }
        self.branches.push(Branch {
            when: when.to_vec(),
            unless: tested,
            actions,
            end,
            at: None,
        });
    }

    /// [`EventWriter::script`] for a part of a script that runs while `when` holds.
    fn script_part(
        &mut self,
        record: usize,
        code: &[Instr],
        actions: &mut Vec<EventAction>,
        with_text: bool,
        when: &[FlagCond],
    ) -> ScriptEnd {
        let base_id = format!("orig_{}_{record}", self.battle_id);
        let mut scene = String::new();
        let flush = |scene: &mut String, actions: &mut Vec<EventAction>, this: &mut Self| {
            if scene.is_empty() {
                return;
            }
            let n = this.scenes.entry(record).or_insert(0);
            *n += 1;
            let id = if *n == 1 {
                base_id.clone()
            } else {
                format!("{base_id}_{n}")
            };
            let _ = write!(this.drama, "\n== {id}\n{scene}@hide all\n");
            scene.clear();
            actions.push(EventAction::Drama { scene: id });
        };
        let mut skip = 0u8;
        let mut after_levels = false;
        let mut end = ScriptEnd::Done;
        // The fighters of the duel being written (persons): left, right.
        let mut duel: Option<(u16, u16, String, String)> = None;
        for (index, instr) in code.iter().enumerate() {
            if skip > 0 {
                skip -= 1;
                continue;
            }
            let get = |name: &str| instr.operands.get(name).unwrap_or(0);
            let was_levels = std::mem::take(&mut after_levels);
            let text = matches!(
                instr.mnemonic,
                "dialogue" | "narration" | "caption" | "duel" | "duel_action" | "duel_end"
            );
            if text && !with_text {
                continue;
            }
            match instr.mnemonic {
                "if_flags" => {
                    if let Operands::Condition {
                        skip: n,
                        all_set,
                        all_clear,
                    } = &instr.operands
                    {
                        // Shared flags become conditions of the event, and so do, in a chapter's
                        // battle, the flags the story set before it (campaign flags the battle
                        // starts with); the others are decided now.
                        let shared = |f: &&u8| {
                            self.shared_flags.contains(*f)
                                || (self.chapter && !self.local_flags.contains(*f))
                        };
                        let (set_shared, set_now): (Vec<u8>, Vec<u8>) =
                            all_set.iter().partition(shared);
                        let (clear_shared, clear_now): (Vec<u8>, Vec<u8>) =
                            all_clear.iter().partition(shared);
                        if !self.holds(record, &set_now, &clear_now) {
                            skip = *n;
                        } else if !set_shared.is_empty() || !clear_shared.is_empty() {
                            // The guarded part becomes a branch with these conditions.
                            let mut cond = when.to_vec();
                            for (flags, cmp) in
                                [(set_shared, Compare::Ne), (clear_shared, Compare::Eq)]
                            {
                                for f in flags {
                                    let c = FlagCond {
                                        flag: self.flag_name(f),
                                        cmp,
                                        value: 0,
                                    };
                                    if !cond.contains(&c) {
                                        cond.push(c);
                                    }
                                }
                            }
                            flush(&mut scene, actions, self);
                            let guarded_to = (index + 1 + usize::from(*n)).min(code.len());
                            let mut guarded = Vec::new();
                            let branch_end = self.script_part(
                                record,
                                &code[index + 1..guarded_to],
                                &mut guarded,
                                with_text,
                                &cond,
                            );
                            // The flags this test adds to those the part already runs under.
                            let tested: Vec<FlagCond> =
                                cond.iter().filter(|c| !when.contains(c)).cloned().collect();
                            self.branches.push(Branch {
                                when: cond,
                                unless: Vec::new(),
                                actions: guarded,
                                end: branch_end,
                                at: when.is_empty().then_some(actions.len()),
                            });
                            skip = *n;
                            if branch_end != ScriptEnd::Done {
                                // What follows runs only while the flags do not hold: a part
                                // of its own that fires unless they all do. (A test of flags
                                // the part already runs under always holds: nothing follows.)
                                let rest = &code[guarded_to..];
                                if rest.iter().any(|c| c.mnemonic != "end") && !tested.is_empty() {
                                    if branch_end == ScriptEnd::Branched {
                                        // The guarded part ends only on some of its own flags:
                                        // what follows would need those too.
                                        self.notes.push(format!(
                                            "record {record}: what its script does while its \
                                             flags do not hold is left out (nested flag tests)"
                                        ));
                                    } else {
                                        self.unless_part(record, rest, with_text, when, tested);
                                    }
                                }
                                end = ScriptEnd::Branched;
                                break;
                            }
                        }
                    }
                }
                "dialogue" => match self.sources.text.dialogue(get("text")) {
                    Ok(lines) => {
                        for (speaker, text) in lines {
                            let head = format!("{}:", self.speaker(speaker));
                            push_text(&mut scene, &head, &text);
                        }
                    }
                    Err(e) => self.notes.push(format!("record {record}: {e}")),
                },
                "narration" | "caption" => {
                    // The game shows level-ups itself.
                    if instr.mnemonic == "caption" && was_levels {
                        continue;
                    }
                    match self.sources.text.string(get("text")) {
                        Ok(text) => push_text(&mut scene, "@narr", &text),
                        Err(e) => self.notes.push(format!("record {record}: {e}")),
                    }
                }
                "duel" => {
                    let (first, second) = (get("first"), get("second"));
                    match (self.officer_ref(first), self.officer_ref(second)) {
                        (Some(left), Some(right)) => {
                            let _ = writeln!(scene, "@duel {left} {right} {DUEL_BACKGROUND}");
                            duel = Some((first, second, left, right));
                        }
                        _ => {
                            self.notes.push(format!(
                                "record {record}: the duel of {} and {} is left out (not both \
                                 officers of the pack)",
                                self.names.person_label(first),
                                self.names.person_label(second)
                            ));
                            duel = None;
                        }
                    }
                }
                "duel_action" => {
                    let Some((first, second, left, right)) = &duel else {
                        self.notes.push(format!(
                            "record {record}: a duel move without a duel it belongs to is left out"
                        ));
                        continue;
                    };
                    let (first, second) = (*first, *second);
                    // A duel that went on past the end of a scene starts again in the next.
                    if !scene.contains("@duel ") {
                        let _ = writeln!(scene, "@duel {left} {right} {DUEL_BACKGROUND}");
                    }
                    let (person, action) = (get("person"), get("action"));
                    let side = if person == first {
                        "left"
                    } else if person == second {
                        "right"
                    } else {
                        self.notes.push(format!(
                            "record {record}: a duel move of {}, who is not fighting, is left out",
                            self.names.person_label(person)
                        ));
                        continue;
                    };
                    match duel_moves(action) {
                        Some([]) => scene.push_str("@wait 300\n"),
                        Some(moves) => {
                            for m in moves {
                                let _ = writeln!(scene, "@duel_act {side} {m}");
                            }
                        }
                        None => self.notes.push(format!(
                            "record {record}: duel move {action} is not known; left out"
                        )),
                    }
                }
                "duel_end" => {
                    if duel.take().is_some() && scene.contains("@duel ") {
                        scene.push_str("@duel_end\n");
                    }
                }
                // What the setup changes in the army before the camp (`chapters::before_scene`)
                // is not changed again as the battle begins.
                "add_levels" | "set_country" | "set_allegiance"
                    if self.opening && self.chapter && self.changed_before_camp(instr) =>
                {
                    let note = format!(
                        "record {record}: `{}` of {} is made before the camp",
                        instr.mnemonic,
                        self.names.person_label(get("person"))
                    );
                    if !self.notes.contains(&note) {
                        self.notes.push(note);
                    }
                    // One who joins still leaves the other side's ranks as the battle begins.
                    let joins = match instr.mnemonic {
                        "set_country" => get("country") == 0,
                        "set_allegiance" => get("army") == 0,
                        _ => false,
                    };
                    if joins {
                        if let Some(target) = self.others_unit(get("person")) {
                            flush(&mut scene, actions, self);
                            let retreat = EventAction::Retreat { target };
                            if !actions.contains(&retreat) {
                                actions.push(retreat);
                            }
                        }
                    }
                }
                "add_levels" => {
                    flush(&mut scene, actions, self);
                    match self.named(get("person")) {
                        Ok(target) => actions.push(EventAction::LevelUp {
                            target,
                            amount: u32::from(get("levels")),
                        }),
                        Err(e) => self.notes.push(format!("record {record}: level-up: {e}")),
                    }
                    after_levels = true;
                }
                "remove_person" => {
                    flush(&mut scene, actions, self);
                    match self.named(get("person")) {
                        Ok(target) => actions.push(EventAction::Retreat { target }),
                        Err(e) => self.notes.push(format!("record {record}: retreat: {e}")),
                    }
                }
                // The opening of the battle: its units already start with this AI, are on the
                // field (or kept for a later arrival) and have this objective.
                "set_ai" | "join_battle" | "set_objective" if self.opening => {}
                "set_ai" => {
                    flush(&mut scene, actions, self);
                    let mode = get("mode") as u8;
                    let param = match mode {
                        4 | 6 => get("p1") | (get("p2") << 8),
                        _ => get("target"),
                    };
                    let (mut ai, target, ai_pos) = ai_mode(mode, param);
                    let mut ai_target = None;
                    if let Some(t) = target {
                        match self.unit_ref(t) {
                            Ok(Some(r)) => ai_target = Some(r),
                            _ => {
                                ai = without_target(ai);
                                self.notes.push(format!(
                                    "record {record}: AI target {} is not on the map; {}",
                                    self.names.person_label(t),
                                    fallback_note(ai)
                                ));
                            }
                        }
                    }
                    match self.named(get("person")) {
                        Ok(target) => actions.push(EventAction::SetAi {
                            target,
                            ai,
                            ai_target,
                            ai_pos,
                        }),
                        Err(e) => self.notes.push(format!("record {record}: AI change: {e}")),
                    }
                }
                "join_battle" => {
                    flush(&mut scene, actions, self);
                    let person = get("person");
                    match self.arrival.get(&person) {
                        Some(Some(group)) if self.groups.contains(group) => {
                            let spawn = EventAction::Spawn {
                                group: group.clone(),
                            };
                            if !actions.contains(&spawn) {
                                actions.push(spawn);
                            }
                        }
                        Some(_) => {
                            let note = format!(
                                "record {record}: {} arrives without a unit of their own in the \
                                 battle (the army's officer): only the scene is converted",
                                self.names.person_label(person)
                            );
                            if !self.notes.contains(&note) {
                                self.notes.push(note);
                            }
                        }
                        None => self.notes.push(format!(
                            "record {record}: {} joins but has no unit in the battle",
                            self.names.person_label(person)
                        )),
                    }
                }
                "add_item" => {
                    flush(&mut scene, actions, self);
                    let item = get("item") as u8;
                    match self.names.items.get(&item) {
                        Some(id) => actions.push(EventAction::GiveItem { item: id.clone() }),
                        None => self.notes.push(format!(
                            "record {record}: item {item} has no base-pack item"
                        )),
                    }
                }
                "data" => {
                    if get("kind") == DATA_GOLD {
                        flush(&mut scene, actions, self);
                        actions.push(EventAction::GiveGold {
                            amount: i64::from(get("value")),
                        });
                    }
                }
                "set_map_chip" => {
                    flush(&mut scene, actions, self);
                    let pos = Pos::new(i32::from(get("x")), i32::from(get("y")));
                    match (self.sources.cell_change)(pos, get("chip") as u8) {
                        Ok(Some((terrain, image))) => actions.push(EventAction::SetTerrain {
                            pos,
                            terrain,
                            image,
                        }),
                        Ok(None) => self.notes.push(format!(
                            "record {record}: map cell ({}, {}): the operation does not apply \
                             to its terrain",
                            pos.x, pos.y
                        )),
                        Err(e) => self.notes.push(format!(
                            "record {record}: map cell ({}, {}): {e}",
                            pos.x, pos.y
                        )),
                    }
                }
                "set_flag" => {
                    let flag = get("flag") as u8;
                    // A chapter's battle keeps every flag: the story after it may read it.
                    if self.shared_flags.contains(&flag) || self.chapter {
                        flush(&mut scene, actions, self);
                        actions.push(EventAction::SetFlag {
                            flag: self.flag_name(flag),
                            value: i64::from(get("clear") == 0),
                        });
                    } else if !self.local_flags.contains(&flag) {
                        // A flag the story reads later (a route, a deed): the campaign's.
                        flush(&mut scene, actions, self);
                        actions.push(EventAction::SetFlag {
                            flag: crate::chapters::flag(flag),
                            value: i64::from(get("clear") == 0),
                        });
                    }
                }
                "leave_parallel" => {
                    end = ScriptEnd::LeavesPhase;
                    break;
                }
                "battle_end" | "goto_block" => {
                    end = ScriptEnd::EndsBattle;
                    break;
                }
                "set_objective" => match self.sources.text.string(get("text")) {
                    Ok(text) => {
                        let text = objective_text(&text);
                        if !text.is_empty() {
                            flush(&mut scene, actions, self);
                            actions.push(EventAction::SetObjective { text });
                        }
                    }
                    Err(e) => self.notes.push(format!("record {record}: objective: {e}")),
                },
                // In a chapter's battle an officer joins (country 0, persuaded) or leaves the
                // army: a campaign flag the story after the battle acts on. The persuaded unit
                // leaves the field.
                // (`set_allegiance` is the same: to army 0 an officer talked round becomes the
                // army's, Zhang Liao at Xuchang 2; to another army they leave it after the battle,
                // Shamoke at Yiling.)
                "set_country" | "set_allegiance" if self.chapter => {
                    let person = get("person");
                    let side = if instr.mnemonic == "set_country" {
                        get("country")
                    } else {
                        get("army")
                    };
                    match self.names.officers.get(&person).cloned() {
                        Some(id) => {
                            let joins = side == 0;
                            flush(&mut scene, actions, self);
                            actions.push(EventAction::SetFlag {
                                flag: army_flag(&id, joins),
                                value: 1,
                            });
                            if joins {
                                if let Ok(target) = self.named(person) {
                                    if !actions.contains(&EventAction::Retreat {
                                        target: target.clone(),
                                    }) {
                                        actions.push(EventAction::Retreat { target });
                                    }
                                }
                            }
                            if !self.army.contains(&(id.clone(), joins)) {
                                self.army.push((id, joins));
                            }
                        }
                        None => self.notes.push(format!(
                            "record {record}: `{}` of {} (no pack officer) is not converted",
                            instr.mnemonic,
                            self.names.person_label(person)
                        )),
                    }
                }
                "set_country" | "set_allegiance" | "set_class" | "set_officer_bit"
                | "withdraw_unit" => {
                    self.notes.push(format!(
                        "record {record}: `{}` is not converted",
                        instr.mnemonic
                    ));
                }
                // Presentation the drama or the battle screen does on its own.
                _ => {}
            }
        }
        flush(&mut scene, actions, self);
        if end == ScriptEnd::EndsBattle {
            actions.extend(self.ended_flag_action());
            actions.push(EventAction::Victory);
        }
        end
    }

    /// The action that tells the outro an event ended the battle ([`EventWriter::ended`]).
    fn ended_flag_action(&self) -> Option<EventAction> {
        self.ended.as_ref().map(|flag| EventAction::SetFlag {
            flag: flag.clone(),
            value: 1,
        })
    }
}

/// An original objective text as one line: the original lists its conditions numbered on lines
/// of their own (`1,적의 전멸\r2,유비가 …`) and marks names in brackets (`[여포]`).
pub(crate) fn objective_text(raw: &str) -> String {
    // A name in brackets followed by a space before its particle: `[여포] 의` is `여포의`.
    const PARTICLES: [&str; 11] = [
        "의", "을", "를", "이", "가", "은", "는", "와", "과", "에게", "에",
    ];
    raw.split(['\r', '\n'])
        .map(|line| {
            let line = line.trim();
            // `1,`, `10,` or `-1,` in front: the number of the condition.
            let line = match line.split_once(',') {
                Some((n, rest)) => {
                    let digits = n.trim().trim_start_matches('-');
                    if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
                        rest.trim()
                    } else {
                        line
                    }
                }
                None => line,
            };
            let mut line = line.to_string();
            for p in PARTICLES {
                line = line.replace(&format!("] {p}"), &format!("]{p}"));
            }
            line.replace(['[', ']'], "")
        })
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" / ")
}

/// Re-stage `base` as the original battle `orig` of `pairing` on the map `map_id`, with the
/// original's mid-battle events read through `sources`.
pub fn convert(
    base: &BattleDef,
    orig: &OriginalBattle,
    names: &Names,
    pairing: &Pairing,
    map_id: &str,
    sources: &mut EventSources<'_>,
) -> Result<Converted, String> {
    let roles = pairing.roles;
    let mut notes = Vec::new();
    let mut battle = base.clone();
    battle.map = MapDef {
        use_map: Some(map_id.to_string()),
        ..MapDef::default()
    };
    battle.turn_limit = u32::from(orig.header.turn_limit);
    let officer_ref = |person: u16| {
        roles
            .iter()
            .find(|(p, _)| *p == person)
            .map(|(_, id)| id.to_string())
            .or_else(|| names.officers.get(&person).cloned())
    };

    // When each person comes onto the map: with the first record that brings it in; `None` = at
    // the start (the opening brings it in right after the battle begins).
    let mut arrival: BTreeMap<u16, Option<String>> = BTreeMap::new();
    for j in &orig.joins {
        let group = (j.group >= FIRST_PHASE_GROUP).then(|| arrival_group(j.record));
        for &p in &j.persons {
            arrival.entry(p).or_insert_with(|| group.clone());
        }
    }
    let mut never_arrive = 0;

    // Units, and the person of each (for references by trigger records).
    let mut units: Vec<UnitSpawn> = Vec::new();
    let mut persons: Vec<u16> = Vec::new();
    let mut placed: BTreeSet<String> = BTreeSet::new();

    // Deployment. Named officers of the setup whom the base battle has on its side, and those
    // the setup keeps back until an event brings them in, become units; every other slot is a
    // deploy tile.
    let mut slots = Vec::new();
    let conditional = orig.flagged_slots_left_out;
    for u in &orig.player {
        let pos = Pos::new(i32::from(u.x), i32::from(u.y));
        let officer = (u.person != LIU_BEI && u.person != ANY_OFFICER)
            .then(|| officer_ref(u.person))
            .flatten();
        let joins_later = u.other.get(2) == Some(&1);
        let group = if joins_later {
            match arrival.get(&u.person) {
                Some(g) => g.clone(),
                None => {
                    never_arrive += 1;
                    continue;
                }
            }
        } else {
            None
        };
        // A civilian stands on the tile as an allied unit, holding it until the opening orders
        // it about (the people of Changban).
        if let Some((class, level)) = names.civilians.get(&u.person) {
            units.push(UnitSpawn {
                side: Side::Ally,
                officer: None,
                name: Some(
                    names
                        .person_names
                        .get(&u.person)
                        .cloned()
                        .unwrap_or_else(|| "백성".to_string()),
                ),
                class: Some(class.clone()),
                level: Some(*level),
                stats: names.stats.get(&u.person).copied(),
                pos,
                ai: AiMode::Hold,
                ai_target: None,
                ai_pos: None,
                commander: false,
                tag: None,
                group,
                equip: None,
                drop: None,
            });
            persons.push(u.person);
            continue;
        }
        let guest = officer.as_ref().and_then(|id| {
            base.units
                .iter()
                .find(|b| b.officer.as_deref() == Some(id) && b.side != Side::Enemy)
        });
        match (officer, guest) {
            (Some(id), Some(b)) => {
                units.push(UnitSpawn {
                    pos,
                    group,
                    ..b.clone()
                });
                persons.push(u.person);
                placed.insert(id);
            }
            (Some(id), None) if joins_later => {
                units.push(UnitSpawn {
                    side: Side::Ally,
                    officer: Some(id.clone()),
                    name: None,
                    class: None,
                    level: None,
                    stats: None,
                    pos,
                    ai: AiMode::Aggressive,
                    ai_target: None,
                    ai_pos: None,
                    commander: false,
                    tag: None,
                    group,
                    equip: None,
                    drop: None,
                });
                persons.push(u.person);
                placed.insert(id);
            }
            (None, _) if joins_later => notes.push(format!(
                "{}: an officer who joins during the battle without a base-pack officer; left out",
                names.person_label(u.person)
            )),
            (officer, _) => slots.push((officer, u.person, pos)),
        }
    }
    if conditional > 0 {
        notes.push(format!(
            "{conditional} player slot(s) that need a campaign flag (officers who join for this \
             battle only) are left out"
        ));
    }
    if slots.is_empty() {
        return Err("the original battle has no player slot".into());
    }
    // The engine fills the tiles with the required officers first, then the lord: put the
    // original tiles of those officers first so everyone starts where the original has them.
    let mut ordered = Vec::with_capacity(slots.len());
    for id in &base.deploy.required {
        if let Some(i) = slots.iter().position(|(o, ..)| o.as_ref() == Some(id)) {
            ordered.push(slots.remove(i));
        }
    }
    if let Some(i) = slots.iter().position(|&(_, p, _)| p == LIU_BEI) {
        ordered.push(slots.remove(i));
    }
    ordered.append(&mut slots);
    let slots: Vec<Pos> = ordered.into_iter().map(|(_, _, pos)| pos).collect();
    if (battle.deploy.max as usize) > slots.len() {
        notes.push(format!(
            "deploy max {} lowered to the original's {} slots",
            battle.deploy.max,
            slots.len()
        ));
        battle.deploy.max = slots.len() as u32;
    }
    battle.deploy.slots = slots;

    // Enemy and allied rosters.
    for (friendly, roster) in &orig.rosters {
        let side = if *friendly { Side::Ally } else { Side::Enemy };
        for u in roster {
            let group = if u.other.get(1) == Some(&1) {
                match arrival.get(&u.person) {
                    Some(g) => g.clone(),
                    None => {
                        never_arrive += 1;
                        continue;
                    }
                }
            } else {
                None
            };
            let Some(class) = u.class.and_then(|c| names.classes.get(&c)).cloned() else {
                notes.push(format!(
                    "{}: class {:?} has no base-pack class; left out",
                    names.person_label(u.person),
                    u.class
                ));
                continue;
            };
            let mut officer = officer_ref(u.person);
            if officer.as_ref().is_some_and(|o| placed.contains(o)) {
                // An officer appears at most once per battle; a second record plays generic.
                officer = None;
            }
            let (ai, target, ai_pos) = ai_mode(u.ai_mode.unwrap_or(0), u.ai_param.unwrap_or(0));
            let mut spawn = UnitSpawn {
                side,
                officer: officer.clone(),
                name: None,
                class: Some(class),
                level: u.level.map(u32::from),
                stats: None,
                pos: Pos::new(i32::from(u.x), i32::from(u.y)),
                ai,
                ai_target: None,
                ai_pos,
                commander: orig.header.defeat_to_win == Some(u.person),
                tag: None,
                group,
                equip: None,
                drop: None,
            };
            if spawn.officer.is_none() {
                spawn.name = Some(
                    names
                        .person_names
                        .get(&u.person)
                        .cloned()
                        .unwrap_or_else(|| "병사".to_string()),
                );
                spawn.stats = names.stats.get(&u.person).copied();
            }
            if let Some(t) = target {
                match officer_ref(t).or_else(|| person_tag(&mut units, &persons, t)) {
                    Some(id) => spawn.ai_target = Some(id),
                    None => {
                        spawn.ai = without_target(spawn.ai);
                        notes.push(format!(
                            "{}: AI target {} has no base-pack officer; {}",
                            names.person_label(u.person),
                            names.person_label(t),
                            fallback_note(spawn.ai)
                        ));
                    }
                }
            }
            if let Some(id) = &officer {
                if let Some(b) = base
                    .units
                    .iter()
                    .find(|b| b.officer.as_deref() == Some(id) && b.side == side)
                {
                    spawn.tag = b.tag.clone();
                    spawn.drop = b.drop.clone();
                    spawn.equip = b.equip.clone();
                    spawn.commander |= b.commander;
                }
                placed.insert(id.clone());
            }
            units.push(spawn);
            persons.push(u.person);
        }
    }
    if never_arrive > 0 {
        notes.push(format!(
            "{never_arrive} unit(s) the original keeps off the map and brings in only from other \
             events are left out"
        ));
    }
    for &person in &orig.flagged_units_left_out {
        notes.push(format!(
            "{} needs a campaign flag in the original and is left out",
            names.person_label(person)
        ));
    }
    if orig.other_route_rosters > 0 {
        notes.push(format!(
            "{} roster(s) for another route of the original are not used",
            orig.other_route_rosters
        ));
    }
    if orig.later_rosters > 0 {
        notes.push(format!(
            "{} roster(s) loaded later in the original battle are not converted yet",
            orig.later_rosters
        ));
    }

    // The opening's AI (group 2 and before).
    for rec in orig
        .records
        .iter()
        .filter(|r| r.trigger.group < FIRST_PHASE_GROUP)
    {
        for instr in rec.code.iter().filter(|i| i.mnemonic == "set_ai") {
            let get = |name: &str| instr.operands.get(name).unwrap_or(0);
            let mode = get("mode") as u8;
            let param = match mode {
                4 | 6 => get("p1") | (get("p2") << 8),
                _ => get("target"),
            };
            let (ai, target, ai_pos) = ai_mode(mode, param);
            let ai_target =
                target.and_then(|t| officer_ref(t).or_else(|| person_tag(&mut units, &persons, t)));
            for (u, _) in units
                .iter_mut()
                .zip(&persons)
                .filter(|(_, &p)| p == get("person"))
            {
                (u.ai, u.ai_target, u.ai_pos) = match (target, &ai_target) {
                    (Some(_), None) => (without_target(ai), None, None),
                    _ => (ai, ai_target.clone(), ai_pos),
                };
            }
        }
    }

    // Base units that did not come along, and the references that name them.
    let kept: BTreeSet<&str> = units
        .iter()
        .flat_map(|u| u.officer.iter().chain(u.tag.iter()))
        .map(String::as_str)
        .collect();
    let mut gone = BTreeSet::new();
    for b in &base.units {
        let refs: Vec<&String> = b.officer.iter().chain(b.tag.iter()).collect();
        if refs.is_empty() || refs.iter().any(|r| kept.contains(r.as_str())) {
            // Generic base units are replaced by the original's rosters as a whole.
            continue;
        }
        notes.push(format!(
            "base unit `{}` is not in the original battle",
            refs[0]
        ));
        gone.extend(refs.into_iter().cloned());
    }

    let groups: BTreeSet<String> = units.iter().filter_map(|u| u.group.clone()).collect();
    let phases = phases(&orig.records);
    let mut stages: Vec<Option<u32>> = Vec::with_capacity(phases.len());
    let mut watched = 0u32;
    for p in &phases {
        stages.push((!p.runs_only).then(|| {
            watched += 1;
            watched - 1
        }));
    }
    let staged = watched > 1;
    let first_watched = stages.iter().position(Option::is_some);

    // Conditions. A `reach` of Liu Bei (or of any player unit) takes the original objective
    // area of the first phase; objectives of later phases become events of their stage.
    let routine = is_routine;
    let area = |r: &Record| {
        let a = r.trigger.args;
        let tile = |row: usize| Pos::new(i32::from(a[row + 1]), i32::from(a[row]));
        (tile(2), (r.trigger.kind == UNIT_IN_AREA).then(|| tile(4)))
    };
    let objective = {
        let areas: BTreeSet<(Pos, Option<Pos>)> = first_watched
            .map(|i| {
                phases[i]
                    .records
                    .iter()
                    .map(|&r| &orig.records[r])
                    .filter(|r| routine(r))
                    .map(area)
                    .collect()
            })
            .unwrap_or_default();
        (areas.len() == 1).then(|| areas.into_iter().next().expect("one area"))
    };
    // The objective area of a later phase, for a bonus (reaching it there wins the battle).
    let later_objective = {
        let areas: BTreeSet<(Pos, Option<Pos>)> = phases
            .iter()
            .enumerate()
            .filter(|(i, p)| !p.runs_only && Some(*i) != first_watched)
            .flat_map(|(_, p)| p.records.iter().map(|&r| &orig.records[r]))
            .filter(|r| routine(r))
            .map(area)
            .collect();
        (areas.len() == 1).then(|| areas.into_iter().next().expect("one area"))
    };
    let lord = officer_ref(LIU_BEI);
    let fix = |c: &Condition,
               notes: &mut Vec<String>,
               what: &str,
               objective: Option<(Pos, Option<Pos>)>|
     -> Option<Condition> {
        if let Some(r) = condition_refs(c).into_iter().find(|r| gone.contains(*r)) {
            notes.push(format!(
                "{what} naming `{r}` dropped (not in the original battle)"
            ));
            return None;
        }
        match c {
            Condition::Reach { who, .. } if who.is_none() || who.as_deref() == lord.as_deref() => {
                match objective {
                    Some((pos, to)) => Some(Condition::Reach {
                        who: who.clone(),
                        pos,
                        radius: 0,
                        to,
                    }),
                    None => {
                        notes.push(format!(
                            "{what} `reach` dropped: the original has no single objective area \
                             for it"
                        ));
                        None
                    }
                }
            }
            Condition::Reach { .. } => {
                notes.push(format!(
                    "{what} `reach` of another unit dropped (base-map tile)"
                ));
                None
            }
            other => Some(other.clone()),
        }
    };
    battle.victory = base
        .victory
        .iter()
        .filter_map(|c| fix(c, &mut notes, "victory condition", objective))
        .collect();
    battle.defeat = base
        .defeat
        .iter()
        .filter_map(|c| fix(c, &mut notes, "defeat condition", objective))
        .collect();
    battle.bonus = base.bonus.as_ref().and_then(|b| {
        let condition = fix(
            &b.condition,
            &mut notes,
            "bonus condition",
            objective.or(later_objective),
        )?;
        Some(hero_core::battledef::BonusDef {
            condition,
            ..b.clone()
        })
    });

    // Events: the base battle's that still fit, then the original's.
    battle.events = base
        .events
        .iter()
        .filter(|e| match event_problem(e, &gone) {
            Some(why) => {
                notes.push(format!("event on {:?} dropped: {why}", e.trigger));
                false
            }
            None => true,
        })
        .cloned()
        .collect();
    let base_events = battle.events.len();
    let flags_of = |r: &Record, tested: bool| -> BTreeSet<u8> {
        r.code
            .iter()
            .flat_map(|c| match (&c.operands, tested) {
                (
                    Operands::Condition {
                        all_set, all_clear, ..
                    },
                    true,
                ) => all_set.iter().chain(all_clear).copied().collect(),
                (_, false) if c.mnemonic == "set_flag" => c
                    .operands
                    .get("flag")
                    .map(|f| f as u8)
                    .into_iter()
                    .collect(),
                _ => Vec::new(),
            })
            .collect()
    };
    let local_flags: BTreeSet<u8> = orig
        .records
        .iter()
        .flat_map(|r| flags_of(r, false))
        .collect();
    let mut shared_flags = BTreeSet::new();
    for (a, ra) in orig.records.iter().enumerate() {
        for (b, rb) in orig.records.iter().enumerate() {
            if a != b {
                shared_flags.extend(flags_of(ra, true).intersection(&flags_of(rb, false)));
            }
        }
    }
    let mut writer = EventWriter {
        battle_id: &base.id,
        chapter: pairing.battle.is_empty(),
        names,
        roles,
        route: pairing.flags,
        local_flags,
        shared_flags,
        branches: Vec::new(),
        scenes: BTreeMap::new(),
        on_the_way: BTreeMap::new(),
        units: &mut units,
        persons: &persons,
        arrival: &arrival,
        groups: &groups,
        sources,
        drama: String::new(),
        notes: Vec::new(),
        skipped: BTreeSet::new(),
        army: Vec::new(),
        ended: (pairing.battle.is_empty() && events_end_battle(&orig.records))
            .then(|| ended_flag(&base.id)),
        opening: false,
    };
    // Where a script that ended with `end` in phase `i` moves the battle on: the actions to
    // add (victory, or the next stage and the scripts on the way), if it leaves the phase.
    let moves_on = |writer: &mut EventWriter, i: usize, parallel: bool, end: ScriptEnd| {
        let leaves = match end {
            ScriptEnd::LeavesPhase => true,
            ScriptEnd::Done => !parallel,
            ScriptEnd::EndsBattle | ScriptEnd::Branched => false,
        };
        if !leaves {
            return Vec::new();
        }
        match next_after(&phases, &stages, i) {
            Next::Victory => writer
                .ended_flag_action()
                .into_iter()
                .chain([EventAction::Victory])
                .collect(),
            Next::Stage(next, on_the_way) => {
                let mut actions = vec![EventAction::SetStage { stage: next }];
                for w in on_the_way {
                    if writer.on_the_way(w, &orig.records[w].code, &mut actions)
                        == ScriptEnd::EndsBattle
                    {
                        break;
                    }
                }
                actions
            }
        }
    };
    let mut events = Vec::new();
    // A chapter battle's opening plays when it begins: the opening group's lines (the challenge
    // before a duel, the words as the sides meet), then the phases of `run` records before the
    // first watched one, which nothing moves the battle into otherwise. They hold its opening
    // lines and what those set (Xuchang 2's opening sets flag 218, which brings Zhang Liao into
    // the next battle's enemy army).
    if pairing.battle.is_empty() {
        let mut actions = Vec::new();
        // The parts of the opening that run only while campaign flags hold (the route's lines:
        // Pang Tong's death at Jincang): events of their own, as a record's guarded parts are.
        let mut guarded = Vec::new();
        writer.opening = true;
        let mut ends = false;
        for (r, rec) in orig
            .records
            .iter()
            .enumerate()
            .filter(|(_, rec)| rec.trigger.group == OPENING_GROUP && rec.trigger.kind == RUN)
        {
            let end = writer.script(r, &rec.code, &mut actions, true);
            guarded.append(&mut writer.branches);
            if end == ScriptEnd::EndsBattle {
                ends = true;
                break;
            }
        }
        writer.opening = false;
        if !ends {
            'opening: for (i, phase) in phases.iter().enumerate() {
                // (A phase that ends the battle is its victory, as `next_after` takes it.)
                if stages[i].is_some() || phase.ends_battle {
                    break;
                }
                for &r in &phase.records {
                    if writer.on_the_way(r, &orig.records[r].code, &mut actions)
                        == ScriptEnd::EndsBattle
                    {
                        break 'opening;
                    }
                }
            }
        }
        events.extend(events_in_order(
            &Trigger::TurnStart {
                turn: 1,
                side: Side::Player,
            },
            None,
            actions,
            guarded,
        ));
    }
    for (i, phase) in phases.iter().enumerate() {
        let Some(stage) = stages[i] else {
            continue; // run records: played on the way from one phase to the next
        };
        for &r in &phase.records {
            let rec = &orig.records[r];
            let t = &rec.trigger;
            match t.kind {
                BATTLE_WON | BATTLE_LOST => {
                    // The base battle's conditions and outro.
                    writer
                        .skipped
                        .insert("the original's victory and defeat scripts");
                    continue;
                }
                _ if is_treasure(rec) => continue,
                _ => {}
            }
            if Some(i) == first_watched && routine(rec) {
                continue; // the objective condition
            }
            let trigger = if t.kind == RUN {
                if stage != 0 {
                    notes.push(format!(
                        "record {r}: a script at the start of a later phase is not converted"
                    ));
                    continue;
                }
                Trigger::TurnStart {
                    turn: 1,
                    side: Side::Player,
                }
            } else {
                match trigger_of(t.kind, t.inverted, t.args, &mut |p| writer.unit_ref(p)) {
                    Ok(trigger) => trigger,
                    Err(e) => {
                        notes.push(format!("record {r} is not converted: {e}"));
                        continue;
                    }
                }
            };
            let occasion = writer.canonical(&trigger);
            let kept = battle.events[..base_events].iter().position(|e| {
                same_occasion(&writer.canonical(&e.trigger), &occasion)
                    // A base event the record of another phase took over stays with that one.
                    && !(staged && e.stage.is_some_and(|s| s != stage))
            });
            if let Some(k) = kept {
                notes.push(format!(
                    "record {r}: the base battle's event on {:?} is kept instead",
                    battle.events[k].trigger
                ));
                // The base event tells it in its own words; the record's other actions (and
                // where it moves the battle on) are added to it.
                let mut extra = Vec::new();
                let end = writer.script(r, &rec.code, &mut extra, false);
                if !std::mem::take(&mut writer.branches).is_empty() {
                    notes.push(format!(
                        "record {r}: its flag-guarded parts are not added to the base event"
                    ));
                }
                let moved = if battle.events[k].actions.contains(&EventAction::Victory) {
                    Vec::new()
                } else {
                    moves_on(&mut writer, i, phase.parallel, end)
                };
                let kept = &mut battle.events[k];
                let before = kept.actions.clone();
                let mut at = kept
                    .actions
                    .iter()
                    .position(|a| *a == EventAction::Victory)
                    .unwrap_or(kept.actions.len());
                for a in extra.into_iter().chain(moved) {
                    if a == EventAction::Victory {
                        if !kept.actions.contains(&a) {
                            kept.actions.push(a);
                        }
                    } else if !kept.actions.contains(&a) {
                        kept.actions.insert(at, a);
                        at += 1;
                    }
                }
                if end == ScriptEnd::EndsBattle && !kept.actions.contains(&EventAction::Victory) {
                    kept.actions.push(EventAction::Victory);
                }
                if staged && kept.actions != before {
                    // What the original adds happens in the record's phase only.
                    kept.stage = Some(stage);
                }
                continue;
            }
            let mut actions = Vec::new();
            let end = writer.script(r, &rec.code, &mut actions, true);
            let mut branches = std::mem::take(&mut writer.branches);
            // The battle moves on after the record's last action: a part that runs the record
            // to its end (the record itself, what runs while a guarded part's flags do not hold)
            // or ends it on its own moves it on; a guarded part the record goes on after does
            // not (it would move the battle to the next stage before the rest is told, and the
            // events of this stage after it would not fire).
            actions.extend(moves_on(&mut writer, i, phase.parallel, end));
            for part in &mut branches {
                if part.end != ScriptEnd::Done || !part.unless.is_empty() {
                    part.actions
                        .extend(moves_on(&mut writer, i, phase.parallel, part.end));
                }
            }
            events.extend(events_in_order(
                &trigger,
                staged.then_some(stage),
                actions,
                branches,
            ));
        }
    }
    let drama = std::mem::take(&mut writer.drama);
    let army = std::mem::take(&mut writer.army);
    notes.append(&mut writer.notes);
    for what in &writer.skipped {
        notes.push(format!("not converted: {what}"));
    }
    battle.events.extend(events);
    if battle.victory.is_empty()
        && !battle
            .events
            .iter()
            .any(|e| e.actions.contains(&EventAction::Victory) && e.stage.is_none_or(|s| s == 0))
    {
        battle.victory = match orig.header.defeat_to_win {
            None => vec![Condition::DefeatAll],
            Some(p) => match officer_ref(p)
                .filter(|id| units.iter().any(|u| u.officer.as_deref() == Some(id)))
            {
                Some(target) => vec![Condition::DefeatUnit { target }],
                None => vec![Condition::DefeatCommander],
            },
        };
        notes.push("victory taken from the original's battle header".into());
    }

    // Units that wait for a group no event brings in never appear.
    let spawned: BTreeSet<&str> = battle
        .events
        .iter()
        .flat_map(|e| &e.actions)
        .filter_map(|a| match a {
            EventAction::Spawn { group } => Some(group.as_str()),
            _ => None,
        })
        .collect();
    let before = units.len();
    units.retain(|u| u.group.as_deref().is_none_or(|g| spawned.contains(g)));
    if units.len() < before {
        notes.push(format!(
            "{} unit(s) brought in only by records that are not converted are left out",
            before - units.len()
        ));
    }
    battle.units = units;

    // Treasures.
    let mut treasures = Vec::new();
    for c in orig.cells.iter().filter(|c| c.person == ANY_UNIT) {
        let item = match c.item {
            Some(i) => match names.items.get(&i) {
                Some(id) => Some(id.clone()),
                None => {
                    notes.push(format!(
                        "treasure item {i} at ({}, {}) has no base-pack item",
                        c.x, c.y
                    ));
                    None
                }
            },
            None => None,
        };
        if item.is_none() && c.gold == 0 {
            continue;
        }
        let pos = Pos::new(i32::from(c.x), i32::from(c.y));
        if treasures.iter().any(|t: &TreasureDef| t.pos == pos) {
            // The original has another record for the cell (a second find, or one of the
            // route's alternatives): a tile holds one treasure.
            notes.push(format!(
                "a second treasure at ({}, {}) left out: the first one stays",
                c.x, c.y
            ));
            continue;
        }
        treasures.push(TreasureDef {
            pos,
            item,
            gold: i64::from(c.gold),
        });
    }
    battle.treasures = treasures;
    Ok(Converted {
        battle,
        drama,
        notes,
        army,
    })
}

/// The events of a script that fire on `trigger` (once, at `stage`): what the script does
/// whatever the flags say (`actions`) and its parts guarded by flags (`branches`), in the order
/// the script reaches them. A guarded part sits where the script met it, so the unconditional
/// actions around it become events of their own: events fire in the order they are defined, and
/// a line that follows the guarded one must not be told before it. (Jincang's opening: the
/// defender, then Pang Tong or Zhao Yun by the route, then Jiang Wei.)
///
/// The parts without a place of their own (`at: None`: a test inside a guarded part, which is
/// written before that part, and what runs while a part's flags do not hold, written after it)
/// go with the next part that has one, or at the end.
fn events_in_order(
    trigger: &Trigger,
    stage: Option<u32>,
    actions: Vec<EventAction>,
    branches: Vec<Branch>,
) -> Vec<EventDef> {
    let event = |when: Vec<FlagCond>, unless: Vec<FlagCond>, actions: Vec<EventAction>| EventDef {
        trigger: trigger.clone(),
        once: true,
        stage,
        when,
        unless,
        actions,
    };
    let mut out = Vec::new();
    let mut start = 0;
    let mut waiting: Vec<Branch> = Vec::new();
    for part in branches {
        let Some(at) = part.at else {
            waiting.push(part);
            continue;
        };
        if at > start && at <= actions.len() {
            out.push(event(Vec::new(), Vec::new(), actions[start..at].to_vec()));
            start = at;
        }
        for p in waiting.drain(..).chain([part]) {
            if !p.actions.is_empty() {
                out.push(event(p.when, p.unless, p.actions));
            }
        }
    }
    for p in waiting {
        if !p.actions.is_empty() {
            out.push(event(p.when, p.unless, p.actions));
        }
    }
    if start < actions.len() {
        out.push(event(Vec::new(), Vec::new(), actions[start..].to_vec()));
    }
    out
}

/// Reinforcement group of the units a trigger record brings in.
fn arrival_group(record: usize) -> String {
    format!("original_{record}")
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::scenario::{Arg, ArgKind, Block, Instr, Record, Trigger as RecTrigger};

    fn instr(mnemonic: &'static str, operands: Operands) -> Instr {
        Instr {
            offset: 0,
            opcode: 0,
            mnemonic,
            operands,
        }
    }

    fn fields(mnemonic: &'static str, args: &[(&'static str, u16)]) -> Instr {
        instr(
            mnemonic,
            Operands::Fields {
                args: args
                    .iter()
                    .map(|&(name, value)| Arg {
                        name,
                        kind: ArgKind::Number,
                        value,
                    })
                    .collect(),
            },
        )
    }

    fn op(mnemonic: &'static str) -> Instr {
        fields(mnemonic, &[])
    }

    /// A trigger record of `group`; `flag` is the group flag (parallel control).
    fn record(kind: u8, group: u8, flag: bool, args: [u8; 6], code: Vec<Instr>) -> Record {
        Record {
            offset: 0,
            trigger: RecTrigger {
                kind,
                kind_name: "",
                inverted: false,
                group,
                group_flag: flag,
                args,
            },
            code_offset: 0,
            code,
        }
    }

    fn unit(person: u16, x: u8, y: u8) -> RosterUnit {
        RosterUnit {
            person,
            x,
            y,
            requires_flag: None,
            class: Some(0),
            level: Some(3),
            ai_mode: Some(0),
            ai_param: Some(0),
            other: Vec::new(),
        }
    }

    /// A roster unit kept off the map until an event brings it in.
    fn hidden(person: u16, x: u8, y: u8) -> RosterUnit {
        RosterUnit {
            other: vec![1, 1],
            ..unit(person, x, y)
        }
    }

    fn setup(win: Option<u16>, turns: u8, player: Vec<RosterUnit>) -> Instr {
        instr(
            "battle_setup",
            Operands::BattleSetup {
                header: BattleHeader {
                    turn_limit: turns,
                    defeat_to_win: win,
                    lose_if_defeated: Some(0),
                    other: [0; 4],
                },
                units: player,
            },
        )
    }

    fn roster(units: Vec<RosterUnit>) -> Instr {
        instr(
            "battle_roster",
            Operands::Roster {
                friendly: false,
                units,
            },
        )
    }

    /// Two battles offered by a choice (setups for both first), then the battle on map 2 in two
    /// phases: group 3 (parallel) until person 300 falls, group 4 (parallel), group 5 after it.
    fn scene() -> Scene {
        let choice = Block {
            offset: 0,
            records: vec![
                record(
                    0,
                    0,
                    false,
                    [0; 6],
                    vec![setup(
                        Some(54),
                        25,
                        vec![unit(ANY_OFFICER, 1, 1), unit(0, 2, 2), unit(1, 3, 3)],
                    )],
                ),
                record(
                    0,
                    1,
                    false,
                    [0; 6],
                    vec![setup(Some(105), 30, vec![unit(0, 2, 2), unit(1, 3, 2)])],
                ),
            ],
        };
        let battle = Block {
            offset: 0,
            records: vec![
                // 0: the map and the enemies.
                record(
                    0,
                    0,
                    false,
                    [0; 6],
                    vec![
                        roster(vec![
                            unit(54, 9, 4),
                            unit(300, 8, 4),
                            hidden(302, 2, 2),
                            hidden(303, 5, 5),
                            unit(304, 7, 7),
                        ]),
                        fields("load_map", &[("map", 0x3002)]),
                    ],
                ),
                // 1: the battle starts.
                record(0, 1, false, [0; 6], vec![op("begin_battle")]),
                // 2: the opening makes person 304 hold its ground.
                record(
                    0,
                    2,
                    false,
                    [0; 6],
                    vec![fields(
                        "set_ai",
                        &[("person", 304), ("mode", 2), ("unused", 0)],
                    )],
                ),
                // 3–5: treasures and Liu Bei's objective tile.
                record(
                    UNIT_AT_CELL,
                    3,
                    true,
                    [0, 4, 5, 6, 0, 0],
                    vec![fields("data", &[("kind", 2), ("value", 100)])],
                ),
                record(
                    UNIT_AT_CELL,
                    3,
                    false,
                    [0, 4, 7, 1, 0, 0],
                    vec![fields("add_item", &[("item", 30)])],
                ),
                record(
                    UNIT_AT_CELL,
                    3,
                    false,
                    [0, 0, 3, 1, 0, 0],
                    vec![fields("data", &[("kind", 4), ("value", 50)])],
                ),
                // 6: a later roster (not converted).
                record(
                    9,
                    3,
                    false,
                    [5, 0, 0, 0, 0, 0],
                    vec![roster(vec![unit(301, 1, 1)])],
                ),
                // 7: Liu Bei in rows 2..=4, columns 3..=5 brings in person 302.
                record(
                    UNIT_IN_AREA,
                    3,
                    false,
                    [0, 0, 2, 3, 4, 5],
                    vec![fields("join_battle", &[("person", 302)])],
                ),
                // 8: Guan Yu's duel with 54, which the base battle tells itself.
                record(
                    4,
                    3,
                    false,
                    [1, 0, 54, 0, 0, 0],
                    vec![
                        fields("dialogue", &[("text", 0x10)]),
                        fields("duel", &[("first", 1), ("second", 54)]),
                        op("leave_parallel"),
                    ],
                ),
                // 9: 300 falls: the drawbridge comes down and the next phase begins.
                record(
                    12,
                    3,
                    false,
                    [44, 1, 0, 0, 0, 0],
                    vec![
                        fields("set_map_chip", &[("x", 3), ("y", 1), ("chip", 2)]),
                        fields("narration", &[("text", 0x30)]),
                        fields("set_flag", &[("flag", 90), ("clear", 0)]),
                        op("leave_parallel"),
                    ],
                ),
                // 10: turn 8 of the second phase: a duel, 54 heads for (2, 1), 303 arrives.
                record(
                    9,
                    4,
                    true,
                    [8, 0, 0, 0, 0, 0],
                    vec![
                        fields("dialogue", &[("text", 0x40)]),
                        fields("duel", &[("first", 1), ("second", 54)]),
                        fields("duel_action", &[("person", 1), ("action", 0)]),
                        fields("duel_action", &[("person", 54), ("action", 4)]),
                        op("duel_end"),
                        fields("add_levels", &[("person", 1), ("levels", 1)]),
                        fields("caption", &[("text", 0x50)]),
                        fields(
                            "set_ai",
                            &[("person", 54), ("mode", 4), ("p1", 2), ("p2", 1)],
                        ),
                        fields("join_battle", &[("person", 303)]),
                        fields("play_music", &[("song", 3)]),
                    ],
                ),
                // 11: the battle is won: the base battle's outro.
                record(7, 4, false, [0; 6], vec![op("leave_parallel")]),
                // 12: Liu Bei at (2, 1) in the second phase wins.
                record(
                    UNIT_AT_CELL,
                    4,
                    false,
                    [0, 0, 1, 2, 0, 0],
                    vec![
                        fields("data", &[("kind", 4), ("value", 50)]),
                        fields("battle_end", &[("next_map", 0x1000)]),
                    ],
                ),
                // 13: after the battle.
                record(
                    0,
                    5,
                    false,
                    [0; 6],
                    vec![
                        fields("data", &[("kind", 2), ("value", 500)]),
                        fields("battle_end", &[("next_map", 0x1000)]),
                    ],
                ),
            ],
        };
        Scene {
            blocks: vec![choice, battle],
        }
    }

    #[test]
    fn finds_the_setup_whose_target_is_in_the_roster() {
        let b = find_battle(&scene(), 2, &[], None).unwrap();
        assert_eq!(
            (b.block, b.header.turn_limit),
            (1, 25),
            "the setup that targets 54"
        );
        assert_eq!(b.player.len(), 3);
        assert_eq!(b.rosters.len(), 1);
        assert_eq!(b.later_rosters, 1);
        assert_eq!(b.records.len(), 14);
        assert_eq!(
            b.cells[..3],
            [
                // Trigger records hold the row first.
                CellRecord {
                    person: ANY_UNIT,
                    x: 6,
                    y: 5,
                    gold: 100,
                    item: None,
                    routine: false
                },
                CellRecord {
                    person: ANY_UNIT,
                    x: 1,
                    y: 7,
                    gold: 0,
                    item: Some(30),
                    routine: false
                },
                CellRecord {
                    person: 0,
                    x: 1,
                    y: 3,
                    gold: 0,
                    item: None,
                    routine: true
                },
            ]
        );
        let joins: Vec<_> = b
            .joins
            .iter()
            .map(|j| (j.record, j.group, j.kind, &j.persons[..]))
            .collect();
        assert_eq!(
            joins,
            [(7, 3, UNIT_IN_AREA, &[302][..]), (10, 4, 9, &[303][..])]
        );
        assert!(find_battle(&scene(), 3, &[], None).is_err());
    }

    #[test]
    fn phases_follow_the_trigger_groups() {
        let b = find_battle(&scene(), 2, &[], None).unwrap();
        let p = phases(&b.records);
        assert_eq!(p.len(), 3);
        assert_eq!(
            (p[0].parallel, p[0].records.len(), p[0].runs_only),
            (true, 7, false)
        );
        assert_eq!(
            (p[1].parallel, &p[1].records[..], p[1].runs_only),
            (true, &[10, 11, 12][..], false)
        );
        assert!(p[2].runs_only && p[2].ends_battle);
        let stages = [Some(0), Some(1), None];
        assert!(matches!(next_after(&p, &stages, 0), Next::Stage(1, ref w) if w.is_empty()));
        assert!(matches!(next_after(&p, &stages, 1), Next::Victory));
    }

    #[test]
    fn trigger_records() {
        let mut named = |p: u16| -> Result<Option<String>, String> {
            Ok((p != ANY_UNIT).then(|| format!("o{p}")))
        };
        assert_eq!(
            trigger_of(9, false, [7, 0, 0, 0, 0, 0], &mut named),
            Ok(Trigger::TurnStart {
                turn: 7,
                side: Side::Player
            })
        );
        assert_eq!(
            trigger_of(6, false, [0, 4, 12, 3, 0, 0], &mut named),
            Ok(Trigger::Reach {
                who: None,
                pos: Pos::new(3, 12),
                radius: 0,
                to: None
            })
        );
        assert_eq!(
            trigger_of(12, false, [89, 0, 0, 0, 0, 0], &mut named),
            Ok(Trigger::UnitDefeated {
                target: "o89".into()
            })
        );
        assert_eq!(
            trigger_of(4, false, [1, 0, 5, 0, 0, 0], &mut named),
            Ok(Trigger::Adjacent {
                a: Some("o1".into()),
                b: "o5".into()
            })
        );
        // Any player unit next to one (고성: contact with ???).
        assert_eq!(
            trigger_of(4, false, [0, 4, 5, 0, 0, 0], &mut named),
            Ok(Trigger::Adjacent {
                a: None,
                b: "o5".into()
            })
        );
        assert!(
            trigger_of(4, false, [1, 0, 0, 4, 0, 0], &mut named).is_err(),
            "the second unit is named"
        );
        assert!(
            trigger_of(12, false, [0, 4, 0, 0, 0, 0], &mut named).is_err(),
            "needs a unit"
        );
        assert!(
            trigger_of(9, true, [7, 0, 0, 0, 0, 0], &mut named).is_err(),
            "inverted"
        );
        assert!(trigger_of(2, false, [0; 6], &mut named).is_err());
    }

    #[test]
    fn ai_modes() {
        assert_eq!(ai_mode(1, 0), (AiMode::Aggressive, None, None));
        assert_eq!(ai_mode(2, 0), (AiMode::Hold, None, None));
        assert_eq!(ai_mode(3, 7), (AiMode::Target, Some(7), None));
        assert_eq!(
            ai_mode(4, 0x0D16),
            (AiMode::Advance, None, Some(Pos::new(22, 13)))
        );
        assert_eq!(ai_mode(5, 7), (AiMode::March, Some(7), None));
        assert_eq!(
            ai_mode(6, 0x0D16),
            (AiMode::March, None, Some(Pos::new(22, 13)))
        );
        assert_eq!(ai_mode(0, 0), (AiMode::Defensive, None, None));
    }

    #[test]
    fn drama_text_is_safe_for_the_parser() {
        let mut out = String::new();
        push_text(
            &mut out,
            "guan_yu:",
            "첫 줄\r\n  둘째 줄\n@명령 같은 줄\n\n- 목록 같은 줄",
        );
        push_text(&mut out, "@narr", "   ");
        assert_eq!(
            out,
            "guan_yu: 첫 줄\n    둘째 줄\nguan_yu: @명령 같은 줄\nguan_yu: - 목록 같은 줄\n"
        );
        let scenes = hero_core::script::parse_drama("t", &format!("== s\n{out}")).unwrap();
        assert_eq!(scenes[0].cmds.len(), 4, "three lines and the end");
        assert_eq!(free_speaker("공 손찬:"), "공손찬");
        assert_eq!(free_speaker("guard"), "???", "an id-like name is not free");
        assert_eq!(
            free_speaker("긴이름".repeat(10).as_str()).chars().count(),
            24
        );
    }

    fn base_battle() -> BattleDef {
        toml::from_str(
            r#"
id = "b"
name = "시험 전투"
objective = "적장을 물리쳐라"
turn_limit = 20
reward_gold = 100
intro = "b_intro"
victory = [{ type = "defeat_unit", target = "boss" }, { type = "reach", who = "liu_bei", pos = [1, 1] }]

[map]
rows = """
..
..
"""

[deploy]
max = 4
required = ["guan_yu"]
slots = [[0, 0], [1, 0], [0, 1], [1, 1]]

[[units]]
side = "enemy"
officer = "boss"
tag = "chief"
drop = "bean"
pos = [1, 1]

[[units]]
side = "enemy"
officer = "extra"
pos = [0, 1]

[[units]]
side = "enemy"
name = "복병"
class = "archer"
level = 2
group = "ambush"
pos = [0, 0]

[[events]]
trigger = { type = "adjacent", a = "guan_yu", b = "chief" }
actions = [{ type = "drama", scene = "duel" }]

[[events]]
trigger = { type = "unit_defeated", target = "extra" }
actions = [{ type = "drama", scene = "extra_falls" }]

[[events]]
trigger = { type = "turn_start", turn = 3 }
actions = [{ type = "spawn", group = "ambush" }]

[[treasures]]
pos = [0, 0]
item = "wine"
"#,
        )
        .unwrap()
    }

    fn names() -> Names {
        let mut n = Names::default();
        n.officers.insert(0, "liu_bei".into());
        n.officers.insert(1, "guan_yu".into());
        n.officers.insert(54, "boss".into());
        n.person_names.insert(300, "보병대".into());
        n.stats.insert(300, [40, 10, 35]);
        n.person_names.insert(9, "전령 갑".into());
        n.classes.insert(0, "short_infantry".into());
        n.items.insert(30, "bean".into());
        n.player_officers = ["liu_bei", "guan_yu"].map(String::from).into();
        n
    }

    /// The scene's text: dialogues and strings by offset.
    #[derive(Default)]
    struct Text {
        dialogues: BTreeMap<u16, Vec<(u16, String)>>,
        strings: BTreeMap<u16, String>,
    }

    impl TextSource for Text {
        fn dialogue(&self, offset: u16) -> Result<Vec<(u16, String)>, String> {
            self.dialogues
                .get(&offset)
                .cloned()
                .ok_or_else(|| format!("no dialogue at {offset:#x}"))
        }

        fn string(&self, offset: u16) -> Result<String, String> {
            self.strings
                .get(&offset)
                .cloned()
                .ok_or_else(|| format!("no string at {offset:#x}"))
        }
    }

    #[test]
    fn objective_texts_become_one_line() {
        assert_eq!(objective_text("1, [여포]의 괴멸"), "여포의 괴멸");
        assert_eq!(
            objective_text("\n1,적의 전멸 \r\n2,유비가 북서쪽 성채에 도달"),
            "적의 전멸 / 유비가 북서쪽 성채에 도달"
        );
        // A comma that is no list number stays.
        assert_eq!(
            objective_text("성문을 열고, 들어가라"),
            "성문을 열고, 들어가라"
        );
        assert_eq!(objective_text(" \r\n "), "");
        assert_eq!(objective_text("-1, [여포]의 괴멸"), "여포의 괴멸");
        assert_eq!(objective_text("10,[여포] 의 퇴각"), "여포의 퇴각");
    }

    fn text() -> Text {
        let mut t = Text::default();
        t.dialogues.insert(0x10, vec![(1, "결투다!".into())]);
        t.dialogues.insert(
            0x40,
            vec![
                (1, "첫 줄\n둘째 줄".into()),
                (54, "덤벼라".into()),
                (9, "큰일입니다".into()),
            ],
        );
        t.strings.insert(0x30, "다리가 내려왔다.".into());
        t.strings.insert(0x50, "관우는 레벨이 올라갔다!".into());
        t
    }

    fn converted() -> Converted {
        let orig = find_battle(&scene(), 2, &[], None).unwrap();
        let text = text();
        let mut cells = |pos: Pos, op: u8| -> Result<CellChange, String> {
            assert_eq!(op, 2);
            Ok(Some((
                "bridge".to_string(),
                Some(format!("cell_{}_{}", pos.x, pos.y)),
            )))
        };
        convert(
            &base_battle(),
            &orig,
            &names(),
            &pair("b", 1, 0, 2),
            "hexz_02",
            &mut EventSources {
                text: &text,
                cell_change: &mut cells,
            },
        )
        .unwrap()
    }

    #[test]
    fn converts_onto_the_original_map() {
        let c = converted();
        let b = &c.battle;
        assert_eq!(b.map.use_map.as_deref(), Some("hexz_02"));
        assert!(!b.map.has_own_content());
        assert_eq!((b.turn_limit, b.intro.as_deref()), (25, Some("b_intro")));
        // Guan Yu (required) and Liu Bei first, as the engine fills the tiles.
        assert_eq!(
            b.deploy.slots,
            [Pos::new(3, 3), Pos::new(2, 2), Pos::new(1, 1)]
        );
        assert_eq!(b.deploy.max, 3, "lowered to the original slots");

        assert_eq!(b.units.len(), 5);
        let boss = &b.units[0];
        assert_eq!(boss.officer.as_deref(), Some("boss"));
        assert_eq!(
            (boss.tag.as_deref(), boss.drop.as_deref()),
            (Some("chief"), Some("bean"))
        );
        assert!(boss.commander, "the header's victory officer");
        assert_eq!(boss.pos, Pos::new(9, 4));
        assert_eq!(boss.class.as_deref(), Some("short_infantry"));
        let generic = &b.units[1];
        assert_eq!(
            (generic.officer.as_ref(), generic.name.as_deref()),
            (None, Some("보병대"))
        );
        assert_eq!(generic.ai, AiMode::Defensive);
        assert_eq!(generic.stats, Some([40, 10, 35]), "its person's stats");
        assert_eq!(
            generic.tag.as_deref(),
            Some("person_300"),
            "named by an event"
        );
        // The hidden units wait for the records that bring them in.
        assert_eq!(b.units[2].group.as_deref(), Some("original_7"));
        assert_eq!(b.units[3].group.as_deref(), Some("original_10"));
        assert_eq!(
            b.units[4].ai,
            AiMode::Hold,
            "the opening's AI (mode 2, 부동)"
        );

        assert_eq!(
            b.victory,
            [
                Condition::DefeatUnit {
                    target: "boss".into()
                },
                Condition::Reach {
                    who: Some("liu_bei".into()),
                    pos: Pos::new(1, 3),
                    radius: 0,
                    to: None,
                },
            ]
        );
        assert_eq!(
            b.treasures,
            [
                TreasureDef {
                    pos: Pos::new(6, 5),
                    item: None,
                    gold: 100
                },
                TreasureDef {
                    pos: Pos::new(1, 7),
                    item: Some("bean".into()),
                    gold: 0
                },
            ]
        );
        let notes = c.notes.join("\n");
        assert!(
            notes.contains("base unit `extra` is not in the original battle"),
            "{notes}"
        );
        assert!(
            !notes.contains("복병"),
            "generic base units are not listed: {notes}"
        );
        assert!(notes.contains("roster(s) loaded later"), "{notes}");
        assert!(notes.contains("ambush"), "{notes}");
        // Round trip through the battle file format.
        let text = toml::to_string(b).unwrap();
        let back: BattleDef = toml::from_str(&text).unwrap();
        assert_eq!(&back, b);
    }

    #[test]
    fn mid_battle_events_follow_the_phases() {
        let c = converted();
        let e = &c.battle.events;
        // The base duel stays (Guan Yu next to `chief`, the tag of officer `boss`), the base
        // events naming the missing officer or the base map's group go; record 8, the same duel,
        // is left out, and the base event takes over its end of the first phase.
        assert!(matches!(e[0].trigger, Trigger::Adjacent { .. }));
        assert_eq!(e[0].stage, Some(0));
        assert_eq!(
            e[0].actions,
            [
                EventAction::Drama {
                    scene: "duel".into()
                },
                EventAction::SetStage { stage: 1 }
            ]
        );
        let notes = c.notes.join("\n");
        assert!(
            notes.contains("record 8: the base battle's event"),
            "{notes}"
        );
        assert!(
            notes.contains("not converted: the original's victory and defeat scripts"),
            "{notes}"
        );
        let original: Vec<&EventDef> = e[1..].iter().collect();
        assert_eq!(original.len(), 4, "{original:#?}");
        // Record 7: Liu Bei in the rectangle brings in 302.
        assert_eq!(
            *original[0],
            EventDef {
                trigger: Trigger::Reach {
                    who: Some("liu_bei".into()),
                    pos: Pos::new(3, 2),
                    radius: 0,
                    to: Some(Pos::new(5, 4)),
                },
                once: true,
                stage: Some(0),
                when: Vec::new(),
                unless: Vec::new(),
                actions: vec![EventAction::Spawn {
                    group: "original_7".into()
                }],
            }
        );
        // Record 9: the drawbridge, the narration, then the next phase.
        assert_eq!(
            *original[1],
            EventDef {
                trigger: Trigger::UnitDefeated {
                    target: "person_300".into()
                },
                once: true,
                stage: Some(0),
                when: Vec::new(),
                unless: Vec::new(),
                actions: vec![
                    EventAction::SetTerrain {
                        pos: Pos::new(3, 1),
                        terrain: "bridge".into(),
                        image: Some("cell_3_1".into()),
                    },
                    EventAction::Drama {
                        scene: "orig_b_9".into()
                    },
                    EventAction::SetStage { stage: 1 },
                ],
            }
        );
        // Record 10: dialogue and duel as a drama, then the level-up (its caption is the game's
        // own), the AI change and the arrival.
        assert_eq!(
            *original[2],
            EventDef {
                trigger: Trigger::TurnStart {
                    turn: 8,
                    side: Side::Player
                },
                once: true,
                stage: Some(1),
                when: Vec::new(),
                unless: Vec::new(),
                actions: vec![
                    EventAction::Drama {
                        scene: "orig_b_10".into()
                    },
                    EventAction::LevelUp {
                        target: "guan_yu".into(),
                        amount: 1
                    },
                    EventAction::SetAi {
                        target: "boss".into(),
                        ai: AiMode::Advance,
                        ai_target: None,
                        ai_pos: Some(Pos::new(2, 1)),
                    },
                    EventAction::Spawn {
                        group: "original_10".into()
                    },
                ],
            }
        );
        // Record 12: the second phase's objective wins the battle.
        assert_eq!(
            *original[3],
            EventDef {
                trigger: Trigger::Reach {
                    who: Some("liu_bei".into()),
                    pos: Pos::new(2, 1),
                    radius: 0,
                    to: None,
                },
                once: true,
                stage: Some(1),
                when: Vec::new(),
                unless: Vec::new(),
                actions: vec![EventAction::Victory],
            }
        );
        assert_eq!(
            c.drama,
            "\n== orig_b_9\n@narr 다리가 내려왔다.\n@hide all\n\
             \n== orig_b_10\nguan_yu: 첫 줄\n    둘째 줄\nboss: 덤벼라\n전령갑: 큰일입니다\n\
             @duel guan_yu boss terrain\n@duel_act left charge\n@duel_act left strike 4\n\
             @duel_act right flee\n@duel_end\n@hide all\n"
        );
        let scenes = hero_core::script::parse_drama("t", &c.drama).unwrap();
        assert_eq!(scenes.len(), 2);
    }

    /// Records that leave a phase share the script on the way (one scene, written once), and a
    /// base event that a later phase's record joins fires in that phase only.
    #[test]
    fn phase_changes_share_the_way_and_keep_their_stage() {
        let mut scene = scene();
        scene.blocks[1].records = vec![
            record(
                0,
                0,
                false,
                [0; 6],
                vec![
                    roster(vec![unit(54, 9, 4), unit(300, 8, 4)]),
                    fields("load_map", &[("map", 0x3002)]),
                ],
            ),
            record(0, 1, false, [0; 6], vec![op("begin_battle")]),
            // Phase 0 (not parallel): turn 2 or 300 falls, whichever comes first.
            record(
                9,
                3,
                false,
                [2, 0, 0, 0, 0, 0],
                vec![fields("dialogue", &[("text", 0x10)])],
            ),
            record(
                12,
                3,
                false,
                [44, 1, 0, 0, 0, 0],
                vec![fields("narration", &[("text", 0x30)])],
            ),
            // On the way to phase 1.
            record(
                0,
                4,
                false,
                [0; 6],
                vec![fields("dialogue", &[("text", 0x40)])],
            ),
            // Phase 1: Guan Yu next to 54 (the base battle's duel) also makes 54 retreat.
            record(
                4,
                5,
                true,
                [1, 0, 54, 0, 0, 0],
                vec![fields("remove_person", &[("person", 54)])],
            ),
            record(
                9,
                5,
                false,
                [9, 0, 0, 0, 0, 0],
                vec![fields("narration", &[("text", 0x50)])],
            ),
        ];
        let orig = find_battle(&scene, 2, &[], None).unwrap();
        let text = text();
        let mut none = |_: Pos, _: u8| -> Result<CellChange, String> { Ok(None) };
        let c = convert(
            &base_battle(),
            &orig,
            &names(),
            &pair("b", 1, 0, 2),
            "hexz_02",
            &mut EventSources {
                text: &text,
                cell_change: &mut none,
            },
        )
        .unwrap();
        let way = [
            EventAction::SetStage { stage: 1 },
            EventAction::Drama {
                scene: "orig_b_4".into(),
            },
        ];
        let staged: Vec<&EventDef> = c
            .battle
            .events
            .iter()
            .filter(|e| e.stage == Some(0))
            .collect();
        assert_eq!(staged.len(), 2, "{:#?}", c.battle.events);
        for e in staged {
            assert!(e.actions.ends_with(&way), "{e:#?}");
        }
        assert_eq!(c.drama.matches("== orig_b_4\n").count(), 1, "{}", c.drama);
        hero_core::script::parse_drama("t", &c.drama).expect("scene ids are unique");
        let duel = &c.battle.events[0];
        assert!(matches!(duel.trigger, Trigger::Adjacent { .. }));
        assert_eq!(duel.stage, Some(1));
        assert!(duel.actions.contains(&EventAction::Retreat {
            target: "boss".into()
        }));
    }

    /// Without the scene's text the battle is still converted, with its dialogue left out.
    #[test]
    fn missing_text_leaves_only_the_dialogue_out() {
        struct Missing;
        impl TextSource for Missing {
            fn dialogue(&self, _: u16) -> Result<Vec<(u16, String)>, String> {
                Err("dialogue left out: SNR1M.R3 missing".into())
            }
            fn string(&self, _: u16) -> Result<String, String> {
                Err("text left out: SNR1M.R3 missing".into())
            }
        }
        let orig = find_battle(&scene(), 2, &[], None).unwrap();
        let mut cells = |_: Pos, _: u8| -> Result<CellChange, String> { Ok(None) };
        let c = convert(
            &base_battle(),
            &orig,
            &names(),
            &pair("b", 1, 0, 2),
            "hexz_02",
            &mut EventSources {
                text: &Missing,
                cell_change: &mut cells,
            },
        )
        .unwrap();
        // The duel's portraits and sounds stay; no line of dialogue does.
        assert!(!c.drama.contains(": "), "{}", c.drama);
        assert!(c
            .battle
            .events
            .iter()
            .any(|e| e.actions.contains(&EventAction::SetStage { stage: 1 })));
        assert!(
            c.notes.iter().any(|n| n.contains("SNR1M.R3 missing")),
            "{:?}",
            c.notes
        );
    }

    /// A record's part guarded by a battle flag is told where the script tells it, between the
    /// lines before and after it, and the battle moves to the next stage only after all of it
    /// (moved on first, the guarded part of the stage it left would not fire).
    #[test]
    fn a_guarded_part_of_a_record_is_told_in_its_place_before_the_stage_moves_on() {
        let mut scene = scene();
        // Record 9 says one more line unless flag 91 is set, which record 7 sets.
        let code = &mut scene.blocks[1].records[9].code;
        code.insert(2, guard(1, vec![], vec![91]));
        code.insert(3, fields("narration", &[("text", 0x30)]));
        scene.blocks[1].records[7]
            .code
            .push(fields("set_flag", &[("flag", 91), ("clear", 0)]));
        let orig = find_battle(&scene, 2, &[], None).unwrap();
        let text = text();
        let mut none = |_: Pos, _: u8| -> Result<CellChange, String> { Ok(None) };
        let c = convert(
            &base_battle(),
            &orig,
            &names(),
            &pair("b", 1, 0, 2),
            "hexz_02",
            &mut EventSources {
                text: &text,
                cell_change: &mut none,
            },
        )
        .unwrap();
        let rec9: Vec<&EventDef> = c
            .battle
            .events
            .iter()
            .filter(|e| {
                e.trigger
                    == Trigger::UnitDefeated {
                        target: "person_300".into(),
                    }
            })
            .collect();
        let drama = |scene: &str| EventAction::Drama {
            scene: scene.into(),
        };
        let shape: Vec<(bool, &[EventAction])> = rec9
            .iter()
            .map(|e| (e.when.is_empty(), &e.actions[..]))
            .collect();
        assert_eq!(shape.len(), 3, "{rec9:#?}");
        assert_eq!(shape[0], (true, &[drama("orig_b_9")][..]));
        assert_eq!(shape[1], (false, &[drama("orig_b_9_2")][..]));
        // The stage moves on once, after the guarded part.
        assert!(shape[2].0);
        assert_eq!(
            shape[2].1.first(),
            Some(&EventAction::SetStage { stage: 1 })
        );
        assert!(rec9.iter().all(|e| e.stage == Some(0)));
    }

    #[test]
    fn route_flags_decide_the_conditions() {
        let mut scene = scene();
        // Record 7 speaks only on the route of flag 133, and not once flag 90 is set.
        scene.blocks[1].records[7].code.insert(
            0,
            instr(
                "if_flags",
                Operands::Condition {
                    skip: 1,
                    all_set: vec![133],
                    all_clear: vec![90],
                },
            ),
        );
        scene.blocks[1].records[7]
            .code
            .insert(1, fields("narration", &[("text", 0x30)]));
        let text = text();
        let mut none = |_: Pos, _: u8| -> Result<CellChange, String> { Ok(None) };
        let orig = find_battle(&scene, 2, &[], None).unwrap();
        let run = |pairing: &Pairing,
                   none: &mut dyn FnMut(Pos, u8) -> Result<CellChange, String>| {
            convert(
                &base_battle(),
                &orig,
                &names(),
                pairing,
                "hexz_02",
                &mut EventSources {
                    text: &text,
                    cell_change: none,
                },
            )
            .unwrap()
        };
        let off_route = run(&pair("b", 1, 0, 2), &mut none);
        assert!(!off_route.drama.contains("orig_b_7"), "{}", off_route.drama);
        let on_route = run(
            &Pairing {
                flags: &[133],
                ..pair("b", 1, 0, 2)
            },
            &mut none,
        );
        assert!(
            on_route.drama.contains("== orig_b_7\n@narr"),
            "{}",
            on_route.drama
        );
        // Flag 90 is set by record 9 and tested by record 7: a battle flag and a condition. Only
        // the narration it guards waits for it; the arrival does not. The narration comes first,
        // as in the script.
        let events = &on_route.battle.events;
        let rec7: Vec<&EventDef> = events
            .iter()
            .filter(|e| matches!(e.trigger, Trigger::Reach { to: Some(_), .. }))
            .collect();
        assert_eq!(rec7.len(), 2, "{rec7:#?}");
        assert_eq!(
            rec7[0].when,
            [FlagCond {
                flag: "orig_b_90".into(),
                cmp: Compare::Eq,
                value: 0
            }]
        );
        assert_eq!(
            rec7[0].actions,
            [EventAction::Drama {
                scene: "orig_b_7".into()
            }]
        );
        assert_eq!(rec7[0].stage, Some(0));
        assert_eq!(
            (&rec7[1].when[..], &rec7[1].actions[..]),
            (
                &[][..],
                &[EventAction::Spawn {
                    group: "original_7".into()
                }][..]
            )
        );
        assert_eq!(rec7[1].stage, Some(0));
        assert!(events
            .iter()
            .any(|e| e.actions.contains(&EventAction::SetFlag {
                flag: "orig_b_90".into(),
                value: 1
            })));
        assert!(
            off_route.battle.events.iter().all(|e| e.when.is_empty()),
            "the route decides first"
        );
        // A cell the operation leaves alone changes nothing.
        assert!(!on_route
            .battle
            .events
            .iter()
            .flat_map(|e| &e.actions)
            .any(|a| matches!(a, EventAction::SetTerrain { .. })));
    }

    /// What a script does past a part guarded by shared flags that ends it runs only while the
    /// flags do not all hold: an event with `unless`. An objective text becomes `set_objective`.
    #[test]
    fn the_part_past_a_guarded_end_fires_unless_the_flags_hold() {
        let mut scene = scene();
        // Record 7, on the route of flag 133: while flag 90 is set it narrates and ends the
        // phase; otherwise it narrates something else and changes the objective.
        let code = &mut scene.blocks[1].records[7].code;
        code.insert(
            0,
            instr(
                "if_flags",
                Operands::Condition {
                    skip: 2,
                    all_set: vec![90],
                    all_clear: vec![],
                },
            ),
        );
        code.insert(1, fields("narration", &[("text", 0x30)]));
        code.insert(2, op("leave_parallel"));
        code.insert(3, fields("narration", &[("text", 0x50)]));
        code.insert(4, fields("set_objective", &[("text", 0x60)]));
        let mut text = text();
        text.strings
            .insert(0x60, "1, [장수]를 설득\r\n2,성문 도달".into());
        let mut none = |_: Pos, _: u8| -> Result<CellChange, String> { Ok(None) };
        let orig = find_battle(&scene, 2, &[], None).unwrap();
        let converted = convert(
            &base_battle(),
            &orig,
            &names(),
            &Pairing {
                flags: &[133],
                ..pair("b", 1, 0, 2)
            },
            "hexz_02",
            &mut EventSources {
                text: &text,
                cell_change: &mut none,
            },
        )
        .unwrap();
        let flag90 = FlagCond {
            flag: "orig_b_90".into(),
            cmp: Compare::Ne,
            value: 0,
        };
        let rec7: Vec<&EventDef> = converted
            .battle
            .events
            .iter()
            .filter(|e| matches!(e.trigger, Trigger::Reach { to: Some(_), .. }))
            .collect();
        let guarded = rec7
            .iter()
            .find(|e| e.when.contains(&flag90))
            .expect("the guarded part");
        assert!(guarded.unless.is_empty());
        let otherwise = rec7
            .iter()
            .find(|e| e.unless.contains(&flag90))
            .expect("the part past it");
        assert!(otherwise.when.is_empty(), "{otherwise:#?}");
        assert!(
            otherwise.actions.contains(&EventAction::SetObjective {
                text: "장수를 설득 / 성문 도달".into()
            }),
            "{otherwise:#?}"
        );
        assert!(
            !converted
                .notes
                .iter()
                .any(|n| n.contains("do not hold is left out")),
            "{:?}",
            converted.notes
        );
    }

    /// Convert scene 1 with `code` in front of record 7's script (the route of flag 133);
    /// record 9, which sets flag 90, sets flag 91 too.
    fn convert_record7(code: Vec<Instr>) -> Converted {
        let mut scene = scene();
        scene.blocks[1].records[7].code.splice(0..0, code);
        scene.blocks[1].records[9]
            .code
            .insert(0, fields("set_flag", &[("flag", 91), ("clear", 0)]));
        let text = text();
        let mut none = |_: Pos, _: u8| -> Result<CellChange, String> { Ok(None) };
        let orig = find_battle(&scene, 2, &[], None).unwrap();
        convert(
            &base_battle(),
            &orig,
            &names(),
            &Pairing {
                flags: &[133],
                ..pair("b", 1, 0, 2)
            },
            "hexz_02",
            &mut EventSources {
                text: &text,
                cell_change: &mut none,
            },
        )
        .unwrap()
    }

    fn guard(skip: u8, all_set: Vec<u8>, all_clear: Vec<u8>) -> Instr {
        instr(
            "if_flags",
            Operands::Condition {
                skip,
                all_set,
                all_clear,
            },
        )
    }

    /// A town whose setup depends on flag 38 (who must not fall: person 132 while it is clear,
    /// 53 once set; a guest slot that needs flag 1), then the battle on map 2 whose enemy army
    /// depends on flag 133, and after it a record that sets up the next battle (213).
    pub(crate) fn route_scene() -> Scene {
        let flagged = |person, x, flag| RosterUnit {
            requires_flag: Some(flag),
            ..unit(person, x, 0)
        };
        let town = Block {
            offset: 0,
            records: vec![record(
                0,
                0,
                false,
                [0; 6],
                vec![
                    guard(1, vec![], vec![38]),
                    setup(
                        Some(54),
                        30,
                        vec![unit(0, 1, 1), unit(132, 2, 1), flagged(12, 3, 1)],
                    ),
                    guard(1, vec![38], vec![]),
                    setup(Some(54), 40, vec![unit(0, 1, 1), unit(53, 2, 1)]),
                ],
            )],
        };
        let battle = Block {
            offset: 0,
            records: vec![
                record(
                    0,
                    0,
                    false,
                    [0; 6],
                    vec![
                        guard(1, vec![], vec![133]),
                        roster(vec![unit(54, 9, 4), unit(300, 8, 4)]),
                        guard(1, vec![133], vec![]),
                        roster(vec![unit(54, 9, 4), unit(301, 8, 5)]),
                        fields("load_map", &[("map", 0x3002)]),
                    ],
                ),
                record(0, 1, false, [0; 6], vec![op("begin_battle")]),
                record(0, 4, false, [0; 6], vec![setup(Some(213), 30, vec![])]),
            ],
        };
        Scene {
            blocks: vec![town, battle],
        }
    }

    /// A tile any unit steps on is a treasure only when its script gives gold or an item;
    /// anything else (Xuchang's wall bringing Huang Zhong in) is an event (issue #86).
    #[test]
    fn only_gold_or_an_item_on_a_tile_is_a_treasure() {
        let cell = |code| record(UNIT_AT_CELL, 3, false, [0, 4, 5, 6, 0, 0], code);
        assert!(is_treasure(&cell(vec![fields("add_item", &[("item", 3)])])));
        assert!(is_treasure(&cell(vec![fields(
            "data",
            &[("kind", DATA_GOLD), ("value", 100)]
        )])));
        assert!(!is_treasure(&cell(vec![
            fields("dialogue", &[("text", 1)]),
            fields("join_battle", &[("person", 169)])
        ])));
        // Liu Bei's tile is no treasure either.
        let liu_bei = record(
            UNIT_AT_CELL,
            3,
            false,
            [0, 0, 5, 6, 0, 0],
            vec![fields("add_item", &[("item", 3)])],
        );
        assert!(!is_treasure(&liu_bei));
    }

    /// The setup and rosters are the ones the flags' `if_flags` let run; slots that need a flag
    /// are there when it is set; the flags they depend on are the battle's route flags.
    #[test]
    fn route_flags_pick_the_setup_and_the_rosters() {
        let scene = route_scene();
        let clear = find_battle(&scene, 2, &[], None).unwrap();
        assert_eq!(clear.route_flags, BTreeSet::from([1, 38, 133]));
        assert_eq!(clear.header.turn_limit, 30);
        let persons = |b: &OriginalBattle| -> Vec<u16> {
            b.rosters
                .iter()
                .flat_map(|(_, u)| u.iter().map(|u| u.person))
                .collect()
        };
        assert_eq!(persons(&clear), [54, 300]);
        assert_eq!(
            clear.player.iter().map(|u| u.person).collect::<Vec<_>>(),
            [0, 132]
        );
        assert_eq!(clear.flagged_slots_left_out, 1);
        assert_eq!(clear.other_route_rosters, 1);

        let set = find_battle(&scene, 2, &[1, 38, 133], None).unwrap();
        assert_eq!(set.header.turn_limit, 40);
        assert_eq!(persons(&set), [54, 301]);
        assert_eq!(
            set.player.iter().map(|u| u.person).collect::<Vec<_>>(),
            [0, 53]
        );
        let guest = find_battle(&scene, 2, &[1], None).unwrap();
        assert_eq!(
            guest
                .player
                .iter()
                .map(|u| (u.person, u.requires_flag))
                .collect::<Vec<_>>(),
            [(0, None), (132, None), (12, None)]
        );
        assert_eq!(guest.flagged_slots_left_out, 0);
    }

    /// A route that skips the battle's setups in the last block that has one fights another
    /// battle there (Xuchang's town on flag 89), even when an earlier block has a setup that
    /// would fit; a setup after the record that loads the map is the next battle's.
    #[test]
    fn a_route_that_skips_the_battles_setups_does_not_fight_it() {
        let mut scene = route_scene();
        let earlier = Block {
            offset: 0,
            records: vec![record(
                0,
                0,
                false,
                [0; 6],
                vec![setup(Some(54), 60, vec![unit(0, 1, 1)])],
            )],
        };
        scene.blocks.insert(0, earlier);
        scene.blocks[1].records[0].code = vec![
            guard(1, vec![], vec![89]),
            setup(Some(54), 30, vec![unit(0, 1, 1)]),
            guard(1, vec![89], vec![]),
            setup(Some(213), 30, vec![unit(0, 1, 1)]),
        ];
        assert_eq!(
            find_battle(&scene, 2, &[], None).unwrap().header.turn_limit,
            30
        );
        let err = find_battle(&scene, 2, &[89], None).unwrap_err();
        assert!(
            err.contains("skips the battle's setups in block 1"),
            "{err}"
        );
    }

    /// A guard on a clear flag negates to "unless it is clear"; a guarded part that ends only
    /// on flags of its own leaves what follows out, with a note and no stray scene.
    #[test]
    fn unless_parts_negate_clear_flags_and_skip_nested_tests() {
        let clear90 = FlagCond {
            flag: "orig_b_90".into(),
            cmp: Compare::Eq,
            value: 0,
        };
        let converted = convert_record7(vec![
            guard(2, vec![], vec![90]),
            fields("narration", &[("text", 0x30)]),
            op("leave_parallel"),
            fields("narration", &[("text", 0x50)]),
        ]);
        assert!(
            converted
                .battle
                .events
                .iter()
                .any(|e| e.unless == [clear90.clone()]),
            "{:#?}",
            converted.battle.events
        );

        // Nested: while 90 is set, a test of 91 ends the script; past the outer test a line.
        let converted = convert_record7(vec![
            guard(3, vec![90], vec![]),
            guard(2, vec![91], vec![]),
            fields("narration", &[("text", 0x30)]),
            op("leave_parallel"),
            fields("narration", &[("text", 0x50)]),
        ]);
        assert!(
            converted
                .notes
                .iter()
                .any(|n| n.contains("do not hold is left out")),
            "{:?}",
            converted.notes
        );
        assert!(converted.battle.events.iter().all(|e| e.unless.is_empty()));
        assert!(
            !converted.drama.contains("관우는 레벨이 올라갔다"),
            "{}",
            converted.drama
        );
    }

    /// A battle that goes on with another battle map (Changban): the first battle's setup is in
    /// the block before; the phase of the first map ends when the civilian (person 344) stands
    /// on tile (5, 4) or the battle is won; then a record sets the second battle up (its own
    /// setup and roster, the jump to map 3), and that battle starts, opens, has a phase and
    /// an epilogue.
    fn two_map_scene() -> Scene {
        let prep = Block {
            offset: 0,
            records: vec![record(
                0,
                0,
                false,
                [0; 6],
                vec![setup(
                    Some(54),
                    70,
                    vec![unit(0, 1, 1), unit(ANY_OFFICER, 2, 2), unit(344, 3, 3)],
                )],
            )],
        };
        let battle = Block {
            offset: 0,
            records: vec![
                // 0: the map and the enemies: 300 goes for the civilian.
                record(
                    0,
                    0,
                    false,
                    [0; 6],
                    vec![
                        roster(vec![
                            unit(54, 9, 4),
                            RosterUnit {
                                ai_mode: Some(3),
                                ai_param: Some(344),
                                ..unit(300, 8, 4)
                            },
                        ]),
                        fields("load_map", &[("map", 0x3002)]),
                    ],
                ),
                record(0, 1, false, [0; 6], vec![op("begin_battle")]),
                // 2: the opening sends the civilian to (5, 4).
                record(
                    0,
                    2,
                    false,
                    [0; 6],
                    vec![fields(
                        "set_ai",
                        &[("person", 344), ("mode", 6), ("p1", 5), ("p2", 4)],
                    )],
                ),
                // 3: the civilian reaches it (row 4, column 5): the phase is over. 4: so is it
                // when the battle is won.
                record(
                    UNIT_AT_CELL,
                    3,
                    true,
                    [88, 1, 4, 5, 0, 0],
                    vec![fields("dialogue", &[("text", 0x10)]), op("leave_parallel")],
                ),
                record(BATTLE_WON, 3, false, [0; 6], vec![op("leave_parallel")]),
                // 5: the second battle's setup, roster and map.
                record(
                    0,
                    4,
                    false,
                    [0; 6],
                    vec![
                        setup(Some(54), 99, vec![unit(0, 1, 1), unit(344, 0, 1)]),
                        roster(vec![unit(54, 7, 1)]),
                        fields("battle_end", &[("next_map", 0x3003)]),
                    ],
                ),
                record(0, 5, false, [0; 6], vec![op("begin_battle")]),
                // 7: its opening sends the civilian to (0, 5).
                record(
                    0,
                    6,
                    false,
                    [0; 6],
                    vec![fields(
                        "set_ai",
                        &[("person", 344), ("mode", 6), ("p1", 0), ("p2", 5)],
                    )],
                ),
                // 8: reaching it (row 5, column 0) ends the phase, and so does winning.
                record(
                    UNIT_AT_CELL,
                    7,
                    true,
                    [88, 1, 5, 0, 0, 0],
                    vec![op("leave_parallel")],
                ),
                record(BATTLE_WON, 7, false, [0; 6], vec![op("leave_parallel")]),
                // 10: after the battle.
                record(
                    0,
                    8,
                    false,
                    [0; 6],
                    vec![
                        fields("data", &[("kind", 2), ("value", 700)]),
                        fields("battle_end", &[("next_map", 0x1000)]),
                    ],
                ),
            ],
        };
        Scene {
            blocks: vec![prep, battle],
        }
    }

    fn convert_leg(scene: &Scene, map: u8, leg: u8, id: &str) -> Converted {
        let orig = find_battle_leg(scene, map, &[], Some(1), leg).unwrap();
        let mut names = names();
        names.civilians.insert(344, ("civilian".into(), 1));
        names.person_names.insert(344, "민중".into());
        let text = text();
        let mut none = |_: Pos, _: u8| -> Result<CellChange, String> { Ok(None) };
        convert(
            &crate::chapters::chapter_base(id, "시험", "목표", 70, true, Some("liu_bei")),
            &orig,
            &names,
            &pair("", 1, 0, map),
            &format!("hexz_{map:02}"),
            &mut EventSources {
                text: &text,
                cell_change: &mut none,
            },
        )
        .unwrap()
    }

    #[test]
    fn a_battle_on_two_maps_is_found_as_two_legs() {
        let scene = two_map_scene();
        assert_eq!(
            battle_map_leg(&scene, 1),
            Some(MapLeg {
                record: 5,
                next_map: 3
            })
        );
        assert_eq!(battle_map_leg(&scene, 0), None);
        // The first leg: the setup before it (not the later one), only its own records.
        let first = find_battle_leg(&scene, 2, &[], Some(1), 0).unwrap();
        assert_eq!((first.header.turn_limit, first.records.len()), (70, 5));
        assert_eq!((first.later_rosters, first.rosters.len()), (0, 1));
        // The second: its own setup and roster, its groups counted from them.
        let second = find_battle_leg(&scene, 3, &[], Some(1), 1).unwrap();
        assert_eq!(second.header.turn_limit, 99);
        assert_eq!((second.later_rosters, second.rosters.len()), (0, 1));
        assert_eq!(second.rosters[0].1[0].x, 7);
        let groups: Vec<u8> = second.records.iter().map(|r| r.trigger.group).collect();
        assert_eq!(groups, [0, 1, 2, 3, 3, 4]);
        // Only the map the battle goes on with is a second leg.
        assert!(find_battle_leg(&scene, 9, &[], Some(1), 1).is_err());
        assert!(find_battle_leg(&scene, 3, &[], None, 1).is_err());
        // (The battle's own map is the one its `load_map` names.)
        assert!(find_battle_leg(&scene, 3, &[], Some(1), 0).is_err());
    }

    /// A civilian in the setup is a fixed allied unit that the opening marches to a tile, whom
    /// enemies may hunt, and whose arrival ends the first leg (a script that leaves its phase).
    #[test]
    fn the_civilians_of_a_setup_are_escorted_allies() {
        let scene = two_map_scene();
        let c = convert_leg(&scene, 2, 0, "c");
        let b = &c.battle;
        assert_eq!(b.turn_limit, 70);
        let civilian = b.units.iter().find(|u| u.side == Side::Ally).unwrap();
        assert_eq!(
            (
                civilian.class.as_deref(),
                civilian.level,
                civilian.name.as_deref()
            ),
            (Some("civilian"), Some(1), Some("민중"))
        );
        assert_eq!(civilian.pos, Pos::new(3, 3));
        assert_eq!(
            (civilian.ai, civilian.ai_pos),
            (AiMode::March, Some(Pos::new(5, 4)))
        );
        assert_eq!(civilian.tag.as_deref(), Some("person_344"));
        // The enemy that hunts it names it.
        let hunter = b
            .units
            .iter()
            .find(|u| u.name.as_deref() == Some("보병대"))
            .unwrap();
        assert_eq!(
            (hunter.ai, hunter.ai_target.as_deref()),
            (AiMode::Target, Some("person_344"))
        );
        // Its tile is no deploy tile.
        assert_eq!(b.deploy.slots, [Pos::new(1, 1), Pos::new(2, 2)]);
        // Reaching (5, 4) plays the record's lines, tells the outro an event ended the battle
        // and wins.
        assert_eq!(b.events.len(), 1, "{:#?}", b.events);
        assert_eq!(
            b.events[0].trigger,
            Trigger::Reach {
                who: Some("person_344".into()),
                pos: Pos::new(5, 4),
                radius: 0,
                to: None
            }
        );
        assert_eq!(
            b.events[0].actions,
            [
                EventAction::Drama {
                    scene: "orig_c_3".into()
                },
                EventAction::SetFlag {
                    flag: ended_flag("c"),
                    value: 1
                },
                EventAction::Victory
            ]
        );
        assert!(
            !c.notes.iter().any(|n| n.contains("later")),
            "{:?}",
            c.notes
        );
        // The second leg: the civilian is there again, marching to (0, 5).
        let c = convert_leg(&scene, 3, 1, "c_2");
        let civilian = c
            .battle
            .units
            .iter()
            .find(|u| u.side == Side::Ally)
            .unwrap();
        assert_eq!(civilian.pos, Pos::new(0, 1));
        assert_eq!(civilian.ai_pos, Some(Pos::new(0, 5)));
        assert_eq!(c.battle.turn_limit, 99);
        assert!(c.battle.events.iter().any(|e| e.trigger
            == Trigger::Reach {
                who: Some("person_344".into()),
                pos: Pos::new(0, 5),
                radius: 0,
                to: None
            }
            && e.actions.last() == Some(&EventAction::Victory)));
    }

    /// A chapter battle's opening (group 2) plays when the battle begins: its lines, a duel
    /// loser's retreat and the flags it sets, in order; not what the setup's units already have
    /// (the AI, who is on the field, the objective).
    #[test]
    fn the_opening_of_a_chapter_battle_plays_when_it_begins() {
        let prep = Block {
            offset: 0,
            records: vec![record(
                0,
                0,
                false,
                [0; 6],
                vec![setup(Some(54), 25, vec![unit(0, 1, 1)])],
            )],
        };
        let battle = Block {
            offset: 0,
            records: vec![
                record(
                    0,
                    0,
                    false,
                    [0; 6],
                    vec![
                        roster(vec![unit(54, 9, 4), unit(300, 8, 4), unit(304, 7, 7)]),
                        fields("load_map", &[("map", 0x3002)]),
                    ],
                ),
                record(0, 1, false, [0; 6], vec![op("begin_battle")]),
                // 2: the opening (the objective's text is not even in the scene's text).
                record(
                    0,
                    2,
                    false,
                    [0; 6],
                    vec![
                        fields("set_ai", &[("person", 304), ("mode", 2), ("unused", 0)]),
                        fields("set_objective", &[("text", 0x60)]),
                        fields("dialogue", &[("text", 0x10)]),
                        fields("remove_person", &[("person", 300)]),
                        fields("join_battle", &[("person", 302)]),
                        fields("set_flag", &[("flag", 5), ("clear", 0)]),
                        fields("dialogue", &[("text", 0x40)]),
                        // A line only the route of flag 38 hears, and one everybody hears after it.
                        guard(1, vec![38], vec![]),
                        fields("dialogue", &[("text", 0x10)]),
                        fields("dialogue", &[("text", 0x40)]),
                    ],
                ),
            ],
        };
        let scene = Scene {
            blocks: vec![prep, battle],
        };
        let c = convert_leg(&scene, 2, 0, "o");
        let turn_one = Trigger::TurnStart {
            turn: 1,
            side: Side::Player,
        };
        let drama = |scene: &str| EventAction::Drama {
            scene: scene.into(),
        };
        // Three events at the first turn, in the order the script tells them: what is said
        // before the route's line, the route's line under its flag, what is said after it.
        assert_eq!(c.battle.events.len(), 3, "{:#?}", c.battle.events);
        assert!(c.battle.events.iter().all(|e| e.trigger == turn_one));
        let [before, route, after] = &c.battle.events[..] else {
            unreachable!()
        };
        assert_eq!(
            before.actions,
            [
                drama("orig_o_2"),
                EventAction::Retreat {
                    target: "person_300".into()
                },
                EventAction::SetFlag {
                    flag: crate::chapters::flag(5),
                    value: 1
                },
                drama("orig_o_2_2"),
            ]
        );
        assert!(before.when.is_empty());
        assert_eq!(
            route.when,
            [FlagCond {
                flag: crate::chapters::flag(38),
                cmp: Compare::Ne,
                value: 0
            }]
        );
        assert_eq!(route.actions, [drama("orig_o_2_3")]);
        assert!(after.when.is_empty() && after.unless.is_empty());
        assert_eq!(after.actions, [drama("orig_o_2_4")]);
        assert!(c.drama.contains("guan_yu: 결투다!"), "{}", c.drama);
        assert!(
            !c.notes
                .iter()
                .any(|n| n.contains("no string") || n.contains("joins but")),
            "{:?}",
            c.notes
        );
    }

    /// The army's changes of the opening (group 2) are made once, before the camp
    /// (`chapters::before_scene`), not again as the battle begins; an enemy's level is the
    /// battle's own.
    #[test]
    fn the_openings_army_changes_are_made_before_the_camp_only() {
        let prep = Block {
            offset: 0,
            records: vec![record(
                0,
                0,
                false,
                [0; 6],
                vec![setup(Some(54), 25, vec![unit(0, 1, 1)])],
            )],
        };
        let battle = Block {
            offset: 0,
            records: vec![
                record(
                    0,
                    0,
                    false,
                    [0; 6],
                    vec![
                        roster(vec![unit(54, 9, 4), unit(300, 8, 4)]),
                        fields("load_map", &[("map", 0x3002)]),
                    ],
                ),
                record(0, 1, false, [0; 6], vec![op("begin_battle")]),
                record(
                    0,
                    2,
                    false,
                    [0; 6],
                    vec![
                        fields("add_levels", &[("person", 1), ("levels", 2)]),
                        fields("add_levels", &[("person", 54), ("levels", 1)]),
                        fields("set_allegiance", &[("person", 54), ("army", 0)]),
                        fields("dialogue", &[("text", 0x10)]),
                    ],
                ),
            ],
        };
        let scene = Scene {
            blocks: vec![prep, battle],
        };
        let c = convert_leg(&scene, 2, 0, "o");
        let actions: Vec<&EventAction> = c.battle.events.iter().flat_map(|e| &e.actions).collect();
        // The officer who joins leaves the enemy's ranks by their unit's own tag (a deployed
        // copy of them would match their officer id).
        assert_eq!(
            actions,
            [
                &EventAction::LevelUp {
                    target: "boss".into(),
                    amount: 1
                },
                &EventAction::Retreat {
                    target: "person_54".into()
                },
                &EventAction::Drama {
                    scene: "orig_o_2".into()
                },
            ]
        );
        let boss = c
            .battle
            .units
            .iter()
            .find(|u| u.officer.as_deref() == Some("boss"))
            .unwrap();
        assert_eq!(boss.tag.as_deref(), Some("person_54"));
        assert!(c.army.is_empty(), "{:?}", c.army);
        assert!(
            c.notes.iter().any(|n| n.contains("made before the camp")),
            "{:?}",
            c.notes
        );
        // ...where the camp's scene makes them.
        let (names, text) = (names(), text());
        let song_key = |_: u16| None;
        let none = BTreeSet::new();
        let ctx = crate::chapters::StoryContext {
            names: &names,
            text: &text,
            song_key: &song_key,
            block: 1,
            route_flag: "route",
            settable: &none,
            pictures: &none,
            places: &[],
        };
        let before = crate::chapters::before_scene(&scene.blocks[1], &ctx);
        assert_eq!(before.text, "@level guan_yu 2\n@join boss\n");
    }

    #[test]
    fn the_ended_flag_is_named_after_the_battle() {
        assert_eq!(ended_flag("c2_s3_b7"), "orig_c2_s3_b7_ended");
    }

    /// The stage's records for `events_end_battle`: a phase of `group` (parallel or not).
    fn stage(parallel: bool, records: Vec<(u8, [u8; 6], Vec<Instr>)>) -> Vec<Record> {
        records
            .into_iter()
            .enumerate()
            .map(|(i, (kind, args, code))| record(kind, 3, parallel && i == 0, args, code))
            .collect()
    }

    #[test]
    fn events_end_battle_needs_a_victory_script_and_a_record_that_ends_the_stage() {
        let leave = || vec![op("leave_parallel")];
        let won = || (BATTLE_WON, [0; 6], leave());
        // A civilian on a tile that leaves the phase, next to the victory script.
        let civilian = || (UNIT_AT_CELL, [88, 1, 4, 5, 0, 0], leave());
        assert!(events_end_battle(&stage(true, vec![civilian(), won()])));
        // Without the victory script there is nothing to leave out.
        assert!(!events_end_battle(&stage(true, vec![civilian()])));
        // A record that runs on and does not leave a parallel phase ends nothing...
        let chatter = (
            9,
            [3, 0, 0, 0, 0, 0],
            vec![fields("play_music", &[("song", 3)])],
        );
        assert!(!events_end_battle(&stage(
            true,
            vec![chatter.clone(), won()]
        )));
        // ...but any record of a phase that is not watched in parallel ends it by running.
        assert!(events_end_battle(&stage(false, vec![chatter, won()])));
        // A treasure (any unit on a tile) is no event, and Liu Bei's objective of a battle with
        // one stage is its victory condition.
        let treasure = (
            UNIT_AT_CELL,
            [0, 4, 5, 6, 0, 0],
            vec![
                fields("data", &[("kind", 2), ("value", 100)]),
                op("leave_parallel"),
            ],
        );
        assert!(!events_end_battle(&stage(true, vec![treasure, won()])));
        let objective = (
            UNIT_AT_CELL,
            [0, 0, 3, 1, 0, 0],
            vec![
                fields("data", &[("kind", 4), ("value", 50)]),
                op("leave_parallel"),
            ],
        );
        assert!(!events_end_battle(&stage(
            true,
            vec![objective.clone(), won()]
        )));
        // ...while in a later stage of a battle with several it is an event.
        let mut two = stage(true, vec![civilian(), won()]);
        two.push(record(UNIT_AT_CELL, 4, true, objective.1, objective.2));
        two.push(record(BATTLE_WON, 4, false, [0; 6], leave()));
        assert!(events_end_battle(&two));
        assert!(!events_end_battle(&[]));
        // A record of an earlier stage that ends the battle itself (its forts taken) counts,
        // although the last stage holds only the victory script; one that merely moves on to
        // the next stage does not.
        let takes = |ends: &'static str| {
            let mut stages = stage(
                true,
                vec![(UNIT_IN_AREA, [0, 0, 1, 1, 2, 2], vec![op(ends)])],
            );
            stages.push(record(BATTLE_WON, 4, true, [0; 6], leave()));
            stages
        };
        assert!(events_end_battle(&takes("goto_block")));
        assert!(events_end_battle(&takes("battle_end")));
        assert!(!events_end_battle(&takes("leave_parallel")));
        // Not Liu Bei's objective of the first stage, which is the victory condition, nor a
        // treasure; but the objective of a later stage is an event.
        let routine = |ends: &'static str| {
            (
                UNIT_AT_CELL,
                [0, 0, 3, 1, 0, 0],
                vec![fields("data", &[("kind", 4), ("value", 50)]), op(ends)],
            )
        };
        let mut objective_first = stage(true, vec![routine("goto_block")]);
        objective_first.push(record(BATTLE_WON, 4, true, [0; 6], leave()));
        assert!(!events_end_battle(&objective_first));
        let treasure = (
            UNIT_AT_CELL,
            [0, 4, 5, 6, 0, 0],
            vec![
                fields("data", &[("kind", 2), ("value", 100)]),
                op("goto_block"),
            ],
        );
        let mut treasure_first = stage(true, vec![treasure]);
        treasure_first.push(record(BATTLE_WON, 4, true, [0; 6], leave()));
        assert!(!events_end_battle(&treasure_first));
        let mut objective_later = stage(true, vec![civilian()]);
        let (kind, args, code) = routine("goto_block");
        objective_later.push(record(kind, 4, true, args, code));
        objective_later.push(record(BATTLE_WON, 5, true, [0; 6], leave()));
        assert!(events_end_battle(&objective_later));
    }

    /// Civilians are the `BAKDATA` persons of the civilian class that no officer of the pack plays.
    #[test]
    fn civilians_are_the_persons_of_the_civilian_class_without_an_officer() {
        let person = |index: usize, class: u8| Officer {
            index,
            name: "민중".into(),
            reading: String::new(),
            portrait: 225,
            sprite: 0,
            leadership: 0,
            war: 0,
            intelligence: 0,
            flags: 1,
            army: 14,
            role: 3,
            morale: 100,
            troops: 1000,
            class,
            level: 1,
            exp: 0,
            items: Vec::new(),
            other: [0; 2],
        };
        let civilian = crate::pack::CLASS_SPRITES
            .iter()
            .position(|s| *s == "civilian")
            .unwrap() as u8;
        let people = [person(344, civilian), person(345, civilian), person(9, 0)];
        let classes = [("civilian".to_string(), "civilian".to_string())];
        let names = Names::new(
            &people,
            &[],
            |o| (o.index == 345).then(|| "someone".to_string()),
            &crate::pack::CLASS_SPRITES,
            &classes,
            &[],
        );
        // 344 is one; 345 has an officer of the pack; 9 is not of the class.
        assert_eq!(
            names.civilians,
            BTreeMap::from([(344, ("civilian".to_string(), 1))])
        );
        // A pack without the class has none.
        let none = Names::new(
            &people,
            &[],
            |_| None,
            &crate::pack::CLASS_SPRITES,
            &[],
            &[],
        );
        assert!(none.civilians.is_empty());
    }

    /// An AI target that is no officer resolves to the tag of its unit when that is already on the
    /// map (the roster lists it first), and stays unresolved when it is not.
    #[test]
    fn ai_targets_name_earlier_units_of_persons_without_an_officer() {
        let hunter = |target| RosterUnit {
            ai_mode: Some(3),
            ai_param: Some(target),
            ..unit(300, 8, 4)
        };
        let converted = |units: Vec<RosterUnit>| {
            let mut scene = two_map_scene();
            scene.blocks[1].records[0].code[0] = roster(units);
            convert_leg(&scene, 2, 0, "c")
        };
        let find = |c: &Converted, name: &str| {
            c.battle
                .units
                .iter()
                .find(|u| u.name.as_deref() == Some(name))
                .cloned()
                .unwrap()
        };
        // 301 stands in the roster before 300, which hunts it.
        let c = converted(vec![unit(54, 9, 4), unit(301, 7, 4), hunter(301)]);
        assert_eq!(find(&c, "보병대").ai_target.as_deref(), Some("person_301"));
        assert_eq!(find(&c, "병사").tag.as_deref(), Some("person_301"));
        // 302 comes after its hunter: not on the map yet, so the hunter attacks instead.
        let c = converted(vec![unit(54, 9, 4), hunter(302), unit(302, 6, 4)]);
        let h = find(&c, "보병대");
        assert_eq!((h.ai, h.ai_target.as_deref()), (AiMode::Aggressive, None));
        assert!(
            c.notes.iter().any(|n| n.contains("AI target")),
            "{:?}",
            c.notes
        );
    }
}
