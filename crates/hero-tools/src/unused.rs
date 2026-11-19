//! `hero-tools unused-officers`: the officers of a pack that nothing in it uses (ROADMAP M6-2).

use hero_core::pack::Pack;
use hero_core::script::Cmd;
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::Path;

pub fn run(dir: &Path) -> Result<bool, String> {
    let pack = crate::load_pack(dir)?;
    print!("{}", render(&pack));
    Ok(true)
}

/// The officer ids that something in `pack` names.
///
/// * Input: a loaded pack (with the packs it extends).
/// * Output: the ids of `officers.toml` named by the campaign's starting army, a battle (its
///   units, `deploy.required`/`forbidden`, and the unit references of its conditions, events
///   and `ai_target`s, which may be officer ids) or a scene (speakers, portraits, `@join`,
///   `@leave`, `@away`, `@level`, `@class`, `@duel`). Speakers and `@show` names count by id
///   or by display name, as [`Pack::speaker_officer`] resolves them.
/// * Why: removing an officer nothing uses is safe for the pack. A scene that writes a speaker
///   by display name (`관우: …`) still shows that officer's name and portrait (the drama runner
///   resolves it the same way), so it is a use. Every `Cmd` is matched without a catch-all, so
///   a new command that names an officer cannot be forgotten here.
pub fn used_officers(pack: &Pack) -> BTreeSet<&str> {
    let mut used: BTreeSet<&str> = BTreeSet::new();
    let mut name = |id: &str| {
        if let Some((id, _)) = pack.officers.get_key_value(id) {
            used.insert(id.as_str());
        }
    };
    for id in &pack.campaign.starting_officers {
        name(id);
    }
    for battle in pack.battles.values() {
        for unit in &battle.units {
            if let Some(o) = &unit.officer {
                name(o);
            }
        }
        for o in battle
            .deploy
            .required
            .iter()
            .chain(&battle.deploy.forbidden)
        {
            name(o);
        }
        for (_, r) in battle.unit_refs() {
            name(r);
        }
    }
    for scene in pack.scenes.values() {
        for cmd in &scene.cmds {
            match cmd {
                Cmd::Say { speaker: who, .. } => {
                    if let Some(o) = pack.speaker_officer(who) {
                        name(&o.id);
                    }
                }
                // `@show` also takes a portrait key, and the name card finds its officer by it.
                Cmd::Show { who, .. } => {
                    let by_key = || pack.officers.values().find(|o| o.portrait_key() == who);
                    if let Some(o) = pack.speaker_officer(who).or_else(by_key) {
                        name(&o.id);
                    }
                }
                Cmd::Join(who)
                | Cmd::Leave(who)
                | Cmd::Away(who)
                | Cmd::Level { officer: who, .. }
                | Cmd::Class { officer: who, .. } => name(who),
                Cmd::Duel { left, right, .. } => {
                    name(left);
                    name(right);
                }
                Cmd::Bg(_)
                | Cmd::Picture(_)
                | Cmd::Bgm(_)
                | Cmd::Sfx(_)
                | Cmd::Hide(_)
                | Cmd::Wait(_)
                | Cmd::FadeOut
                | Cmd::FadeIn
                | Cmd::Title(_)
                | Cmd::Narr(_)
                | Cmd::Choice(_)
                | Cmd::Label(_)
                | Cmd::Goto(_)
                | Cmd::If { .. }
                | Cmd::Set { .. }
                | Cmd::Gold(_)
                | Cmd::Item(_)
                | Cmd::DuelAct { .. }
                | Cmd::DuelEnd
                | Cmd::End => {}
            }
        }
    }
    used
}

