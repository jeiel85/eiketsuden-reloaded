//! Command line parsing (no dependencies: the grammar is a handful of subcommands and options).

use hero_import::edition::EditionId;
use hero_import::extract::Selection;
use std::path::PathBuf;

pub const USAGE: &str = "\
hero-tools — command line tools for Eiketsuden Reloaded data packs

USAGE:
    hero-tools validate <pack_dir>
        Load the pack, cross-check every reference, report unknown TOML keys and missing
        media files. Exits with 1 when there are errors.

    hero-tools simulate <pack_dir> [--seeds N] [--battle ID]
        Play battles AI against AI (the player side is run by the AI too) with N seeds each
        (default 4), at most 200 phases per run. Reports win rates and average turns, warns
        about battles that are never or always won, and exits with 1 when a battle panics,
        does not finish or cannot be set up. A --battle ID the pack does not have is a
        command line error (exit 2).

    hero-tools simulate <pack_dir> --campaign [--seeds N] [--choose SCENE=N[,N...]]...
                             [--level-bonus N] [--trace ID]
                             [--difficulty easy|normal|hard] [--extended-rules]
        Play the whole campaign from a new game N times (default 4), AI against AI, carrying
        levels, recruits, items and flags from battle to battle: dramas run with their side
        effects, camps buy battle items (until 8 are in hand) and keep the camp screen's first
        selection, the player's units go for the battle's goal (an officer who must reach a
        tile marches for it), a lost battle follows its on_defeat or ends the run.
        --level-bonus N gives every army officer N levels once, before their first battle
        (a check of how far a stronger army gets, not of the balance). --trace ID writes every
        phase of battle ID to stderr (per seed: each unit's side, tile, HP and AI).
        --difficulty and --extended-rules start the campaign with those new-game choices
        (DECISIONS D25; RULES.md §4 joint attack, §7.6 difficulty), as the title's new game does.
        --choose takes option N (1 = first) at the
        choices of scene SCENE, one N per choice it asks in order (past them, and by default:
        the first option not taken yet at that question while the scene plays). Reports each run's end and, per battle, how often it was
        won, its average turns and the army's average level at its start; exits with 1 when a
        run panics, gets stuck or cannot go on, and with 2 when --choose names an unknown
        scene.

    hero-tools info <pack_dir>
        Print a summary of the pack's content.

    hero-tools unused-officers <pack_dir>
        List the officers that no battle, scene or the campaign's starting army names (by
        officer id; a scene's speakers and portraits also by display name). A pack that extends
        this one may still use them: run it on that pack too.

    A <pack_dir> whose pack.toml says `extends = \"../base\"` is loaded together with the
    packs it builds on; validate checks media files in every pack of that chain.

    hero-tools original probe <install_dir> [--out <manifest.json>]
        EXPERIMENTAL. Identify which release of the original game (that you own) a folder
        holds and, with --out, write a shareable manifest: file names, sizes, SHA-256
        hashes, the first 16 bytes of each file and container summaries, no game content.
        The folder is only read. See docs/ORIGINAL_DATA.md.

    hero-tools original extract <install_dir> --out <dir> [--text] [--sprites] [--portraits] [--maps]
                                [--edition korean-dos|chinese-dos]
        EXPERIMENTAL. Convert the original files into a media overlay folder (PNG + UTF-8
        JSON + index.json) for `eiketsuden --original <dir>`. Without a kind option every
        kind is attempted and unsupported ones are only reported; a kind chosen explicitly
        that cannot be extracted fails. --edition skips identification. The output folder
        must be new, empty or a previous extraction, and outside the install. Exits with 1
        when any kind failed.

    hero-tools original pack <install_dir> --out <pack_dir> [--base <pack_dir>]
                             [--edition korean-dos|chinese-dos]
        EXPERIMENTAL. Write the original mode: a data pack that extends the base pack
        (default: the `base` folder next to <pack_dir>, e.g. --out data/original) with the
        original art the base pack's keys can be mapped to — officer portraits, unit sheets
        and a 32-px battle-map tileset learned from the original maps, and the original's
        battle and main screen frames — on a 640×400 canvas.
        Play it with `eiketsuden --data <pack_dir>`. The folder must be new, empty or a
        previous pack of this command, and outside the install; the written pack is then
        validated. Exits with 1 when a kind failed or the pack does not validate.

    hero-tools help | --help | -h
    hero-tools --version";

/// Default number of seeds per battle for `simulate`.
pub const DEFAULT_SEEDS: u32 = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Validate {
        pack: PathBuf,
    },
    Simulate {
        pack: PathBuf,
        seeds: u32,
        battle: Option<String>,
    },
    SimulateCampaign {
        pack: PathBuf,
        seeds: u32,
        /// Scene id -> option index (0-based) at each choice the scene asks, in order.
        choose: std::collections::BTreeMap<String, Vec<usize>>,
        options: crate::campaign_sim::Options,
    },
    Info {
        pack: PathBuf,
    },
    UnusedOfficers {
        pack: PathBuf,
    },
    OriginalProbe {
        dir: PathBuf,
        out: Option<PathBuf>,
    },
    OriginalExtract {
        dir: PathBuf,
        out: PathBuf,
        /// `None` when no kind option was given.
        selection: Option<Selection>,
        edition: Option<EditionId>,
    },
    OriginalPack {
        dir: PathBuf,
        out: PathBuf,
        /// The pack to extend; `None` = `base` next to `out`.
        base: Option<PathBuf>,
        edition: Option<EditionId>,
    },
    Help,
    Version,
}

