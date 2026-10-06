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
///   `@leave`, `@away`, `@level`, `@class`, `@duel`).
/// * Why: removing an officer nothing uses is safe for the pack; one that only looks unused
///   by name (a speaker written as a display name) is not counted, as the game does not tie
///   that line to the officer either. Every `Cmd` is matched without a catch-all, so a new
///   command that names an officer cannot be forgotten here.
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
                Cmd::Say { speaker: who, .. }
                | Cmd::Show { who, .. }
                | Cmd::Join(who)
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
        assert!(ids.len() >= 2, "{ids:?}");
        // Nothing names anyone: every officer is unused.
        pack.campaign.starting_officers.clear();
        pack.battles.clear();
        pack.scenes.clear();
        assert!(used_officers(&pack).is_empty());
        assert!(
            render(&pack).starts_with(&format!("{n} of {n} officers are not used", n = ids.len()))
        );
        // A speaker, a portrait or an event's target names one; a display name does not.
        let scene = hero_core::script::parse_drama(
            "t.drama",
            &format!(
                "== s\n{}: 안녕\n@show {} left\n관우: 이름만\n",
                ids[0], ids[1]
            ),
        )
        .unwrap()
        .remove(0);
        pack.scenes.insert(scene.id.clone(), scene);
        assert_eq!(
            used_officers(&pack),
            BTreeSet::from([ids[0].as_str(), ids[1].as_str()])
        );
        let report = render(&pack);
        assert!(!report.contains(&format!("  {} (", ids[0])), "{report}");
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
