//! Scenario bytecode (`SNR0D.R3`–`SNR4D.R3`): scenes, event blocks, trigger records and the
//! event-script instruction set.
//!
//! Everything here was worked out from the Korean DOS/V copy: the layouts by walking the data
//! (every byte of all 18 scenes is accounted for) and the instruction set from the script
//! interpreter in `MAIN.EXE` (its opcode switch, the operand reads of each handler and the
//! debug strings the handlers print). Names of instructions whose effect was not pinned down
//! say so in their summary; see `docs/ORIGINAL_DATA.md` §10.
//!
//! ```text
//! SNRnD.R3            LS11 archive, one entry per scene (1/5/4/5/3 scenes for n = 0..4)
//! scene               [u16le block offset]… FFFF   blocks
//! block               [10-byte trigger record]… [10 × FF]   code
//! trigger record      [kind][group][u16 a][u16 b][u16 c][u16 code offset (from block start)]
//! code                instructions, each [opcode][operands], ending with FF
//! ```
//!
//! Message operands are byte offsets into the scene's section of `SNRnM.R3`
//! (see [`crate::text`]); person operands are officer indices of `BAKDATA.R3`
//! (see [`crate::bakdata`]).

use serde::Serialize;
use std::fmt;

/// Length of a trigger record.
pub const RECORD_LEN: usize = 10;
/// Number of unit slots in a battle roster instruction (`0x03`, `0x22`).
pub const ROSTER_SLOTS: usize = 30;
/// Opcode that ends a script.
pub const END: u8 = 0xff;

/// A structural problem in a scene.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScenarioError {
    /// The block table has no `0xFFFF` terminator.
    UnterminatedTable,
    /// A block, record or code offset lies outside the scene.
    OutOfRange { what: &'static str, offset: usize },
    /// A block's record list has no `10 × FF` terminator.
    UnterminatedRecords { block: usize },
    /// An opcode the interpreter does not have (or one whose operand length is undefined).
    UnknownOpcode { offset: usize, opcode: u8 },
    /// An instruction's operands run past the end of the scene.
    Truncated { offset: usize, opcode: u8 },
    /// An AI instruction (`0x1C`) with a mode whose operand length is undefined (> 6).
    BadAiMode { offset: usize, mode: u8 },
}

impl fmt::Display for ScenarioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScenarioError::UnterminatedTable => {
                write!(f, "scene: block table has no FFFF terminator")
            }
            ScenarioError::OutOfRange { what, offset } => {
                write!(f, "scene: {what} offset {offset:#x} lies outside the scene")
            }
            ScenarioError::UnterminatedRecords { block } => {
                write!(f, "scene: block {block} has no end-of-records marker")
            }
            ScenarioError::UnknownOpcode { offset, opcode } => {
                write!(f, "scene: unknown opcode {opcode:#04x} at {offset:#x}")
            }
            ScenarioError::Truncated { offset, opcode } => write!(
                f,
                "scene: opcode {opcode:#04x} at {offset:#x} runs past the end of the scene"
            ),
            ScenarioError::BadAiMode { offset, mode } => {
                write!(
                    f,
                    "scene: AI instruction at {offset:#x} has undefined mode {mode}"
                )
            }
        }
    }
}

impl std::error::Error for ScenarioError {}

// ----- operands ------------------------------------------------------------------------------

/// What an operand refers to (for display and lookups).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ArgKind {
    /// A plain number.
    Number,
    /// An officer index of `BAKDATA.R3`.
    Person,
    /// A dialogue (speaker records up to `FFFF`) in the scene's message section.
    Dialogue,
    /// A plain string in the scene's message section.
    Message,
    /// An item index of `BAKDATA.R3`.
    Item,
    /// A map id (high nibble: 1 = campaign map, 2 = town / interior, 3 = battle map).
    Map,
    /// A scenario flag (0–255).
    Flag,
    /// A block index of the same scene.
    Block,
    /// A song of `MUSIC.R3`.
    Song,
    /// A unit class (0–18).
    Class,
}

#[derive(Clone, Copy)]
enum Width {
    U8,
    U16,
}

#[derive(Clone, Copy)]
struct Field(&'static str, Width, ArgKind);

const fn b(name: &'static str) -> Field {
    Field(name, Width::U8, ArgKind::Number)
}
const fn w(name: &'static str) -> Field {
    Field(name, Width::U16, ArgKind::Number)
}
const fn person(name: &'static str) -> Field {
    Field(name, Width::U16, ArgKind::Person)
}
const fn msg(name: &'static str) -> Field {
    Field(name, Width::U16, ArgKind::Message)
}

/// One decoded operand.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Arg {
    pub name: &'static str,
    pub kind: ArgKind,
    pub value: u16,
}

/// A unit slot of a battle roster.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RosterUnit {
    pub person: u16,
    /// First coordinate byte (runtime slot +2).
    pub x: u8,
    /// Second coordinate byte (runtime slot +3).
    pub y: u8,
    /// The unit is only deployed when this flag is set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requires_flag: Option<u8>,
    /// Unit class (enemy / NPC rosters only; player units keep their officer's class).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub class: Option<u8>,
    /// Level (enemy / NPC rosters only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<u8>,
    /// AI mode and its parameter (enemy / NPC rosters only; as for instruction `0x1C`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ai_mode: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ai_param: Option<u16>,
    /// The remaining bytes, in record order, whose meaning is not known.
    pub other: Vec<u8>,
}