/// The report: how many officers are unused and each one's id and name.
pub fn render(pack: &Pack) -> String {
    let used = used_officers(pack);
    let unused: Vec<_> = pack
        .officers
        .values()
        .filter(|o| !used.contains(o.id.as_str()))
        .collect();
    let mut out = format!(
        "{} of {} officers are not used by the campaign, a battle or a scene",
        unused.len(),
        pack.officers.len()
    );
    if unused.is_empty() {
        out.push_str(".\n");
        return out;
    }
    out.push_str(":\n");
    for o in &unused {
        let _ = writeln!(out, "  {} ({})", o.id, o.name);
    }
    out.push_str(
        "A pack that extends this one, or the original mode's pack built on it (which matches the \
         original's officers to these by name), may still use them: run this on that pack too.\n",
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use hero_core::pack::DirSource;

    fn pack(rel: &str) -> Pack {
        Pack::load(&DirSource {
            root: Path::new(env!("CARGO_MANIFEST_DIR")).join(rel),
        })
        .unwrap()
    }

    #[test]
    fn every_way_of_naming_an_officer_counts() {
        let mut pack = pack("../hero-core/tests/fixtures/mini");
        let ids: Vec<String> = pack.officers.keys().cloned().collect();
        assert!(ids.len() >= 4, "{ids:?}");
        // Nothing names anyone: every officer is unused.
        pack.campaign.starting_officers.clear();
        pack.battles.clear();
        pack.scenes.clear();
        assert!(used_officers(&pack).is_empty());
        assert!(
            render(&pack).starts_with(&format!("{n} of {n} officers are not used", n = ids.len()))
        );
        // A speaker by id, a portrait by id, a speaker by display name and a portrait by the
        // officer's own portrait key each name one (the drama runner shows that officer's
        // portrait, the name card finds the officer by the key); a free name does not.
        let display = pack.officers[&ids[2]].name.clone();
        pack.officers.get_mut(&ids[3]).unwrap().portrait = Some("young_face".into());
        let scene = hero_core::script::parse_drama(
            "t.drama",
            &format!(
                "== s\n{}: 안녕\n@show {} left\n{display}: 이름으로\n@show young_face right\n\
                 아무개: 지나가는 사람\n",
                ids[0], ids[1]
            ),
        )
        .unwrap()
        .remove(0);
        pack.scenes.insert(scene.id.clone(), scene);
        assert_eq!(
            used_officers(&pack),
            BTreeSet::from([
                ids[0].as_str(),
                ids[1].as_str(),
                ids[2].as_str(),
                ids[3].as_str()
            ])
        );
        let report = render(&pack);
        assert!(!report.contains(&format!("  {} (", ids[0])), "{report}");
    }

    /// An officer nothing in the pack names, to check that a path does not over-count.
    fn with_a_spare_officer(pack: &mut Pack) {
        let mut spare = pack.officers.values().next().expect("officers").clone();
        spare.id = "spare".into();
        spare.name = "여분".into();
        pack.officers.insert(spare.id.clone(), spare);
    }

    /// Every command that names an officer counts (`@join`, `@leave`, `@away`, `@level`,
    /// `@class`, `@duel`'s two fighters), and one nothing names stays unused.
    #[test]
    fn every_scene_command_that_names_an_officer_counts() {
        let mut pack = pack("../hero-core/tests/fixtures/mini");
        pack.campaign.starting_officers.clear();
        pack.battles.clear();
        pack.scenes.clear();
        with_a_spare_officer(&mut pack);
        let ids: Vec<String> = pack
            .officers
            .keys()
            .filter(|id| id.as_str() != "spare")
            .cloned()
            .collect();
        let scene = hero_core::script::parse_drama(
            "t.drama",
            &format!(
                "== s\n@join {}\n@leave {}\n@away {}\n@level {} 1\n@class {} bandit\n\
                 @duel {} {}\n",
                ids[0], ids[1], ids[2], ids[3], ids[4], ids[5], ids[4]
            ),
        )
        .unwrap()
        .remove(0);
        pack.scenes.insert(scene.id.clone(), scene);
        assert_eq!(
            used_officers(&pack),
            ids.iter().map(String::as_str).collect::<BTreeSet<_>>()
        );
        assert!(render(&pack).starts_with("1 of 7 officers are not used"));
    }

    /// A battle names its units' officers, its deployment, and the officers its conditions,
    /// triggers and actions refer to (`unit_refs`); a spawn tag names no officer.
    #[test]
    fn a_battle_names_its_units_deploy_and_unit_refs() {
        let mut pack = pack("../hero-core/tests/fixtures/mini");
        pack.campaign.starting_officers.clear();
        pack.scenes.clear();
        pack.battles.clear();
        with_a_spare_officer(&mut pack);
        let battle: hero_core::battledef::BattleDef = toml::from_str(
            r#"
id = "t"
name = "t"
objective = "t"
turn_limit = 5
victory = [{ type = "defeat_unit", target = "zhang_bao" }]
bonus = { condition = { type = "reach", who = "deng_mao", pos = [1, 1] }, exp = 1 }

[map]
rows = ".."
legend = { "." = "plain" }

[deploy]
max = 2
required = ["liu_bei"]
forbidden = ["guan_yu"]
slots = [[0, 0]]

[[units]]
side = "enemy"
officer = "zhang_fei"
pos = [1, 0]

[[units]]
side = "enemy"
name = "복병"
class = "bandit"
pos = [0, 1]
ai = "target"
ai_target = "jian_yong"

[[events]]
trigger = { type = "unit_defeated", target = "boss" }
actions = [{ type = "set_flag", flag = "captives", value = 1 }]
"#,
        )
        .unwrap();
        pack.battles.insert(battle.id.clone(), battle);
        assert_eq!(
            used_officers(&pack),
            BTreeSet::from([
                "liu_bei",
                "guan_yu",
                "zhang_fei",
                "jian_yong",
                "zhang_bao",
                "deng_mao",
            ])
        );
        // The one nothing names, and the `boss` tag (no officer of that id), stay unused.
        let report = render(&pack);
        assert!(
            report.starts_with("1 of 7 officers are not used"),
            "{report}"
        );
        assert!(report.contains("  spare ("), "{report}");
    }

    #[test]
    fn the_fixture_and_the_base_pack_report() {
        // The fixture's campaign and battles name its officers.
        let mini = pack("../hero-core/tests/fixtures/mini");
        let used = used_officers(&mini);
        assert!(mini
            .campaign
            .starting_officers
            .iter()
            .all(|o| used.contains(o.as_str())));
        for battle in mini.battles.values() {
            for o in battle.units.iter().filter_map(|u| u.officer.as_deref()) {
                assert!(used.contains(o), "{o}");
            }
        }
        // The base pack keeps its roster for the original mode: most are listed, with the
        // caveat; the starting army is not.
        let base = pack("../../data/base");
        let report = render(&base);
        assert!(report.contains("original mode"), "{report}");
        for o in &base.campaign.starting_officers {
            assert!(!report.contains(&format!("  {o} (")), "{o}");
        }
    }
}