/// Parse the arguments after the program name.
pub fn parse(args: &[String]) -> Result<Command, String> {
    let Some((command, rest)) = args.split_first() else {
        return Err("no command given".into());
    };
    match command.as_str() {
        "help" | "--help" | "-h" => Ok(Command::Help),
        "--version" | "-V" => Ok(Command::Version),
        "validate" => Ok(Command::Validate {
            pack: only_pack(command, rest)?,
        }),
        "info" => Ok(Command::Info {
            pack: only_pack(command, rest)?,
        }),
        "unused-officers" => Ok(Command::UnusedOfficers {
            pack: only_pack(command, rest)?,
        }),
        "simulate" => parse_simulate(rest),
        "original" => parse_original(rest),
        other => Err(format!("unknown command `{other}`")),
    }
}

/// Split `--name=value` into `(--name, Some(value))`.
fn split_inline(arg: &str) -> (&str, Option<String>) {
    match arg.split_once('=') {
        Some((n, v)) if arg.starts_with("--") => (n, Some(v.to_string())),
        _ => (arg, None),
    }
}

fn parse_original(rest: &[String]) -> Result<Command, String> {
    let Some((sub, rest)) = rest.split_first() else {
        return Err("`original` needs a subcommand: probe, extract or pack".into());
    };
    let (extract, pack) = match sub.as_str() {
        "probe" => (false, false),
        "extract" => (true, false),
        "pack" => (false, true),
        other => return Err(format!("unknown `original` subcommand `{other}`")),
    };
    let command = format!("original {sub}");
    let mut dir = None;
    let mut out = None;
    let mut selection: Option<Selection> = None;
    let mut edition = None;
    let mut base = None;
    let mut args = rest.iter();
    while let Some(arg) = args.next() {
        let (name, inline) = split_inline(arg);
        let mut value = |what: &str| -> Result<String, String> {
            inline
                .clone()
                .or_else(|| args.next().cloned())
                .filter(|v| !v.is_empty())
                .ok_or_else(|| format!("`{name}` needs {what}"))
        };
        let mut select = |f: fn(&mut Selection)| {
            f(selection.get_or_insert_with(Selection::default));
        };
        match name {
            "--out" => out = Some(PathBuf::from(value("a path")?)),
            "--base" if pack => base = Some(PathBuf::from(value("a pack directory")?)),
            "--text" if extract => select(|s| s.text = true),
            "--sprites" if extract => select(|s| s.sprites = true),
            "--portraits" if extract => select(|s| s.portraits = true),
            "--maps" if extract => select(|s| s.maps = true),
            "--edition" if extract || pack => {
                let v = value("an edition id")?;
                edition = match EditionId::parse(&v) {
                    Some(id) if id.is_extractable() => Some(id),
                    _ => {
                        return Err(format!(
                            "`--edition` must be korean-dos or chinese-dos, got `{v}`"
                        ))
                    }
                };
            }
            flag if flag.starts_with('-') => {
                return Err(format!("unknown option `{flag}` for `{command}`"))
            }
            _ if dir.is_none() => dir = Some(PathBuf::from(arg)),
            _ => return Err(format!("`{command}` takes exactly one install directory")),
        }
    }
    let dir = dir.ok_or_else(|| format!("`{command}` needs an install directory"))?;
    if pack {
        Ok(Command::OriginalPack {
            dir,
            out: out.ok_or(
                "`original pack` needs `--out <pack_dir>` (e.g. data/original, next to data/base)",
            )?,
            base,
            edition,
        })
    } else if extract {
        Ok(Command::OriginalExtract {
            dir,
            out: out
                .ok_or("`original extract` needs `--out <dir>` (a folder outside the install)")?,
            selection,
            edition,
        })
    } else {
        Ok(Command::OriginalProbe { dir, out })
    }
}