/// The header of the battle set-up instruction (`0x03`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BattleHeader {
    /// Turn limit.
    pub turn_limit: u8,
    /// Victory by defeating this officer (when set).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub defeat_to_win: Option<u16>,
    /// Defeat when this officer retreats (when set; 0 = Liu Bei).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lose_if_defeated: Option<u16>,
    /// Header bytes 0, 2, 3 and 7, whose meaning is not known (3 and 7 are never read).
    pub other: [u8; 4],
}

/// Decoded operands of an instruction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "form", rename_all = "kebab-case")]
pub enum Operands {
    /// Fixed fields.
    Fields { args: Vec<Arg> },
    /// `0x03`: battle header and the player-side roster.
    BattleSetup {
        header: BattleHeader,
        units: Vec<RosterUnit>,
    },
    /// `0x22`: an enemy (`friendly == false`) or NPC-ally roster.
    Roster {
        friendly: bool,
        units: Vec<RosterUnit>,
    },
    /// `0x21`: unless every flag of `all_set` is set and every flag of `all_clear` is clear,
    /// skip the next `skip` instructions.
    Condition {
        skip: u8,
        all_set: Vec<u8>,
        all_clear: Vec<u8>,
    },
    /// `0x20`: list operand (`reset_all` = bit 7 of the count byte).
    List { reset_all: bool, values: Vec<u8> },
    /// `0x32`: raw bytes stored into the event state.
    Bytes { bytes: Vec<u8> },
}

impl Operands {
    /// The fixed fields (empty for the other forms).
    pub fn args(&self) -> &[Arg] {
        match self {
            Operands::Fields { args } => args,
            _ => &[],
        }
    }

    /// The value of a named fixed field.
    pub fn get(&self, name: &str) -> Option<u16> {
        self.args().iter().find(|a| a.name == name).map(|a| a.value)
    }
}

/// One instruction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Instr {
    /// Offset of the opcode in the scene.
    pub offset: usize,
    pub opcode: u8,
    pub mnemonic: &'static str,
    #[serde(flatten)]
    pub operands: Operands,
}

// ----- instruction set -----------------------------------------------------------------------

