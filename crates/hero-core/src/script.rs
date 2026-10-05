//! Drama script (`.drama`) parser.
//!
//! A drama file holds one or more scenes. Each scene is a flat list of
//! commands that the [`crate::drama::DramaRunner`] executes one by one.
//! The format is documented in `docs/MODDING.md`; the short version:
//!
//! ```text
//! == prologue
//! @bg village
//! @bgm peace
//! @title 제1장 도원결의
//! @narr 184년, 황건적의 난이 천하를 뒤덮었다.
//! liu_bei: 어지러운 세상이로구나.
//!     (들여쓴 줄은 앞 대사에 이어 붙는다)
//! @choice
//!   - 의병에 참가한다 -> join
//!   - 조금 더 생각한다 -> think
//! @label join
//! @set joined = 1
//! @end
//! ```

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

/// Where a portrait is placed on the drama screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Slot {
    Left,
    Center,
    Right,
}

/// `@duel` background of a duel in a battle: `gfx/duel/terrain_<terrain id>.png` of the terrain
/// under the left officer (the right one's when the left one is not on the field), as the
/// original picks its sky and ground strips; a plain stage outside a battle or without it.
pub const DUEL_TERRAIN: &str = "terrain";

/// One side of a duel (`@duel`): the fighter on the left or on the right.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DuelSide {
    Left,
    Right,
}

/// A move of a duel fighter (`@duel_act`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DuelAct {
    /// Gallop towards the other fighter, up to a short distance from them.
    Charge,
    /// Strike: the attack frames `frame` (4, 6, 8 or 10) and the next, with a clash.
    Strike(u8),
    /// Fall from the horse and lie on the ground.
    Fall,
    /// Gallop away off the stage.
    Flee,
    /// Gallop back to the starting place.
    Back,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Compare {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

impl Compare {
    /// The operator as data files write it.
    pub fn symbol(self) -> &'static str {
        match self {
            Compare::Eq => "==",
            Compare::Ne => "!=",
            Compare::Lt => "<",
            Compare::Le => "<=",
            Compare::Gt => ">",
            Compare::Ge => ">=",
        }
    }

    /// An operator (`==`, `!=`, `<`, `<=`, `>`, `>=`) or a variant name (`Eq`, `Ge`, ... in
    /// any case).
    pub fn parse(s: &str) -> Option<Compare> {
        Some(match s.to_ascii_lowercase().as_str() {
            "==" | "eq" => Compare::Eq,
            "!=" | "ne" => Compare::Ne,
            "<" | "lt" => Compare::Lt,
            "<=" | "le" => Compare::Le,
            ">" | "gt" => Compare::Gt,
            ">=" | "ge" => Compare::Ge,
            _ => return None,
        })
    }

    pub fn eval(self, lhs: i64, rhs: i64) -> bool {
        match self {
            Compare::Eq => lhs == rhs,
            Compare::Ne => lhs != rhs,
            Compare::Lt => lhs < rhs,
            Compare::Le => lhs <= rhs,
            Compare::Gt => lhs > rhs,
            Compare::Ge => lhs >= rhs,
        }
    }
}

