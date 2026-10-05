//! The flow of a chapter's scenario as a Markdown outline (docs/reverse-engineering/SCENARIO.md):
//! per block what it is (story, battle), its triggers and groups, and what moves the story on
//! (choices and where each option goes, yes/no questions, `goto_block`, game overs, officers
//! joining and leaving, items, flags, battle headers and rosters). Written by the extraction
//! twice: [`Detail::Structure`] (no original text: the outline the repository keeps) and
//! [`Detail::Text`] (every record with the lines it shows: stays on the player's computer).

use crate::extract::{BlockOut, InstrOut, RecordOut, ScenarioFile};
use crate::scenario::{story, Operands, TALK};
use std::collections::BTreeSet;
use std::fmt::Write as _;

/// How much of the scenario an outline shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Detail {
    /// Block summaries without any original text.
    Structure,
    /// The summaries, then every record with the lines it shows.
    Text,
}

/// A map id as its kind and number (FORMATS §13.3 `load_map`).
fn map_label(map: u16) -> String {
    match map & 0xf000 {
        0x1000 => "지도".to_string(),
        0x2000 => format!("마을 {}", map & 0xff),
        0x3000 => format!("전투 맵 {}", map & 0xff),
        _ => format!("{map:#06x}"),
    }
}

fn get(i: &InstrOut, name: &str) -> u16 {
    i.instr.operands.get(name).unwrap_or(0)
}

fn leaves(r: &RecordOut) -> bool {
    story::leaves_parallel(&r.code)
}

/// A person as the extraction resolved it, else its number.
fn person(i: &InstrOut, arg: &str) -> String {
    i.resolved
        .get(arg)
        .cloned()
        .unwrap_or_else(|| format!("#{}", get(i, arg)))
}

/// What option `k` of a choice (or the answer to a question) leads to: the record it goes on
/// with and what that record does with the story.
fn outcome(block: &BlockOut, record: usize, own_block: usize, any_goes_on: bool) -> String {
    let Some(r) = block.records.get(record) else {
        return "레코드 없음".into();
    };
    for c in &r.code {
        match c.instr.mnemonic {
            "game_over" => return format!("r{record} → 게임 오버"),
            "goto_block" if usize::from(get(c, "block")) != own_block => {
                return format!("r{record} → 블록 {}", get(c, "block"))
            }
            "goto_block" => return format!("r{record} → 다시 물음(같은 블록)"),
            _ => {}
        }
    }
    if leaves(r) || !any_goes_on {
        format!("r{record} → 진행")
    } else {
        format!("r{record} → 다시 물음")
    }
}