#[derive(Clone, Copy)]
enum Layout {
    Fixed(&'static [Field]),
    BattleSetup,
    Roster,
    Condition,
    List,
    Bytes,
    Ai,
    End,
}

/// Description of one opcode.
pub struct OpInfo {
    pub opcode: u8,
    pub mnemonic: &'static str,
    /// What the handler does, as far as verified.
    pub summary: &'static str,
    layout: Layout,
}

macro_rules! op {
    ($code:expr, $name:expr, $layout:expr, $summary:expr) => {
        OpInfo {
            opcode: $code,
            mnemonic: $name,
            summary: $summary,
            layout: $layout,
        }
    };
}

const NONE: Layout = Layout::Fixed(&[]);

/// The instruction set of the Korean DOS/V interpreter (`0x00`–`0x3D` and `0xFF`). `0x19` falls
/// through to the default case like `0x31` but its operand length is contradictory (the skip
/// table says two bytes, the default case reads none) and it never occurs, so it is rejected.
pub const OPCODES: &[OpInfo] = &[
    op!(0x00, "dialogue", Layout::Fixed(&[Field("text", Width::U16, ArgKind::Dialogue)]),
        "show a dialogue (speaker records up to FFFF); when the next instruction is 0x15 its first line becomes a yes/no question"),
    op!(0x01, "move_person", Layout::Fixed(&[person("person"), b("x"), b("y"), b("dir")]),
        "walk a person on the town map; consecutive moves run together, then the script waits"),
    op!(0x02, "add_menu", Layout::Fixed(&[b("entry")]), "add a menu entry (debug text: menu add)"),
    op!(0x03, "battle_setup", Layout::BattleSetup,
        "battle header (turn limit, victory and defeat officers) and the player-side roster of 30 slots"),
    op!(0x04, "weather", NONE, "no operands; only prints a debug line (weather) in this build"),
    op!(0x05, "show_screen", NONE,
        "bring up the prepared map (screen-mode dependent) and wait; always follows load_map and the placements (name unverified)"),
    op!(0x06, "op_06", NONE, "screen-mode dependent call like 0x05's with other arguments; not used by the data"),
    op!(0x07, "show_picture", Layout::Fixed(&[b("picture"), b("variant")]),
        "show an event picture; following narrations (0x08) are drawn over it, the next other instruction closes it"),
    op!(0x08, "narration", Layout::Fixed(&[msg("text")]), "show a narration string"),
    op!(0x09, "load_map", Layout::Fixed(&[Field("map", Width::U16, ArgKind::Map)]),
        "switch to a map (debug text: map %04x select) and suspend the script until it is loaded"),
    op!(0x0a, "place_person", Layout::Fixed(&[person("person"), b("x"), b("y"), b("dir")]),
        "put a person on the town map"),
    op!(0x0b, "caption", Layout::Fixed(&[msg("text")]), "show a caption box (places, gains)"),
    op!(0x0c, "op_0c", Layout::Fixed(&[w("unused"), msg("text")]),
        "loads a string without showing it; not used by the scenario data"),
    op!(0x0d, "title", Layout::Fixed(&[msg("text")]), "show a title"),
    op!(0x0e, "chapter_title", Layout::Fixed(&[msg("key")]),
        "show one of 18 chapter titles built into MAIN.EXE; the message is a key character, index = key - '0'"),
    op!(0x0f, "goto_block", Layout::Fixed(&[Field("block", Width::U8, ArgKind::Block)]),
        "continue with another block of the scene (debug text: block (scene) move)"),
    op!(0x10, "duel", Layout::Fixed(&[person("first"), person("second")]), "one-on-one duel"),
    op!(0x11, "battle_end", Layout::Fixed(&[Field("next_map", Width::U16, ArgKind::Map)]),
        "end of the battle definition; the map to return to (must be a campaign map)"),
    op!(0x12, "end_event", NONE, "end the event (debug text: event end)"),
    op!(0x13, "leave_parallel", NONE, "leave parallel (concurrent) control and suspend"),
    op!(0x14, "set_flag", Layout::Fixed(&[Field("flag", Width::U8, ArgKind::Flag), b("clear")]),
        "set a scenario flag (clear = 0) or clear it (clear = 1)"),
    op!(0x15, "if_answer", Layout::Fixed(&[b("answer"), b("skip")]),
        "ask the preceding dialogue as a yes/no question; if the reply equals `answer`, skip the next `skip` instructions (answer 0 skips the battle start after the prologue's 'ready?', so 0 is the refusal)"),
    op!(0x16, "withdraw_unit", Layout::Fixed(&[person("person"), w("arg")]),
        "remove a unit from the battle (debug text: HEX unit can no longer fight); unverified"),
    op!(0x17, "input_control", Layout::Fixed(&[b("value")]), "user input control switch"),
    op!(0x18, "remove_person", Layout::Fixed(&[person("person")]),
        "remove a person from the town map"),
    op!(0x1a, "join_battle", Layout::Fixed(&[person("person")]),
        "a unit joins the battle (debug text: %s's unit joins)"),
    op!(0x1b, "add_item", Layout::Fixed(&[Field("item", Width::U8, ArgKind::Item)]),
        "add an item to the inventory"),
    op!(0x1c, "set_ai", Layout::Ai,
        "set an officer's battle AI mode (0-6); modes 3 and 5 take a target officer, 4 and 6 two bytes, 0-2 an unused word"),
    op!(0x1d, "set_previous_map", Layout::Fixed(&[Field("map", Width::U16, ArgKind::Map), b("x"), b("y")]),
        "remember the map and position to return to (debug text: previous map setting)"),
    op!(0x1e, "clear_persons", NONE, "clear the town map's person list"),
    op!(0x1f, "place_townsfolk", NONE, "place the town's ordinary people"),
    op!(0x20, "enable_list", Layout::List,
        "mark the listed entries of a 32-byte table as available (bit 7 of the count: first mark all unavailable); unverified what the table is"),
    op!(0x21, "if_flags", Layout::Condition,
        "flag condition; at the start of a group-flagged record it is the trigger condition"),
    op!(0x22, "battle_roster", Layout::Roster,
        "enemy (mode 0) or NPC-ally (mode != 0) roster of 30 slots with class, level and AI"),
    op!(0x23, "op_23", NONE, "battle routine call (unverified)"),
    op!(0x24, "set_allegiance", Layout::Fixed(&[person("person"), b("army")]),
        "change an officer's army (0 = Liu Bei ... 14 = none)"),
    op!(0x25, "choice", Layout::Fixed(&[msg("options")]),
        "menu of the message's lines; option i continues with record (current + 1 + i) of the group"),
    op!(0x26, "set_map_chip", Layout::Fixed(&[b("x"), b("y"), b("chip")]), "change a map cell"),
    op!(0x27, "screen_effect", Layout::Fixed(&[b("x"), b("y"), b("effect")]),
        "effect graphic on a map cell (debug text: screen effect CG); runs of these trace cell paths"),
    op!(0x28, "set_country", Layout::Fixed(&[person("person"), b("country")]),
        "change an officer's country number"),
    op!(0x29, "game_over", NONE, "game over"),
    op!(0x2a, "ending", Layout::Fixed(&[b("ending")]), "play an ending (0-3)"),
    op!(0x2b, "data", Layout::Fixed(&[b("kind"), w("value")]),
        "data operation: kind 2 adds gold, kind 4 is a battle routine (always value 50: the +50 EXP bonus?)"),
    op!(0x2c, "redraw", NONE, "redraw the screen"),
    op!(0x2d, "halve", Layout::Fixed(&[b("a"), b("b")]), "halve the morale (b 0) or troops of one side's units on the field (a 0: the player's side)"),
    op!(0x2e, "reset_player_position", Layout::Fixed(&[w("target"), b("x"), b("y"), b("dir")]),
        "reset the player's position"),
    op!(0x2f, "set_graphic", Layout::Fixed(&[person("person"), b("graphic")]),
        "change an officer's graphic (debug text: change %s's CG to %d); used on the campaign map for Liu Bei"),
    op!(0x30, "set_objective", Layout::Fixed(&[msg("text")]), "store the battle objective text"),
    op!(0x31, "nop", NONE, "falls through to the interpreter's default case: no effect"),
    op!(0x32, "set_shop_items", Layout::Bytes,
        "store an item list in the event state; both uses list consumables and run when the item seller is talked to (the shop's stock)"),
    op!(0x33, "duel_end", NONE, "closes a duel sequence (always after 0x34 … 0x35; name from its position)"),
    op!(0x34, "duel_begin", NONE, "opens a duel sequence (always right after 0x10; name from its position)"),
    op!(0x35, "duel_action", Layout::Fixed(&[person("person"), b("action")]),
        "one duel move of a fighter (between 0x34 and 0x33; action codes unverified)"),
    op!(0x36, "begin_battle", NONE,
        "the whole script of group 1 of every battle block, right after the roster (starts the battle; unverified)"),
    op!(0x37, "set_officer_bit", Layout::Fixed(&[person("person"), b("on")]),
        "set or clear bit 6 of an officer's army byte"),
    op!(0x38, "play_music", Layout::Fixed(&[Field("song", Width::U8, ArgKind::Song)]),
        "play a song of MUSIC.R3 (0-19)"),
    op!(0x39, "add_levels", Layout::Fixed(&[person("person"), b("levels")]),
        "raise an officer's level (capped at 99)"),
    op!(0x3a, "set_class", Layout::Fixed(&[person("person"), Field("class", Width::U8, ArgKind::Class)]),
        "change an officer's class"),
    op!(0x3b, "op_3b", Layout::Fixed(&[b("x"), b("y")]),
        "campaign-map routine with a position, then suspend (unverified)"),
    op!(0x3c, "op_3c", Layout::Fixed(&[person("person")]), "campaign-map routine for an officer (unverified)"),
    op!(0x3d, "op_3d", Layout::Fixed(&[b("value")]), "campaign-map routine (unverified)"),
    op!(END, "end", Layout::End, "end of the script"),
];

/// The description of an opcode.
pub fn op_info(opcode: u8) -> Option<&'static OpInfo> {
    OPCODES.iter().find(|o| o.opcode == opcode)
}