/// Serde helpers for a `cmp` field of the data files: written as an operator, read as an
/// operator or a variant name ([`Compare::parse`]); `!=` when left out.
pub(crate) mod cmp_field {
    use super::Compare;
    use serde::de::Error as _;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn default() -> Compare {
        Compare::Ne
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Compare, D::Error> {
        let s = String::deserialize(d)?;
        Compare::parse(&s).ok_or_else(|| {
            D::Error::custom(format!(
                "unknown comparison `{s}`, expected one of ==, !=, <, <=, >, >="
            ))
        })
    }

    pub fn serialize<S: Serializer>(cmp: &Compare, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(cmp.symbol())
    }
}

/// A condition on a campaign flag. A bare `@if flag -> label` means `flag != 0`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cond {
    pub flag: String,
    pub cmp: Compare,
    pub value: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SetOp {
    Assign,
    Add,
    Sub,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChoiceOption {
    pub text: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Cmd {
    /// Background image key, `None` clears it.
    Bg(Option<String>),
    /// Picture (an illustration of the event, `gfx/pictures/<key>.png`) shown framed over the
    /// background until another replaces it, `None` clears it.
    Picture(Option<String>),
    /// Music key, `None` stops the music.
    Bgm(Option<String>),
    Sfx(String),
    /// Show a portrait (officer id or portrait key) in a slot.
    Show {
        who: String,
        slot: Slot,
    },
    /// Hide the portrait in a slot, or every slot when `None`.
    Hide(Option<Slot>),
    /// Pause for the given number of milliseconds.
    Wait(u32),
    FadeOut,
    FadeIn,
    /// Large centred caption, e.g. a chapter title.
    Title(String),
    /// Narration box without a speaker.
    Narr(String),
    /// Dialogue. `speaker` is an officer id or a free display name.
    Say {
        speaker: String,
        text: String,
    },
    Choice(Vec<ChoiceOption>),
    Label(String),
    Goto(String),
    If {
        cond: Cond,
        label: String,
    },
    Set {
        flag: String,
        op: SetOp,
        value: i64,
    },
    /// Officer joins the player's army.
    Join(String),
    /// Officer leaves the player's army.
    Leave(String),
    /// Officer of the player's army goes away for now, keeping their progress (`@join` brings
    /// them back).
    Away(String),
    /// An officer of the army gains levels (outside battle: the level only; HP and MP follow
    /// from it in the next battle). One of the pack who is not in the army yet has them kept
    /// for when they join ([`crate::campaign::CampaignState::pending_growth`]).
    Level {
        officer: String,
        levels: u32,
    },
    /// An officer of the army changes class (equipment the new class may not use goes back to
    /// the inventory). One of the pack who is not in the army yet has it kept for when they
    /// join.
    Class {
        officer: String,
        class: String,
    },
    Gold(i64),
    /// Give an item to the army inventory.
    Item(String),
    /// Start a duel scene: two mounted officers facing each other over the background
    /// `gfx/duel/<bg>.png` (a plain stage without it; [`DUEL_TERRAIN`]: the one of the terrain
    /// under the left officer).
    Duel {
        left: String,
        right: String,
        bg: Option<String>,
    },
    /// A fighter's move; the scene waits until it is over.
    DuelAct {
        side: DuelSide,
        act: DuelAct,
    },
    /// Close the duel scene.
    DuelEnd,
    End,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scene {
    pub id: String,
    pub cmds: Vec<Cmd>,
    /// Label name -> command index.
    pub labels: BTreeMap<String, usize>,
}

/// Version prefix of [`Scene::fingerprint`]: a fingerprint of another definition never
/// matches.
pub const FINGERPRINT_VERSION: &str = "v1";

impl Scene {
    /// What identifies the scene's commands and labels across pack versions: a quick save made
    /// in the middle of the scene is played on in a newer pack whose scene has the same
    /// fingerprint ([`crate::save::SaveGame::check_resume`]).
    ///
    /// Input: the scene. Output: `v1:` and 16 hex digits.
    ///
    /// Why this form: the position of a saved scene is a command index, so "the same scene"
    /// means the same command list and labels, whatever else in the pack changed. They are
    /// hashed in their JSON form (field order fixed by the type) with FNV-1a 64, whose result
    /// does not depend on the Rust version as `std`'s hasher may; a change of `Cmd` itself
    /// changes the fingerprint, which refuses the save (the safe side).
    pub fn fingerprint(&self) -> String {
        let text =
            serde_json::to_string(&(&self.cmds, &self.labels)).expect("scene commands serialize");
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in text.bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
        format!("{FINGERPRINT_VERSION}:{hash:016x}")
    }

    pub fn label_index(&self, label: &str) -> Option<usize> {
        self.labels.get(label).copied()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub file: String,
    pub line: usize,
    pub msg: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: {}", self.file, self.line, self.msg)
    }
}

impl std::error::Error for ParseError {}

/// Parse a whole `.drama` file into its scenes (in file order).
pub fn parse_drama(file: &str, src: &str) -> Result<Vec<Scene>, ParseError> {
    let mut p = Parser {
        file,
        scenes: Vec::new(),
        cur: None,
    };
    let lines: Vec<&str> = src.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let lineno = i + 1;
        let raw = lines[i].trim_end_matches('\r');
        i += 1;
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let indented = raw.starts_with(' ') || raw.starts_with('\t');
        if let Some(id) = trimmed.strip_prefix("==") {
            p.start_scene(id.trim(), lineno)?;
            continue;
        }
        if indented && !trimmed.starts_with('@') && !trimmed.starts_with('-') {
            p.continue_text(trimmed, lineno)?;
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix('@') {
            let (word, arg) = split_word(rest);
            if word == "choice" {
                let mut options = Vec::new();
                while i < lines.len() {
                    let t = lines[i].trim();
                    if t.is_empty() || t.starts_with('#') {
                        i += 1;
                        continue;
                    }
                    let Some(opt) = t.strip_prefix('-') else {
                        break;
                    };
                    let Some((text, label)) = opt.rsplit_once("->") else {
                        return Err(p.err(i + 1, "choice option needs `- text -> label`"));
                    };
                    let (text, label) = (text.trim(), label.trim());
                    if text.is_empty() || label.is_empty() {
                        return Err(p.err(i + 1, "choice option text and label must not be empty"));
                    }
                    options.push(ChoiceOption {
                        text: text.to_string(),
                        label: label.to_string(),
                    });
                    i += 1;
                }
                if options.is_empty() {
                    return Err(p.err(lineno, "@choice has no options"));
                }
                p.push(Cmd::Choice(options), lineno)?;
                continue;
            }
            let cmd = p.parse_command(word, arg, lineno)?;
            p.push(cmd, lineno)?;
            continue;
        }
        // Dialogue: `speaker: text`
        match trimmed.split_once(':') {
            Some((speaker, text)) if is_speaker(speaker.trim()) => {
                let cmd = Cmd::Say {
                    speaker: speaker.trim().to_string(),
                    text: text.trim().to_string(),
                };
                p.push(cmd, lineno)?;
            }
            _ => {
                return Err(p.err(
                    lineno,
                    "expected `@command`, `speaker: text`, `== scene` or an indented continuation",
                ))
            }
        }
    }
    p.finish_scene(lines.len())?;
    Ok(p.scenes)
}

fn is_speaker(s: &str) -> bool {
    !s.is_empty() && s.chars().count() <= 24 && !s.contains(char::is_whitespace)
}

fn split_word(s: &str) -> (&str, &str) {
    let s = s.trim();
    match s.find(char::is_whitespace) {
        Some(pos) => (&s[..pos], s[pos..].trim()),
        None => (s, ""),
    }
}

struct Parser<'a> {
    file: &'a str,
    scenes: Vec<Scene>,
    cur: Option<Scene>,
}

impl Parser<'_> {
    fn err(&self, line: usize, msg: impl Into<String>) -> ParseError {
        ParseError {
            file: self.file.to_string(),
            line,
            msg: msg.into(),
        }
    }

    fn start_scene(&mut self, id: &str, line: usize) -> Result<(), ParseError> {
        if id.is_empty() {
            return Err(self.err(line, "scene id missing after `==`"));
        }
        self.finish_scene(line)?;
        if self.scenes.iter().any(|s| s.id == id) {
            return Err(self.err(line, format!("duplicate scene id `{id}`")));
        }
        self.cur = Some(Scene {
            id: id.to_string(),
            cmds: Vec::new(),
            labels: BTreeMap::new(),
        });
        Ok(())
    }

    fn finish_scene(&mut self, line: usize) -> Result<(), ParseError> {
        let Some(mut scene) = self.cur.take() else {
            return Ok(());
        };
        // Validate jump targets.
        for cmd in &scene.cmds {
            let targets: Vec<&String> = match cmd {
                Cmd::Goto(l) => vec![l],
                Cmd::If { label, .. } => vec![label],
                Cmd::Choice(opts) => opts.iter().map(|o| &o.label).collect(),
                _ => vec![],
            };
            for t in targets {
                if !scene.labels.contains_key(t) {
                    return Err(self.err(
                        line,
                        format!("scene `{}` jumps to unknown label `{t}`", scene.id),
                    ));
                }
            }
        }
        if scene.cmds.last() != Some(&Cmd::End) {
            scene.cmds.push(Cmd::End);
        }
        self.scenes.push(scene);
        Ok(())
    }

    fn push(&mut self, cmd: Cmd, line: usize) -> Result<(), ParseError> {
        let Some(scene) = self.cur.as_mut() else {
            return Err(self.err(line, "command outside of a scene (start one with `== id`)"));
        };
        if let Cmd::Label(name) = &cmd {
            if scene.labels.contains_key(name) {
                return Err(ParseError {
                    file: self.file.to_string(),
                    line,
                    msg: format!("duplicate label `{name}`"),
                });
            }
            scene.labels.insert(name.clone(), scene.cmds.len());
        }
        scene.cmds.push(cmd);
        Ok(())
    }

    fn continue_text(&mut self, text: &str, line: usize) -> Result<(), ParseError> {
        let last = self.cur.as_mut().and_then(|s| s.cmds.last_mut());
        match last {
            Some(Cmd::Say { text: t, .. }) | Some(Cmd::Narr(t)) | Some(Cmd::Title(t)) => {
                t.push('\n');
                t.push_str(text);
                Ok(())
            }
            _ => Err(self.err(
                line,
                "indented continuation line must follow dialogue, @narr or @title",
            )),
        }
    }

    fn parse_command(&self, word: &str, arg: &str, line: usize) -> Result<Cmd, ParseError> {
        let need = |what: &str| -> Result<String, ParseError> {
            if arg.is_empty() {
                Err(self.err(line, format!("@{word} needs {what}")))
            } else {
                Ok(arg.to_string())
            }
        };
        let optional_key = |a: &str| -> Option<String> {
            match a {
                "" | "none" | "stop" => None,
                other => Some(other.to_string()),
            }
        };
        Ok(match word {
            "bg" => Cmd::Bg(optional_key(arg)),
            "picture" => Cmd::Picture(optional_key(arg)),
            "bgm" => Cmd::Bgm(optional_key(arg)),
            "sfx" => Cmd::Sfx(need("a sound key")?),
            "show" => {
                let (who, slot) = split_word(arg);
                if who.is_empty() {
                    return Err(self.err(line, "@show needs `<who> <left|center|right>`"));
                }
                let slot = parse_slot(slot)
                    .ok_or_else(|| self.err(line, "@show slot must be left, center or right"))?;
                Cmd::Show {
                    who: who.to_string(),
                    slot,
                }
            }
            "duel" => {
                let mut words = arg.split_whitespace();
                let (Some(left), Some(right)) = (words.next(), words.next()) else {
                    return Err(self.err(line, "@duel needs `<left> <right> [background]`"));
                };
                let bg = words.next().map(str::to_string);
                if words.next().is_some() {
                    return Err(self.err(line, "@duel takes `<left> <right> [background]`"));
                }
                Cmd::Duel {
                    left: left.to_string(),
                    right: right.to_string(),
                    bg,
                }
            }
            "duel_act" => {
                let usage = "@duel_act needs `<left|right> <charge|strike N|fall|flee|back>`";
                let mut words = arg.split_whitespace();
                let side = match words.next() {
                    Some("left") => DuelSide::Left,
                    Some("right") => DuelSide::Right,
                    _ => return Err(self.err(line, usage)),
                };
                let act = match (words.next(), words.next()) {
                    (Some("charge"), None) => DuelAct::Charge,
                    (Some("fall"), None) => DuelAct::Fall,
                    (Some("flee"), None) => DuelAct::Flee,
                    (Some("back"), None) => DuelAct::Back,
                    (Some("strike"), Some(n)) => match n.parse::<u8>() {
                        Ok(frame @ (4 | 6 | 8 | 10)) => DuelAct::Strike(frame),
                        _ => {
                            return Err(
                                self.err(line, "@duel_act strike takes the frame 4, 6, 8 or 10")
                            )
                        }
                    },
                    _ => return Err(self.err(line, usage)),
                };
                if words.next().is_some() {
                    return Err(self.err(line, usage));
                }
                Cmd::DuelAct { side, act }
            }
            "duel_end" => Cmd::DuelEnd,
            "hide" => match arg {
                "" | "all" => Cmd::Hide(None),
                s => Cmd::Hide(Some(parse_slot(s).ok_or_else(|| {
                    self.err(line, "@hide takes left, center, right or all")
                })?)),
            },
            "wait" => Cmd::Wait(
                arg.parse()
                    .map_err(|_| self.err(line, "@wait needs milliseconds as a number"))?,
            ),
            "fade" => match arg {
                "out" => Cmd::FadeOut,
                "in" => Cmd::FadeIn,
                _ => return Err(self.err(line, "@fade takes `in` or `out`")),
            },
            "title" => Cmd::Title(need("text")?),
            "narr" => Cmd::Narr(need("text")?),
            "label" => Cmd::Label(need("a label name")?),
            "goto" => Cmd::Goto(need("a label name")?),
            "if" => {
                let (cond, label) = arg
                    .rsplit_once("->")
                    .ok_or_else(|| self.err(line, "@if needs `<flag> [op value] -> label`"))?;
                let label = label.trim();
                if label.is_empty() {
                    return Err(self.err(line, "@if label missing"));
                }
                Cmd::If {
                    cond: self.parse_cond(cond.trim(), line)?,
                    label: label.to_string(),
                }
            }
            "set" => {
                let (flag, op, value) = if let Some((f, v)) = arg.split_once("+=") {
                    (f, SetOp::Add, v)
                } else if let Some((f, v)) = arg.split_once("-=") {
                    (f, SetOp::Sub, v)
                } else if let Some((f, v)) = arg.split_once('=') {
                    (f, SetOp::Assign, v)
                } else {
                    return Err(self.err(line, "@set needs `flag = n`, `flag += n` or `flag -= n`"));
                };
                let flag = flag.trim();
                if flag.is_empty() {
                    return Err(self.err(line, "@set flag name missing"));
                }
                let value = v_parse(value)
                    .ok_or_else(|| self.err(line, "@set value must be an integer"))?;
                Cmd::Set {
                    flag: flag.to_string(),
                    op,
                    value,
                }
            }
            "join" => Cmd::Join(need("an officer id")?),
            "leave" => Cmd::Leave(need("an officer id")?),
            "away" => Cmd::Away(need("an officer id")?),
            "level" => {
                let mut parts = arg.split_whitespace();
                match (
                    parts.next(),
                    parts.next().map(str::parse::<u32>),
                    parts.next(),
                ) {
                    (Some(officer), Some(Ok(levels)), None) if levels > 0 => Cmd::Level {
                        officer: officer.to_string(),
                        levels,
                    },
                    _ => {
                        return Err(self.err(
                            line,
                            "@level needs `<officer> <levels>` (levels at least 1)",
                        ))
                    }
                }
            }
            "class" => {
                let mut parts = arg.split_whitespace();
                match (parts.next(), parts.next(), parts.next()) {
                    (Some(officer), Some(class), None) => Cmd::Class {
                        officer: officer.to_string(),
                        class: class.to_string(),
                    },
                    _ => return Err(self.err(line, "@class needs `<officer> <class>`")),
                }
            }
            "gold" => Cmd::Gold(
                v_parse(arg).ok_or_else(|| self.err(line, "@gold needs an integer like +100"))?,
            ),
            "item" => Cmd::Item(need("an item id")?),
            "end" => Cmd::End,
            other => return Err(self.err(line, format!("unknown command @{other}"))),
        })
    }

    fn parse_cond(&self, s: &str, line: usize) -> Result<Cond, ParseError> {
        const OPS: [(&str, Compare); 6] = [
            ("==", Compare::Eq),
            ("!=", Compare::Ne),
            ("<=", Compare::Le),
            (">=", Compare::Ge),
            ("<", Compare::Lt),
            (">", Compare::Gt),
        ];
        for (tok, cmp) in OPS {
            if let Some((flag, value)) = s.split_once(tok) {
                let flag = flag.trim();
                let value = v_parse(value)
                    .ok_or_else(|| self.err(line, "@if comparison value must be an integer"))?;
                if flag.is_empty() {
                    return Err(self.err(line, "@if flag name missing"));
                }
                return Ok(Cond {
                    flag: flag.to_string(),
                    cmp,
                    value,
                });
            }
        }
        if s.is_empty() || s.contains(char::is_whitespace) {
            return Err(self.err(line, "@if needs a flag name"));
        }
        Ok(Cond {
            flag: s.to_string(),
            cmp: Compare::Ne,
            value: 0,
        })
    }
}

fn v_parse(s: &str) -> Option<i64> {
    let s = s.trim();
    let s = s.strip_prefix('+').unwrap_or(s);
    s.parse().ok()
}

fn parse_slot(s: &str) -> Option<Slot> {
    match s.trim() {
        "left" | "l" | "L" => Some(Slot::Left),
        "center" | "c" | "C" => Some(Slot::Center),
        "right" | "r" | "R" => Some(Slot::Right),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fingerprint of a scene is fixed: games of other versions compute the same text for
    /// the same scene (a quick save made in one plays on in another), and any change of its
    /// commands or labels gives another.
    #[test]
    fn a_scenes_fingerprint_is_fixed_and_follows_its_commands() {
        let src = "== s\nliu_bei: 가자.\n@label next\n@gold +10\n@goto next\n";
        let scene = &parse_drama("t", src).unwrap()[0];
        // A change here means saves of earlier games stop playing on in newer packs: bump
        // `FINGERPRINT_VERSION` instead of updating the value silently.
        assert_eq!(scene.fingerprint(), "v1:58b210658ad11b44");
        let same = &parse_drama("t", &format!("# comment\n{src}")).unwrap()[0];
        assert_eq!(same.fingerprint(), scene.fingerprint());
        for other in [
            "== s\nliu_bei: 가자!\n@label next\n@gold +10\n@goto next\n",
            "== s\nliu_bei: 가자.\n@label again\n@gold +10\n@goto again\n",
            "== s\n@gold +10\nliu_bei: 가자.\n@label next\n@goto next\n",
        ] {
            let changed = &parse_drama("t", other).unwrap()[0];
            assert_ne!(changed.fingerprint(), scene.fingerprint(), "{other}");
        }
    }

    const SAMPLE: &str = r#"
# 주석
== prologue
@bg village
@bgm peace
@title 제1장 도원결의
@narr 184년, 황건적의 난이
    천하를 뒤덮었다.
liu_bei: 어지러운 세상이로구나.
    백성을 구하고 싶다.
장비: 형님!
@choice
  - 의병에 참가한다 -> join
  - 생각한다 -> think
@label think
@set doubt += 1
@if doubt >= 2 -> join
@goto join
@label join
@set joined = 1
@join guan_yu
@gold +100
@item bean
@show zhang_fei right
@hide all
@wait 300
@fade out
@bgm stop
@end

== second
@if joined -> done
@narr 아직.
@label done
"#;

    #[test]
    fn parses_sample() {
        let scenes = parse_drama("t.drama", SAMPLE).unwrap();
        assert_eq!(scenes.len(), 2);
        let s = &scenes[0];
        assert_eq!(s.id, "prologue");
        assert_eq!(s.cmds[0], Cmd::Bg(Some("village".into())));
        let l = parse_drama("t.drama", "== l\n@level guan_yu 2\n@class guan_yu archer\n").unwrap();
        assert_eq!(
            l[0].cmds[..2],
            [
                Cmd::Level {
                    officer: "guan_yu".into(),
                    levels: 2
                },
                Cmd::Class {
                    officer: "guan_yu".into(),
                    class: "archer".into()
                }
            ]
        );
        for bad in [
            "@level guan_yu",
            "@level guan_yu 0",
            "@level guan_yu x",
            "@class guan_yu",
        ] {
            assert!(
                parse_drama("t.drama", &format!("== b\n{bad}\n")).is_err(),
                "{bad}"
            );
        }
        let p = parse_drama("t.drama", "== p\n@picture flood\n@picture none\n").unwrap();
        assert_eq!(
            p[0].cmds,
            [
                Cmd::Picture(Some("flood".into())),
                Cmd::Picture(None),
                Cmd::End
            ]
        );
        assert_eq!(
            s.cmds[3],
            Cmd::Narr("184년, 황건적의 난이\n천하를 뒤덮었다.".into())
        );
        assert_eq!(
            s.cmds[4],
            Cmd::Say {
                speaker: "liu_bei".into(),
                text: "어지러운 세상이로구나.\n백성을 구하고 싶다.".into()
            }
        );
        assert!(matches!(&s.cmds[6], Cmd::Choice(o) if o.len() == 2 && o[1].label == "think"));
        assert_eq!(s.label_index("think"), Some(7));
        assert_eq!(
            s.cmds[8],
            Cmd::Set {
                flag: "doubt".into(),
                op: SetOp::Add,
                value: 1
            }
        );
        assert_eq!(
            s.cmds[9],
            Cmd::If {
                cond: Cond {
                    flag: "doubt".into(),
                    cmp: Compare::Ge,
                    value: 2
                },
                label: "join".into()
            }
        );
        assert_eq!(s.cmds.last(), Some(&Cmd::End));
        assert!(s.cmds.contains(&Cmd::Gold(100)));
        assert!(s.cmds.contains(&Cmd::Bgm(None)));
        // Scenes without an explicit @end get one appended.
        assert_eq!(scenes[1].cmds.last(), Some(&Cmd::End));
        assert!(
            matches!(&scenes[1].cmds[0], Cmd::If { cond, .. } if cond.cmp == Compare::Ne && cond.value == 0)
        );
    }

    #[test]
    fn parses_duels() {
        let src = "== d\n@duel guan_yu hua_xiong plain\n@duel_act left charge\n\
                   @duel_act right strike 6\n@duel_act left fall\n@duel_end\n@duel a b\n";
        let scenes = parse_drama("t.drama", src).unwrap();
        let cmds = &scenes[0].cmds;
        assert_eq!(
            cmds[0],
            Cmd::Duel {
                left: "guan_yu".into(),
                right: "hua_xiong".into(),
                bg: Some("plain".into())
            }
        );
        assert_eq!(
            cmds[2],
            Cmd::DuelAct {
                side: DuelSide::Right,
                act: DuelAct::Strike(6)
            }
        );
        assert_eq!(cmds[4], Cmd::DuelEnd);
        assert!(matches!(&cmds[5], Cmd::Duel { bg: None, .. }));
        for bad in [
            "@duel a",
            "@duel a b c d",
            "@duel_act up charge",
            "@duel_act left strike 3",
            "@duel_act left strike 11",
            "@duel_act left strike 5",
            "@duel_act left dance",
            "@duel_act left fall now",
        ] {
            assert!(
                parse_drama("t.drama", &format!("== d\n{bad}\n")).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn rejects_unknown_label() {
        let err = parse_drama("t.drama", "== a\n@goto nowhere\n").unwrap_err();
        assert!(err.msg.contains("unknown label"), "{err}");
    }

    #[test]
    fn rejects_command_outside_scene() {
        let err = parse_drama("t.drama", "@bg x\n").unwrap_err();
        assert_eq!(err.line, 1);
    }

    #[test]
    fn rejects_duplicate_scene_and_label() {
        assert!(parse_drama("t", "== a\n== a\n").is_err());
        assert!(parse_drama("t", "== a\n@label x\n@label x\n").is_err());
    }

    #[test]
    fn rejects_bad_lines() {
        assert!(parse_drama("t", "== a\n@frobnicate\n").is_err());
        assert!(parse_drama("t", "== a\nthis line has no colon\n").is_err());
        assert!(parse_drama("t", "== a\n@choice\n@end\n").is_err());
        assert!(parse_drama("t", "== a\n@wait soon\n").is_err());
        assert!(parse_drama("t", "== a\n    orphan continuation\n").is_err());
    }

    #[test]
    fn colon_inside_text_is_kept() {
        let s = parse_drama("t", "== a\n관우: 시각은 3:00 이다\n").unwrap();
        assert_eq!(
            s[0].cmds[0],
            Cmd::Say {
                speaker: "관우".into(),
                text: "시각은 3:00 이다".into()
            }
        );
    }
}