/// The summary lines of a block.
fn summary(block: &BlockOut, text: bool) -> Vec<String> {
    let mut out = Vec::new();
    let all = || block.records.iter().flat_map(|r| &r.code);
    // (A battle setup may load its map with a `battle_end` to it: `battles::loaded_battle_map`.)
    let battle_maps: Vec<u16> = block
        .records
        .iter()
        .flat_map(|r| {
            r.code
                .iter()
                .filter_map(|c| crate::battles::loaded_battle_map(&r.trigger, &c.instr))
        })
        .collect();
    let maps: Vec<String> = all()
        .filter(|c| c.instr.mnemonic == "load_map")
        .map(|c| get(c, "map"))
        .chain(battle_maps.iter().copied())
        .map(map_label)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let battle_map = !battle_maps.is_empty();
    let sets_up = all().any(|c| story::sets_up_battle(&c.instr));
    let kind = if battle_map && sets_up {
        "전투"
    } else if sets_up {
        "전투 준비"
    } else if all().any(|c| matches!(c.instr.mnemonic, "dialogue" | "narration" | "title")) {
        "이야기"
    } else {
        "연출"
    };
    let groups: BTreeSet<u8> = block.records.iter().map(|r| r.trigger.group).collect();
    // Chatter as the converter reads it (`scenario::story`), over the whole block: the
    // converter reads a block from where its story starts (after a battle, `records[from..]`),
    // which only matters for a group whose moving record comes before that start.
    let progressing: BTreeSet<u8> = block
        .records
        .iter()
        .filter(|r| leaves(r))
        .map(|r| r.trigger.group)
        .collect();
    let talks = block
        .records
        .iter()
        .filter(|r| r.trigger.kind == TALK)
        .count();
    let chatter = block
        .records
        .iter()
        .filter(|r| {
            story::is_chatter(
                r.trigger.kind,
                progressing.contains(&r.trigger.group),
                &r.code,
            )
        })
        .count();
    let mut head = format!(
        "{kind}; 레코드 {}, 그룹 {}; 대화 {talks}(잡담 {chatter})",
        block.records.len(),
        groups.len()
    );
    if !maps.is_empty() {
        let _ = write!(head, "; 맵: {}", maps.join(", "));
    }
    out.push(head);

    // The battle: its header and rosters.
    for c in all() {
        match &c.instr.operands {
            Operands::BattleSetup { header, units } => {
                let mut s = format!(
                    "전투 설정: 턴 제한 {}, 아군 출진 {}명",
                    header.turn_limit,
                    units.len()
                );
                if let Some(p) = header.defeat_to_win {
                    let _ = write!(s, ", 격파 대상 #{p}");
                }
                if let Some(p) = header.lose_if_defeated {
                    let _ = write!(s, ", 퇴각하면 패배 #{p}");
                }
                out.push(s);
            }
            Operands::Roster { friendly, units } => {
                let conditional = units.iter().filter(|u| u.requires_flag.is_some()).count();
                let mut s = format!(
                    "{} 명단: {}기",
                    if *friendly { "우군" } else { "적군" },
                    units.len()
                );
                if conditional > 0 {
                    let _ = write!(s, " (플래그 조건 {conditional}기)");
                }
                out.push(s);
            }
            _ => {}
        }
    }
    let kinds: BTreeSet<&str> = block
        .records
        .iter()
        .filter(|r| r.trigger.kind != 0 && r.trigger.kind != TALK)
        .map(|r| r.trigger.kind_name)
        .collect();
    if !kinds.is_empty() {
        out.push(format!(
            "트리거: {}",
            kinds.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }
    let objectives: Vec<&InstrOut> = all()
        .filter(|c| c.instr.mnemonic == "set_objective")
        .collect();
    if !objectives.is_empty() {
        out.push(if text {
            format!(
                "목표 문구: {}",
                objectives
                    .iter()
                    .map(|c| one_line(c.resolved.get("text").map_or("", String::as_str)))
                    .collect::<Vec<_>>()
                    .join(" / ")
            )
        } else {
            format!("목표 문구 {}개", objectives.len())
        });
    }

    // What moves the story on.
    let any_goes_on = |from: usize, count: usize| {
        (0..count).any(|k| {
            block.records.get(from + 1 + k).is_some_and(|t| {
                leaves(t)
                    || t.code.iter().any(|c| {
                        c.instr.mnemonic == "game_over"
                            || (c.instr.mnemonic == "goto_block"
                                && usize::from(get(c, "block")) != block.index)
                    })
            })
        })
    };
    let mut flags_set = BTreeSet::new();
    let mut flags_cleared = BTreeSet::new();
    let mut flags_tested = BTreeSet::new();
    for (i, r) in block.records.iter().enumerate() {
        for (at, c) in r.code.iter().enumerate() {
            match c.instr.mnemonic {
                "choice" => {
                    let options: Vec<String> = c
                        .resolved
                        .get("options")
                        .map(|o| {
                            o.split('\n')
                                .map(|l| l.trim_end_matches('\r').trim().to_string())
                                .filter(|l| !l.is_empty())
                                .collect()
                        })
                        .unwrap_or_default();
                    let goes_on = any_goes_on(i, options.len());
                    let list: Vec<String> = options
                        .iter()
                        .enumerate()
                        .map(|(k, o)| {
                            let to = outcome(block, i + 1 + k, block.index, goes_on);
                            if text {
                                format!("「{o}」 {to}")
                            } else {
                                format!("{}번 {to}", k + 1)
                            }
                        })
                        .collect();
                    out.push(format!("선택지 r{i}: {}", list.join("; ")));
                    // A choice ends its record's script: what the listing shows after it is the
                    // next record's code (the options' records share it).
                    break;
                }
                "if_answer" => {
                    // (The count as the script gives it, for the listing.)
                    let skip = get(c, "skip");
                    let guarded = story::guarded(&r.code, at);
                    let sortie = story::starts_battle(guarded);
                    let then: Vec<&str> = guarded
                        .iter()
                        .map(|g| g.instr.mnemonic)
                        .filter(|m| {
                            matches!(
                                *m,
                                "goto_block"
                                    | "leave_parallel"
                                    | "set_allegiance"
                                    | "battle_setup"
                                    | "game_over"
                                    | "add_item"
                            )
                        })
                        .collect();
                    let who = r
                        .trigger_person
                        .as_deref()
                        .map(|p| format!(" ({p})"))
                        .unwrap_or_default();
                    out.push(format!(
                        "질문 r{i}{who}: {} 답이면 {} 명령 실행{}{}",
                        if get(c, "answer") == 0 {
                            "예"
                        } else {
                            "아니오"
                        },
                        skip,
                        if sortie { ", 출진 확인" } else { "" },
                        if then.is_empty() {
                            String::new()
                        } else {
                            format!(" [{}]", then.join(", "))
                        }
                    ));
                }
                "goto_block" => {
                    out.push(format!(
                        "r{i} ({}) → 블록 {}",
                        r.trigger.kind_name,
                        get(c, "block")
                    ));
                }
                "game_over" => out.push(format!("r{i} 게임 오버")),
                "set_allegiance" => out.push(format!(
                    "r{i} 소속: {} → {}",
                    person(c, "person"),
                    match get(c, "army") {
                        0 => "유비군(합류)".to_string(),
                        14 => "무소속(이탈)".to_string(),
                        a => format!("군 {a}(이탈)"),
                    }
                )),
                "add_item" => out.push(format!("r{i} 아이템: {}", person(c, "item"))),
                "set_shop_items" => out.push(format!("r{i} 상점 목록 설정")),
                "add_levels" => out.push(format!(
                    "r{i} 레벨 +{}: {}",
                    get(c, "levels"),
                    person(c, "person")
                )),
                "set_class" => out.push(format!("r{i} 병종 변경: {}", person(c, "person"))),
                "set_flag" if get(c, "clear") == 0 => {
                    flags_set.insert(get(c, "flag"));
                }
                "set_flag" => {
                    flags_cleared.insert(get(c, "flag"));
                }
                "if_flags" => {
                    if let Operands::Condition {
                        all_set, all_clear, ..
                    } = &c.instr.operands
                    {
                        flags_tested.extend(all_set.iter().chain(all_clear).map(|&f| u16::from(f)));
                    }
                }
                "duel" => out.push(format!("r{i} 일기토")),
                "battle_end" => out.push(format!("r{i} ({}) 전투 끝", r.trigger.kind_name)),
                "show_picture" => out.push(format!("r{i} 삽화 {}", get(c, "picture"))),
                "ending" => out.push(format!("r{i} 엔딩")),
                _ => {}
            }
        }
    }
    for (what, flags) in [
        ("켬", &flags_set),
        ("끔", &flags_cleared),
        ("검사", &flags_tested),
    ] {
        if !flags.is_empty() {
            out.push(format!("플래그 {what}: {flags:?}"));
        }
    }
    out
}

fn one_line(s: &str) -> String {
    s.split(['\r', '\n'])
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Mnemonics of presentation (people walking, the screen) left out of a record's listing.
const PRESENTATION: [&str; 16] = [
    "place_person",
    "move_person",
    "remove_person",
    "set_graphic",
    "show_screen",
    "screen_effect",
    "input_control",
    "reset_player_position",
    "set_ai",
    "end",
    "enable_list",
    "clear_persons",
    "place_townsfolk",
    "op_23",
    "op_3b",
    "op_3c",
];

/// The records of a block with the lines they show.
fn records(block: &BlockOut, out: &mut String) {
    for r in &block.records {
        let t = &r.trigger;
        let who = r
            .trigger_person
            .as_deref()
            .map(|p| format!(" [{p}]"))
            .unwrap_or_default();
        let _ = writeln!(
            out,
            "- **r{}** {}{}({}){who} 그룹 {}{}{}",
            r.index,
            if t.inverted { "!" } else { "" },
            t.kind_name,
            t.kind,
            t.group,
            if t.group_flag { "*" } else { "" },
            if leaves(r) { " — 그룹 넘김" } else { "" }
        );
        for c in &r.code {
            let m = c.instr.mnemonic;
            if PRESENTATION.contains(&m) {
                continue;
            }
            let texts: Vec<&String> = ["text", "options"]
                .iter()
                .filter_map(|k| c.resolved.get(k))
                .collect();
            if texts.is_empty() {
                let args: Vec<String> = c
                    .instr
                    .operands
                    .args()
                    .iter()
                    .map(|a| match c.resolved.get(a.name) {
                        Some(v) => format!("{}={v}", a.name),
                        None => format!("{}={}", a.name, a.value),
                    })
                    .collect();
                let _ = writeln!(out, "  - `{m}` {}", args.join(" "));
            } else {
                for t in texts {
                    let _ = writeln!(out, "  - `{m}`");
                    for line in t.lines().map(str::trim_end).filter(|l| !l.is_empty()) {
                        let _ = writeln!(out, "    > {line}");
                    }
                }
            }
            if m == "choice" {
                // The rest is the first option's record (listed on its own).
                break;
            }
        }
    }
}

/// The outline of one scenario file.
pub(crate) fn chapter_flow(file: &ScenarioFile, detail: Detail) -> String {
    let text = detail == Detail::Text;
    let mut out = format!("## {} + {}\n", file.scenario, file.messages);
    for scene in &file.scenes {
        let _ = writeln!(out, "\n### 장면 {}\n", scene.index);
        if text {
            // The chapter's title as the scene shows it.
            if let Some(title) = scene
                .blocks
                .iter()
                .flat_map(|b| &b.records)
                .flat_map(|r| &r.code)
                .find(|c| c.instr.mnemonic == "title")
                .and_then(|c| c.resolved.get("text"))
            {
                let _ = writeln!(out, "제목: {}\n", one_line(title));
            }
        }
        for block in &scene.blocks {
            let lines = summary(block, text);
            let _ = writeln!(out, "- **블록 {}**: {}", block.index, lines[0]);
            for l in &lines[1..] {
                let _ = writeln!(out, "  - {l}");
            }
            if text {
                let mut listing = String::new();
                records(block, &mut listing);
                for l in listing.lines() {
                    let _ = writeln!(out, "  {l}");
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::{Arg, ArgKind, Instr, Trigger};
    use std::collections::BTreeMap;

    fn instr(mnemonic: &'static str, args: &[(&'static str, u16)]) -> Instr {
        Instr {
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
        }
    }

    fn record(index: usize, kind: u8, group: u8, code: Vec<Instr>) -> RecordOut {
        let trigger = Trigger {
            kind,
            kind_name: if kind == TALK { "talk" } else { "run" },
            inverted: false,
            group,
            group_flag: false,
            args: [0; 6],
        };
        let code = code
            .into_iter()
            .map(|i| {
                let mut resolved = BTreeMap::new();
                match i.mnemonic {
                    "choice" => {
                        resolved.insert("options", "간다\r\n안 간다".to_string());
                    }
                    "dialogue" => {
                        resolved.insert("text", "유비: 어떻게 할까?".to_string());
                    }
                    _ => {}
                }
                (i, resolved)
            })
            .collect();
        RecordOut::for_test(index, trigger, code)
    }

    #[test]
    fn an_outline_shows_where_each_option_goes_and_the_text_only_when_asked() {
        let file = ScenarioFile::for_test(vec![vec![
            // The choice; the listing goes on with the first option's code (they share it).
            record(
                0,
                TALK,
                1,
                vec![
                    instr("dialogue", &[("text", 1)]),
                    instr("choice", &[("options", 2)]),
                    instr("game_over", &[]),
                ],
            ),
            record(1, 0, 1, vec![instr("game_over", &[])]),
            record(2, 0, 1, vec![instr("leave_parallel", &[])]),
            // Chatter: a talk of the group that neither moves it on nor has effects.
            record(3, TALK, 1, vec![instr("dialogue", &[("text", 1)])]),
            record(4, 0, 2, vec![instr("goto_block", &[("block", 5)])]),
        ]]);
        let s = chapter_flow(&file, Detail::Structure);
        assert!(
            s.contains("- **블록 0**: 이야기; 레코드 5, 그룹 2; 대화 2(잡담 1)"),
            "{s}"
        );
        assert!(
            s.contains("선택지 r0: 1번 r1 → 게임 오버; 2번 r2 → 진행"),
            "{s}"
        );
        assert!(!s.contains("r0 게임 오버"), "{s}");
        assert!(
            s.contains("r1 게임 오버") && s.contains("r4 (run) → 블록 5"),
            "{s}"
        );
        assert!(!s.contains("어떻게") && !s.contains("간다"), "{s}");
        let t = chapter_flow(&file, Detail::Text);
        assert!(
            t.contains("「간다」 r1 → 게임 오버") && t.contains("> 유비: 어떻게 할까?"),
            "{t}"
        );
    }
}