// ----- triggers ------------------------------------------------------------------------------

/// A trigger record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Trigger {
    /// Record kind (low 7 bits of byte 0).
    pub kind: u8,
    pub kind_name: &'static str,
    /// Bit 7 of byte 0: the record fires when its test does *not* match.
    pub inverted: bool,
    /// Record group (low 7 bits of byte 1); consecutive records with the same group are the
    /// alternatives of one event.
    pub group: u8,
    /// Bit 7 of byte 1 (read from a group's first record).
    pub group_flag: bool,
    /// Bytes 2–7.
    pub args: [u8; 6],
}

impl Trigger {
    /// The first argument word (a person, location or turn for most kinds).
    pub fn word(&self, i: usize) -> u16 {
        u16::from_le_bytes([self.args[2 * i], self.args[2 * i + 1]])
    }
}

/// Name of a trigger kind, from the matcher that tests it and the game code that calls it.
pub fn trigger_kind_name(kind: u8) -> &'static str {
    match kind {
        0 => "run",
        1 => "condition",
        2 => "location",
        3 => "talk",
        4 => "unit_contact",
        5 => "campaign_location",
        6 => "unit_at_cell",
        7 => "battle_won",
        8 => "battle_lost",
        9 => "turn",
        0x0b => "unit_in_area",
        0x0c => "unit_defeated",
        _ => "unknown",
    }
}

/// Record kind of a person one talks to (FORMATS §13.2).
pub const TALK: u8 = 3;

/// Readings of a record's script that the story converter (`chapters`) and the scenario outline
/// (`flow`) share, so that the outline documents what the conversion does.
pub mod story {
    use super::{Instr, TALK};

    /// Whether the script leaves its group's parallel control (moves the story on).
    pub fn leaves_parallel<I: AsRef<Instr>>(code: &[I]) -> bool {
        code.iter().any(|c| c.as_ref().mnemonic == "leave_parallel")
    }

    /// Whether the script changes the army, the inventory or the original's flags: officers
    /// joining or leaving, items, the shop, gold, flags, levels and classes.
    pub fn changes_state<I: AsRef<Instr>>(code: &[I]) -> bool {
        code.iter().any(|c| {
            matches!(
                c.as_ref().mnemonic,
                "set_allegiance"
                    | "set_country"
                    | "add_item"
                    | "set_shop_items"
                    | "data"
                    | "set_flag"
                    | "add_levels"
                    | "set_class"
            )
        })
    }

    /// Whether an instruction sets a battle up (its setup, rosters, or its start): a block
    /// with one is a battle, or a battle's preparation when the battle map comes later.
    pub fn sets_up_battle(instr: &Instr) -> bool {
        matches!(
            instr.mnemonic,
            "battle_setup" | "battle_roster" | "begin_battle"
        )
    }