fn only_pack(command: &str, rest: &[String]) -> Result<PathBuf, String> {
    match rest {
        [pack] if !pack.starts_with('-') => Ok(PathBuf::from(pack)),
        [] => Err(format!("`{command}` needs a pack directory")),
        [flag, ..] if flag.starts_with('-') => {
            Err(format!("unknown option `{flag}` for `{command}`"))
        }
        _ => Err(format!("`{command}` takes exactly one pack directory")),
    }
}

fn parse_simulate(rest: &[String]) -> Result<Command, String> {
    let mut pack = None;
    let mut seeds = DEFAULT_SEEDS;
    let mut battle = None;
    let mut campaign = false;
    let mut choose = std::collections::BTreeMap::new();
    let mut level_bonus = None;
    let mut trace = None;
    let mut game: Option<hero_core::campaign::GameOptions> = None;
    let mut args = rest.iter();
    while let Some(arg) = args.next() {
        // Accept both `--seeds 8` and `--seeds=8`.
        let (name, inline) = match arg.split_once('=') {
            Some((n, v)) if arg.starts_with("--") => (n, Some(v.to_string())),
            _ => (arg.as_str(), None),
        };
        let mut value = |what: &str| -> Result<String, String> {
            inline
                .clone()
                .or_else(|| args.next().cloned())
                .filter(|v| !v.is_empty())
                .ok_or_else(|| format!("`{name}` needs {what}"))
        };
        match name {
            "--seeds" => {
                let v = value("a number")?;
                seeds = match v.parse::<u32>() {
                    Ok(n) if n > 0 => n,
                    _ => return Err(format!("`--seeds` needs a positive number, got `{v}`")),
                };
            }
            "--battle" => battle = Some(value("a battle id")?),
            "--trace" => trace = Some(value("a battle id")?),
            "--level-bonus" => {
                let v = value("a number")?;
                level_bonus =
                    Some(v.parse::<u32>().map_err(|_| {
                        format!("`--level-bonus` needs a number of levels, got `{v}`")
                    })?);
            }
            "--difficulty" => {
                use hero_core::campaign::Difficulty;
                let v = value("easy, normal or hard")?;
                let difficulty = match v.as_str() {
                    "easy" => Difficulty::Easy,
                    "normal" => Difficulty::Normal,
                    "hard" => Difficulty::Hard,
                    _ => {
                        return Err(format!(
                            "`--difficulty` needs easy, normal or hard, got `{v}`"
                        ))
                    }
                };
                game.get_or_insert_with(Default::default).difficulty = difficulty;
            }
            "--extended-rules" if inline.is_some() => {
                return Err("`--extended-rules` takes no value".into())
            }
            "--extended-rules" => game.get_or_insert_with(Default::default).extended_rules = true,
            "--campaign" if inline.is_some() => return Err("`--campaign` takes no value".into()),
            "--campaign" => campaign = true,
            "--choose" => {
                let v = value("SCENE=N")?;
                let bad = || {
                    format!("`--choose` needs SCENE=N or SCENE=N,N,... with N from 1, got `{v}`")
                };
                let (scene, list) = v.rsplit_once('=').ok_or_else(bad)?;
                let options = list
                    .split(',')
                    .map(|n| {
                        n.trim()
                            .parse::<usize>()
                            .ok()
                            .filter(|&n| n > 0)
                            .map(|n| n - 1)
                    })
                    .collect::<Option<Vec<usize>>>()
                    .ok_or_else(bad)?;
                if scene.is_empty() {
                    return Err(bad());
                }
                if choose.insert(scene.to_string(), options).is_some() {
                    return Err(format!("`--choose` names scene `{scene}` twice"));
                }
            }
            flag if flag.starts_with('-') => {
                return Err(format!("unknown option `{flag}` for `simulate`"))
            }
            _ if pack.is_none() => pack = Some(PathBuf::from(arg)),
            _ => return Err("`simulate` takes exactly one pack directory".into()),
        }
    }
    let pack = pack.ok_or("`simulate` needs a pack directory")?;
    if campaign {
        if battle.is_some() {
            return Err("`--battle` and `--campaign` cannot be combined".into());
        }
        return Ok(Command::SimulateCampaign {
            pack,
            seeds,
            choose,
            options: crate::campaign_sim::Options {
                level_bonus: level_bonus.unwrap_or(0),
                trace,
                game: game.unwrap_or_default(),
            },
        });
    }
    if !choose.is_empty() {
        return Err("`--choose` needs `--campaign`".into());
    }
    if level_bonus.is_some() {
        return Err("`--level-bonus` needs `--campaign`".into());
    }
    if trace.is_some() {
        return Err("`--trace` needs `--campaign`".into());
    }
    if game.is_some() {
        return Err("`--difficulty` and `--extended-rules` need `--campaign`".into());
    }
    Ok(Command::Simulate {
        pack,
        seeds,
        battle,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_str(args: &[&str]) -> Result<Command, String> {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        parse(&args)
    }

    #[test]
    fn subcommands() {
        assert_eq!(
            parse_str(&["validate", "data/base"]),
            Ok(Command::Validate {
                pack: "data/base".into()
            })
        );
        assert_eq!(
            parse_str(&["info", "data/base"]),
            Ok(Command::Info {
                pack: "data/base".into()
            })
        );
        assert_eq!(
            parse_str(&["unused-officers", "data/base"]),
            Ok(Command::UnusedOfficers {
                pack: "data/base".into()
            })
        );
        assert!(parse_str(&["unused-officers"]).is_err());
        assert_eq!(parse_str(&["--help"]), Ok(Command::Help));
        assert_eq!(parse_str(&["help"]), Ok(Command::Help));
        assert_eq!(parse_str(&["--version"]), Ok(Command::Version));
    }

    #[test]
    fn simulate_options() {
        assert_eq!(
            parse_str(&["simulate", "data/base"]),
            Ok(Command::Simulate {
                pack: "data/base".into(),
                seeds: DEFAULT_SEEDS,
                battle: None
            })
        );
        let expected = Ok(Command::Simulate {
            pack: "p".into(),
            seeds: 8,
            battle: Some("b03".into()),
        });
        assert_eq!(
            parse_str(&["simulate", "p", "--seeds", "8", "--battle", "b03"]),
            expected
        );
        assert_eq!(
            parse_str(&["simulate", "--battle=b03", "--seeds=8", "p"]),
            expected
        );
        assert_eq!(
            parse_str(&[
                "simulate",
                "p",
                "--campaign",
                "--choose",
                "oath=2,1",
                "--choose=a=b=1"
            ]),
            Ok(Command::SimulateCampaign {
                pack: "p".into(),
                seeds: DEFAULT_SEEDS,
                choose: [
                    ("oath".to_string(), vec![1, 0]),
                    ("a=b".to_string(), vec![0])
                ]
                .into(),
                options: Default::default(),
            })
        );
        assert_eq!(
            parse_str(&[
                "simulate",
                "p",
                "--campaign",
                "--level-bonus",
                "3",
                "--trace=b01"
            ]),
            Ok(Command::SimulateCampaign {
                pack: "p".into(),
                seeds: DEFAULT_SEEDS,
                choose: Default::default(),
                options: crate::campaign_sim::Options {
                    level_bonus: 3,
                    trace: Some("b01".into()),
                    ..Default::default()
                },
            })
        );
        assert_eq!(
            parse_str(&[
                "simulate",
                "p",
                "--campaign",
                "--difficulty=hard",
                "--extended-rules"
            ]),
            Ok(Command::SimulateCampaign {
                pack: "p".into(),
                seeds: DEFAULT_SEEDS,
                choose: Default::default(),
                options: crate::campaign_sim::Options {
                    game: hero_core::campaign::GameOptions {
                        difficulty: hero_core::campaign::Difficulty::Hard,
                        free_edit: false,
                        extended_rules: true,
                    },
                    ..Default::default()
                },
            })
        );
        for (args, msg) in [
            (
                &["simulate", "p", "--level-bonus", "2"][..],
                "`--level-bonus` needs `--campaign`",
            ),
            (
                &["simulate", "p", "--trace", "b"][..],
                "`--trace` needs `--campaign`",
            ),
            (
                &["simulate", "p", "--campaign", "--trace"][..],
                "needs a battle id",
            ),
            (
                &["simulate", "p", "--difficulty", "easy"][..],
                "need `--campaign`",
            ),
            (
                &["simulate", "p", "--extended-rules"][..],
                "need `--campaign`",
            ),
            (
                &["simulate", "p", "--campaign", "--difficulty", "brutal"][..],
                "easy, normal or hard, got `brutal`",
            ),
            (
                &["simulate", "p", "--campaign", "--extended-rules=1"][..],
                "takes no value",
            ),
            (
                &["simulate", "p", "--campaign", "--level-bonus", "x"][..],
                "number of levels",
            ),
            (
                &["simulate", "p", "--campaign", "--battle", "b"][..],
                "cannot be combined",
            ),
            (
                &["simulate", "p", "--choose", "s=1"][..],
                "needs `--campaign`",
            ),
            (
                &["simulate", "p", "--campaign", "--choose", "s=0"][..],
                "N from 1",
            ),
            (
                &["simulate", "p", "--campaign", "--choose", "s"][..],
                "SCENE=N",
            ),
            (
                &["simulate", "p", "--campaign", "--choose", "=2"][..],
                "SCENE=N",
            ),
            (&["simulate", "p", "--campaign=yes"][..], "takes no value"),
            (
                &["simulate", "p", "--campaign", "--choose", "s=1,x"][..],
                "SCENE=N",
            ),
            (
                &[
                    "simulate",
                    "p",
                    "--campaign",
                    "--choose",
                    "s=1",
                    "--choose",
                    "s=2",
                ][..],
                "twice",
            ),
        ] {
            let err = parse_str(args).unwrap_err();
            assert!(err.contains(msg), "{args:?}: {err}");
        }
    }

    #[test]
    fn original_commands() {
        assert_eq!(
            parse_str(&["original", "probe", "D:/Games/GAME"]),
            Ok(Command::OriginalProbe {
                dir: "D:/Games/GAME".into(),
                out: None
            })
        );
        assert_eq!(
            parse_str(&["original", "probe", "g", "--out=m.json"]),
            Ok(Command::OriginalProbe {
                dir: "g".into(),
                out: Some("m.json".into())
            })
        );
        assert_eq!(
            parse_str(&["original", "extract", "g", "--out", "o"]),
            Ok(Command::OriginalExtract {
                dir: "g".into(),
                out: "o".into(),
                selection: None,
                edition: None
            })
        );
        assert_eq!(
            parse_str(&["original", "pack", "g", "--out", "data/original"]),
            Ok(Command::OriginalPack {
                dir: "g".into(),
                out: "data/original".into(),
                base: None,
                edition: None
            })
        );
        assert_eq!(
            parse_str(&[
                "original",
                "pack",
                "--base=b",
                "g",
                "--out=o",
                "--edition",
                "korean-dos"
            ]),
            Ok(Command::OriginalPack {
                dir: "g".into(),
                out: "o".into(),
                base: Some("b".into()),
                edition: Some(EditionId::KoreanDos)
            })
        );
        assert_eq!(
            parse_str(&[
                "original",
                "extract",
                "--text",
                "g",
                "--portraits",
                "--out",
                "o",
                "--edition=chinese-dos"
            ]),
            Ok(Command::OriginalExtract {
                dir: "g".into(),
                out: "o".into(),
                selection: Some(Selection {
                    text: true,
                    portraits: true,
                    sprites: false,
                    maps: false
                }),
                edition: Some(EditionId::ChineseDos)
            })
        );
    }

    #[test]
    fn usage_errors() {
        for (args, fragment) in [
            (&[][..], "no command"),
            (&["frobnicate"][..], "unknown command"),
            (&["validate"][..], "needs a pack directory"),
            (&["validate", "a", "b"][..], "exactly one pack directory"),
            (&["validate", "--fast"][..], "unknown option `--fast`"),
            (&["simulate"][..], "needs a pack directory"),
            (&["simulate", "p", "q"][..], "exactly one pack directory"),
            (
                &["simulate", "p", "--seeds"][..],
                "`--seeds` needs a number",
            ),
            (&["simulate", "p", "--seeds", "0"][..], "positive number"),
            (&["simulate", "p", "--seeds", "many"][..], "positive number"),
            (
                &["simulate", "p", "--battle="][..],
                "`--battle` needs a battle id",
            ),
            (
                &["simulate", "p", "--turbo"][..],
                "unknown option `--turbo`",
            ),
            (&["original"][..], "needs a subcommand"),
            (
                &["original", "convert", "g"][..],
                "unknown `original` subcommand",
            ),
            (&["original", "probe"][..], "needs an install directory"),
            (&["original", "probe", "a", "b"][..], "exactly one install"),
            (
                &["original", "probe", "g", "--text"][..],
                "unknown option `--text` for `original probe`",
            ),
            (
                &["original", "probe", "g", "--out"][..],
                "`--out` needs a path",
            ),
            (&["original", "extract", "g"][..], "needs `--out <dir>`"),
            (&["original", "pack", "g"][..], "needs `--out <pack_dir>`"),
            (
                &["original", "extract", "g", "--out", "o", "--base", "b"][..],
                "unknown option `--base`",
            ),
            (
                &["original", "pack", "g", "--out", "o", "--text"][..],
                "unknown option `--text` for `original pack`",
            ),
            (
                &[
                    "original",
                    "extract",
                    "g",
                    "--out",
                    "o",
                    "--edition",
                    "steam-2017",
                ][..],
                "must be korean-dos or chinese-dos",
            ),
        ] {
            let err = parse_str(args).unwrap_err();
            assert!(err.contains(fragment), "{args:?}: {err}");
        }
    }
}