    /// Whether instructions an answer guards start a battle (`op_3d`, a battle's setup).
    pub fn starts_battle<I: AsRef<Instr>>(guarded: &[I]) -> bool {
        guarded.iter().any(|g| {
            matches!(
                g.as_ref().mnemonic,
                "op_3d" | "battle_setup" | "begin_battle"
            )
        })
    }

    /// The instructions the `if_answer` at `code[at]` guards.
    pub fn guarded<I: AsRef<Instr>>(code: &[I], at: usize) -> &[I] {
        let skip = usize::from(code[at].as_ref().operands.get("skip").unwrap_or(0));
        &code[at + 1..(at + 1 + skip).min(code.len())]
    }

    /// Whether the script asks whether to set out: a question whose yes starts a battle.
    pub fn asks_sortie<I: AsRef<Instr>>(code: &[I]) -> bool {
        code.iter()
            .enumerate()
            .any(|(i, c)| c.as_ref().mnemonic == "if_answer" && starts_battle(guarded(code, i)))
    }

    /// Whether a record of `kind` with this script is optional chatter: a talk that does not
    /// move the story on, in a group where another record does (`group_progresses`), and that
    /// changes nothing, asks nothing and is no call to set out.
    pub fn is_chatter<I: AsRef<Instr>>(kind: u8, group_progresses: bool, code: &[I]) -> bool {
        kind == TALK
            && group_progresses
            && !leaves_parallel(code)
            && !changes_state(code)
            // A talk that asks leads to the records of its options.
            && !code.iter().any(|c| c.as_ref().mnemonic == "choice")
            && !asks_sortie(code)
    }
}

impl AsRef<Instr> for Instr {
    fn as_ref(&self) -> &Instr {
        self
    }
}

/// A trigger record and its script.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Record {
    /// Offset of the record in the scene.
    pub offset: usize,
    pub trigger: Trigger,
    /// Offset of the script in the scene.
    pub code_offset: usize,
    pub code: Vec<Instr>,
}

/// An event block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Block {
    /// Offset of the block in the scene.
    pub offset: usize,
    pub records: Vec<Record>,
}

/// A decoded scene.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Scene {
    pub blocks: Vec<Block>,
}

impl Scene {
    /// Every instruction of every record.
    pub fn instructions(&self) -> impl Iterator<Item = &Instr> {
        self.blocks
            .iter()
            .flat_map(|b| b.records.iter().flat_map(|r| r.code.iter()))
    }
}

fn u16_at(data: &[u8], at: usize, what: &'static str) -> Result<u16, ScenarioError> {
    data.get(at..at + 2)
        .map(|p| u16::from_le_bytes([p[0], p[1]]))
        .ok_or(ScenarioError::OutOfRange { what, offset: at })
}

/// Parse a scene (one decoded entry of an `SNRnD.R3` archive).
pub fn parse_scene(data: &[u8]) -> Result<Scene, ScenarioError> {
    let mut offsets = Vec::new();
    let mut at = 0;
    loop {
        let v = u16_at(data, at, "block table").map_err(|_| ScenarioError::UnterminatedTable)?;
        at += 2;
        if v == 0xffff {
            break;
        }
        offsets.push(v as usize);
    }
    let mut blocks = Vec::with_capacity(offsets.len());
    for (index, &block) in offsets.iter().enumerate() {
        if block >= data.len() {
            return Err(ScenarioError::OutOfRange {
                what: "block",
                offset: block,
            });
        }
        let mut records = Vec::new();
        let mut at = block;
        loop {
            let Some(raw) = data.get(at..at + RECORD_LEN) else {
                return Err(ScenarioError::UnterminatedRecords { block: index });
            };
            if raw.iter().all(|&b| b == 0xff) {
                break;
            }
            let code_offset = block + u16::from_le_bytes([raw[8], raw[9]]) as usize;
            let mut args = [0u8; 6];
            args.copy_from_slice(&raw[2..8]);
            records.push(Record {
                offset: at,
                trigger: Trigger {
                    kind: raw[0] & 0x7f,
                    kind_name: trigger_kind_name(raw[0] & 0x7f),
                    inverted: raw[0] & 0x80 != 0,
                    group: raw[1] & 0x7f,
                    group_flag: raw[1] & 0x80 != 0,
                    args,
                },
                code_offset,
                code: decode_script(data, code_offset)?,
            });
            at += RECORD_LEN;
        }
        blocks.push(Block {
            offset: block,
            records,
        });
    }
    Ok(Scene { blocks })
}

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
    start: usize,
    opcode: u8,
}

impl Reader<'_> {
    fn u8(&mut self) -> Result<u8, ScenarioError> {
        let v = *self.data.get(self.at).ok_or(ScenarioError::Truncated {
            offset: self.start,
            opcode: self.opcode,
        })?;
        self.at += 1;
        Ok(v)
    }

    fn u16(&mut self) -> Result<u16, ScenarioError> {
        Ok(u16::from(self.u8()?) | u16::from(self.u8()?) << 8)
    }

    fn bytes(&mut self, n: usize) -> Result<Vec<u8>, ScenarioError> {
        (0..n).map(|_| self.u8()).collect()
    }
}

/// Decode the script starting at `start` up to and including its `FF`.
pub fn decode_script(data: &[u8], start: usize) -> Result<Vec<Instr>, ScenarioError> {
    if start >= data.len() {
        return Err(ScenarioError::OutOfRange {
            what: "code",
            offset: start,
        });
    }
    let mut out = Vec::new();
    let mut at = start;
    loop {
        let opcode = data[at];
        let info = op_info(opcode).ok_or(ScenarioError::UnknownOpcode { offset: at, opcode })?;
        let mut r = Reader {
            data,
            at: at + 1,
            start: at,
            opcode,
        };
        let operands = decode_operands(&mut r, info.layout)?;
        out.push(Instr {
            offset: at,
            opcode,
            mnemonic: info.mnemonic,
            operands,
        });
        if opcode == END {
            return Ok(out);
        }
        at = r.at;
        if at >= data.len() {
            return Err(ScenarioError::Truncated {
                offset: at,
                opcode: END,
            });
        }
    }
}

fn fields(r: &mut Reader<'_>, spec: &[Field]) -> Result<Vec<Arg>, ScenarioError> {
    spec.iter()
        .map(|&Field(name, width, kind)| {
            let value = match width {
                Width::U8 => u16::from(r.u8()?),
                Width::U16 => r.u16()?,
            };
            Ok(Arg { name, kind, value })
        })
        .collect()
}

fn decode_operands(r: &mut Reader<'_>, layout: Layout) -> Result<Operands, ScenarioError> {
    Ok(match layout {
        Layout::End => Operands::Fields { args: Vec::new() },
        Layout::Fixed(spec) => Operands::Fields {
            args: fields(r, spec)?,
        },
        Layout::Ai => {
            let mut args = fields(r, &[person("person"), b("mode")])?;
            let mode = args[1].value as u8;
            match mode {
                0..=2 => args.extend(fields(r, &[w("unused")])?),
                3 | 5 => args.extend(fields(r, &[person("target")])?),
                4 | 6 => args.extend(fields(r, &[b("p1"), b("p2")])?),
                _ => {
                    return Err(ScenarioError::BadAiMode {
                        offset: r.start,
                        mode,
                    })
                }
            }
            Operands::Fields { args }
        }
        Layout::BattleSetup => {
            let h = r.bytes(11)?;
            let word = |i: usize| u16::from_le_bytes([h[i], h[i + 1]]);
            let header = BattleHeader {
                turn_limit: h[1],
                defeat_to_win: (h[4] != 0).then(|| word(5)),
                lose_if_defeated: (h[8] != 0).then(|| word(9)),
                other: [h[0], h[2], h[3], h[7]],
            };
            let mut units = Vec::new();
            for _ in 0..ROSTER_SLOTS {
                let u = r.bytes(9)?;
                let id = u16::from_le_bytes([u[0], u[1]]);
                if id == 0xffff {
                    continue;
                }
                units.push(RosterUnit {
                    person: id,
                    x: u[2],
                    y: u[3],
                    requires_flag: (u[5] != 0).then_some(u[6]),
                    class: None,
                    level: None,
                    ai_mode: None,
                    ai_param: None,
                    other: vec![u[4], u[7], u[8]],
                });
            }
            Operands::BattleSetup { header, units }
        }
        Layout::Roster => {
            let mode = r.u8()?;
            let mut units = Vec::new();
            for _ in 0..ROSTER_SLOTS {
                let u = r.bytes(13)?;
                let id = u16::from_le_bytes([u[0], u[1]]);
                if id == 0xffff {
                    continue;
                }
                units.push(RosterUnit {
                    person: id,
                    x: u[2],
                    y: u[3],
                    requires_flag: (u[4] != 0).then_some(u[5]),
                    class: Some(u[11]),
                    level: Some(u[12]),
                    ai_mode: Some(u[8]),
                    ai_param: Some(u16::from_le_bytes([u[9], u[10]])),
                    other: vec![u[6], u[7]],
                });
            }
            Operands::Roster {
                friendly: mode != 0,
                units,
            }
        }
        Layout::Condition => {
            let skip = r.u8()?;
            let n = r.u8()? as usize;
            let all_set = r.bytes(n)?;
            let m = r.u8()? as usize;
            let all_clear = r.bytes(m)?;
            Operands::Condition {
                skip,
                all_set,
                all_clear,
            }
        }
        Layout::List => {
            let count = r.u8()?;
            Operands::List {
                reset_all: count & 0x80 != 0,
                values: r.bytes((count & 0x7f) as usize)?,
            }
        }
        Layout::Bytes => {
            let n = r.u8()? as usize;
            Operands::Bytes { bytes: r.bytes(n)? }
        }
    })
}

// ----- fixtures ------------------------------------------------------------------------------

/// Assemble a scene (fixtures): blocks of `(record header bytes 0–7, script bytes)`. Each
/// script must end with `FF`.
pub fn build_scene(blocks: &[Vec<([u8; 8], Vec<u8>)>]) -> Vec<u8> {
    let table_len = (blocks.len() + 1) * 2;
    let mut body = Vec::new();
    let mut offsets = Vec::new();
    for records in blocks {
        let block = table_len + body.len();
        offsets.push(u16::try_from(block).expect("fixture fits in 64 KiB"));
        let head = (records.len() + 1) * RECORD_LEN;
        let mut code = Vec::new();
        for (header, script) in records {
            body.extend_from_slice(header);
            let at = u16::try_from(head + code.len()).expect("fixture fits in 64 KiB");
            body.extend_from_slice(&at.to_le_bytes());
            code.extend_from_slice(script);
        }
        body.extend_from_slice(&[0xff; RECORD_LEN]);
        body.extend(code);
    }
    let mut out: Vec<u8> = offsets.iter().flat_map(|o| o.to_le_bytes()).collect();
    out.extend_from_slice(&[0xff, 0xff]);
    out.extend(body);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn talks_that_change_nothing_are_chatter() {
        let op = |mnemonic: &'static str, args: &[(&'static str, u16)]| Instr {
            offset: 0,
            opcode: 0,
            mnemonic,
            operands: Operands::Fields {
                args: args
                    .iter()
                    .map(|&(name, value)| Arg {
                        name,
                        kind: ArgKind::Number,
                        value,
                    })
                    .collect(),
            },
        };
        let chatter = |kind, progresses, code: &[Instr]| story::is_chatter(kind, progresses, code);
        let talk = [op("dialogue", &[("text", 1)])];
        assert!(chatter(TALK, true, &talk));
        // Not a talk, or in a group nothing moves on: part of the story.
        assert!(!chatter(0, true, &talk));
        assert!(!chatter(TALK, false, &talk));
        // A talk that moves the story on, changes the army (a level too), asks, or asks
        // whether to set out.
        for extra in [
            op("leave_parallel", &[]),
            op("add_levels", &[("person", 9), ("levels", 1)]),
            op("set_class", &[("person", 9), ("class", 1)]),
            op("set_country", &[("person", 9), ("country", 0)]),
            op("choice", &[]),
        ] {
            let code = [op("dialogue", &[("text", 1)]), extra.clone()];
            assert!(!chatter(TALK, true, &code), "{}", extra.mnemonic);
        }
        let sortie = [
            op("if_answer", &[("answer", 0), ("skip", 1)]),
            op("op_3d", &[]),
        ];
        assert!(story::asks_sortie(&sortie));
        assert!(!chatter(TALK, true, &sortie));
        // An answer that guards something else.
        let other = [
            op("if_answer", &[("answer", 0), ("skip", 1)]),
            op("dialogue", &[("text", 1)]),
            op("op_3d", &[]),
        ];
        assert!(!story::asks_sortie(&other));
    }

    fn script(parts: &[&[u8]]) -> Vec<u8> {
        let mut s: Vec<u8> = parts.concat();
        s.push(END);
        s
    }

    #[test]
    fn every_opcode_has_one_description() {
        for op in 0u8..=0x3d {
            let n = OPCODES.iter().filter(|o| o.opcode == op).count();
            let expected = usize::from(op != 0x19);
            assert_eq!(n, expected, "opcode {op:#04x}");
        }
        assert!(op_info(END).is_some());
        assert!(op_info(0x3e).is_none());
    }

    #[test]
    fn decodes_fixed_operands() {
        let s = script(&[
            &[0x38, 0x02],
            &[0x0a, 0x7e, 0x01, 0x19, 0x06, 0x02],
            &[0x00, 0x34, 0x12],
            &[0x1d, 0x0b, 0x00, 0x1a, 0x06],
            &[0x14, 0x05, 0x00],
        ]);
        let code = decode_script(&s, 0).unwrap();
        let names: Vec<&str> = code.iter().map(|i| i.mnemonic).collect();
        assert_eq!(
            names,
            [
                "play_music",
                "place_person",
                "dialogue",
                "set_previous_map",
                "set_flag",
                "end"
            ]
        );
        assert_eq!(code[1].operands.get("person"), Some(0x17e));
        assert_eq!(code[1].operands.get("x"), Some(0x19));
        assert_eq!(code[1].operands.get("dir"), Some(2));
        assert_eq!(code[2].operands.args()[0].kind, ArgKind::Dialogue);
        assert_eq!(code[2].operands.get("text"), Some(0x1234));
        // 0x1D reads four operand bytes (the interpreter's skip table says two).
        assert_eq!(code[3].operands.get("map"), Some(0x0b));
        assert_eq!(code[3].operands.get("y"), Some(6));
        assert_eq!(code[4].offset, 2 + 6 + 3 + 5);
    }

    #[test]
    fn decodes_variable_operands() {
        let mut setup = vec![
            0x03, 0x00, 30, 0x00, 0x00, 0x01, 0x05, 0x00, 0x00, 0x01, 0x00, 0x00,
        ];
        // Two units, the second conditional on flag 1, then 28 empty slots.
        setup.extend([0x00, 0x00, 22, 9, 1, 0, 0, 3, 0]);
        setup.extend([0x0f, 0x00, 16, 6, 1, 1, 1, 3, 0]);
        for _ in 2..ROSTER_SLOTS {
            setup.extend([0xff, 0xff, 0, 0, 0, 0, 0, 0, 0]);
        }
        let mut roster = vec![0x22, 0x00];
        roster.extend([0x05, 0x00, 3, 9, 0, 0, 1, 0, 2, 0, 0, 6, 5]);
        for _ in 1..ROSTER_SLOTS {
            roster.extend([0xff; 13]);
        }
        let s = script(&[
            &setup,
            &roster,
            &[0x21, 0x02, 0x02, 0x07, 0x08, 0x01, 0x09],
            &[0x20, 0x83, 1, 2, 3],
            &[0x32, 0x02, 0xaa, 0xbb],
            &[0x1c, 0x05, 0x00, 0x03, 0x04, 0x00],
            &[0x1c, 0x05, 0x00, 0x04, 0x01, 0x02],
        ]);
        let code = decode_script(&s, 0).unwrap();
        assert_eq!(code.len(), 8);
        let Operands::BattleSetup { header, units } = &code[0].operands else {
            panic!("{:?}", code[0]);
        };
        assert_eq!(header.turn_limit, 30);
        assert_eq!(header.defeat_to_win, Some(5));
        assert_eq!(header.lose_if_defeated, Some(0));
        assert_eq!(units.len(), 2);
        assert_eq!((units[0].person, units[0].x, units[0].y), (0, 22, 9));
        assert_eq!(units[0].requires_flag, None);
        assert_eq!(units[1].requires_flag, Some(1));
        let Operands::Roster { friendly, units } = &code[1].operands else {
            panic!("{:?}", code[1]);
        };
        assert!(!friendly);
        assert_eq!(units.len(), 1);
        assert_eq!((units[0].class, units[0].level), (Some(6), Some(5)));
        assert_eq!((units[0].ai_mode, units[0].ai_param), (Some(2), Some(0)));
        assert_eq!(
            code[2].operands,
            Operands::Condition {
                skip: 2,
                all_set: vec![7, 8],
                all_clear: vec![9]
            }
        );
        assert_eq!(
            code[3].operands,
            Operands::List {
                reset_all: true,
                values: vec![1, 2, 3]
            }
        );
        assert_eq!(
            code[4].operands,
            Operands::Bytes {
                bytes: vec![0xaa, 0xbb]
            }
        );
        assert_eq!(code[5].operands.get("target"), Some(4));
        assert_eq!(code[6].operands.get("p2"), Some(2));
        assert_eq!(code[7].opcode, END);
    }

    #[test]
    fn script_errors() {
        assert_eq!(
            decode_script(&[0x3e, 0xff], 0).unwrap_err(),
            ScenarioError::UnknownOpcode {
                offset: 0,
                opcode: 0x3e
            }
        );
        assert_eq!(
            decode_script(&[0x19, 0x00, 0xff], 0).unwrap_err(),
            ScenarioError::UnknownOpcode {
                offset: 0,
                opcode: 0x19
            }
        );
        assert_eq!(
            decode_script(&[0x08, 0x01], 0).unwrap_err(),
            ScenarioError::Truncated {
                offset: 0,
                opcode: 0x08
            }
        );
        assert!(matches!(
            decode_script(&[0x12], 0).unwrap_err(),
            ScenarioError::Truncated { .. }
        ));
        assert_eq!(
            decode_script(&[0x1c, 0, 0, 7, 0, 0, 0xff], 0).unwrap_err(),
            ScenarioError::BadAiMode { offset: 0, mode: 7 }
        );
    }

    #[test]
    fn scene_round_trip() {
        let data = build_scene(&[
            vec![([0; 8], script(&[&[0x17, 0x01], &[0x08, 0x1d, 0x00]]))],
            vec![
                ([0, 0, 0, 0, 0, 0, 0, 0], script(&[&[0x12]])),
                (
                    [0x83, 0x02, 0x09, 0, 0, 0, 0, 0],
                    script(&[&[0x00, 0x10, 0x00]]),
                ),
                ([0x09, 0x03, 0x12, 0, 0, 0, 0, 0], script(&[&[0x0f, 0x01]])),
            ],
        ]);
        let scene = parse_scene(&data).unwrap();
        assert_eq!(scene.blocks.len(), 2);
        assert_eq!(scene.blocks[0].offset, 6);
        let b1 = &scene.blocks[1];
        assert_eq!(b1.records.len(), 3);
        let t = &b1.records[1].trigger;
        assert_eq!((t.kind, t.kind_name, t.inverted), (3, "talk", true));
        assert_eq!((t.group, t.group_flag, t.word(0)), (2, false, 9));
        assert_eq!(b1.records[2].trigger.kind_name, "turn");
        assert_eq!(b1.records[2].trigger.word(0), 0x12);
        assert_eq!(b1.records[1].code[0].mnemonic, "dialogue");
        assert_eq!(scene.instructions().count(), 3 + 2 + 2 + 2);
    }

    #[test]
    fn scene_errors() {
        assert_eq!(
            parse_scene(&[4, 0]).unwrap_err(),
            ScenarioError::UnterminatedTable
        );
        assert_eq!(
            parse_scene(&[9, 0, 0xff, 0xff]).unwrap_err(),
            ScenarioError::OutOfRange {
                what: "block",
                offset: 9
            }
        );
        let mut data = vec![4, 0, 0xff, 0xff];
        data.extend([0u8; 8]);
        assert_eq!(
            parse_scene(&data).unwrap_err(),
            ScenarioError::UnterminatedRecords { block: 0 }
        );
    }
}
