//! `hero-tools original probe|extract|pack`: the experimental importer for a legally owned copy of
//! the original game (see `docs/ORIGINAL_DATA.md` and the `hero-import` crate).

use hero_core::pack::{DirSource, Severity};
use hero_import::edition::{Edition, EditionId};
use hero_import::extract::{self, Index, KindReport, Options, Selection, Status};
use hero_import::install::lies_inside;
use hero_import::pack::{self, PackIndex, PackOptions};
use hero_import::probe::{self, Manifest};
use std::fmt::Write as _;
use std::path::{Component, Path, PathBuf};

/// `original probe`: print what the folder holds and optionally save the shareable manifest.
pub fn run_probe(dir: &Path, out: Option<&Path>) -> Result<bool, String> {
    if let Some(out) = out {
        let inside = lies_inside(out, dir).map_err(|e| format!("{}: {e}", out.display()))?;
        if inside {
            return Err(format!(
                "{} lies inside the probed folder; write the manifest somewhere else (the folder is only read)",
                out.display()
            ));
        }
    }
    let manifest = probe::probe(dir).map_err(|e| e.to_string())?;
    print!("{}", render_probe(dir, &manifest));
    match out {
        Some(out) => {
            let mut json = serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?;
            json.push(b'\n');
            std::fs::write(out, json).map_err(|e| format!("{}: {e}", out.display()))?;
            println!(
                "Manifest:  written to {} — file names, sizes, SHA-256 hashes, the first 16 bytes of\n           \
                 each file and container summaries, no game content. Check the file list before sharing it.",
                out.display()
            );
        }
        None => {
            println!("Manifest:  not written (add --out <file> to save the shareable manifest)")
        }
    }
    Ok(true)
}

fn render_edition(out: &mut String, edition: &Edition) {
    let _ = writeln!(
        out,
        "Edition:   {} [{}], confidence {}{}",
        edition.name,
        edition.id.as_str(),
        edition.confidence.as_str(),
        if edition.forced {
            " (chosen with --edition)"
        } else {
            ""
        }
    );
    for line in &edition.evidence {
        let _ = writeln!(out, "           - {line}");
    }
}

/// What the importer can do for an edition, in one line.
fn support_line(id: EditionId) -> &'static str {
    match id {
        EditionId::KoreanDos | EditionId::ChineseDos => {
            "text, sprites and portraits can be extracted (`hero-tools original extract`: \
             scenario scripts and dialogue, BAKDATA officers / items / townsfolk, unit frames, \
             map icons, chips, battle UI icons, palettes, contact sheets and TF-DCE portraits), \
             and so can battle, campaign and town maps; opening / ending pictures are not read yet"
        }
        EditionId::Steam2017 => {
            "not extractable yet: the Steam container format is unknown. Sharing this manifest \
             helps add support (docs/ORIGINAL_DATA.md)"
        }
        EditionId::Pc98Images => {
            "disk images recognised; reading the files inside them is not implemented yet"
        }
        EditionId::Unknown => "no supported edition recognised (see the evidence above)",
    }
}

pub fn render_probe(dir: &Path, m: &Manifest) -> String {
    let mut out = format!("Probed:    {} (read-only)\n", dir.display());
    render_edition(&mut out, &m.edition);
    let s = &m.summary;
    let _ = writeln!(
        out,
        "Files:     {} files, {} bytes; {} LS11 archives ({} failing validation), {} 6-byte tables, {} disk images",
        s.files, s.total_bytes, s.ls11_archives, s.ls11_failures, s.table6_containers, s.disk_images
    );
    for f in &m.files {
        let error = f
            .ls11
            .as_ref()
            .and_then(|l| l.error.as_ref().or(l.decode_error.as_ref()))
            .or(f.error.as_ref());
        if let Some(e) = error {
            let _ = writeln!(out, "           ! {}: {e}", f.path);
        }
    }
    if m.truncated {
        let _ = writeln!(
            out,
            "           ! stopped after {} files; point the probe at the game folder itself",
            probe::MAX_FILES
        );
    }
    if !m.skipped.is_empty() {
        let _ = writeln!(
            out,
            "           {} paths skipped (listed in the manifest)",
            m.skipped.len()
        );
    }
    let _ = writeln!(out, "Support:   {}", support_line(m.edition.id));
    out
}

/// `original extract`: convert into a media overlay folder. `Ok(false)` when a kind failed.
pub fn run_extract(
    dir: &Path,
    out: &Path,
    selection: Option<Selection>,
    edition: Option<EditionId>,
) -> Result<bool, String> {
    let options = Options { selection, edition };
    let index = extract::extract(dir, out, &options).map_err(|e| e.to_string())?;
    print!("{}", render_extract(dir, out, &index));
    Ok(index.success())
}

fn status_word(status: Status) -> &'static str {
    match status {
        Status::Extracted => "extracted",
        Status::Partial => "partial",
        Status::Failed => "FAILED",
        Status::Unsupported => "unsupported",
        Status::MissingSource => "no source",
    }
}

fn render_kind(out: &mut String, kind: &str, r: &KindReport) {
    let marker = if r.ok() { " " } else { "!" };
    let _ = writeln!(
        out,
        "{marker} {kind:<10} {:<12} {}",
        status_word(r.status),
        r.summary
    );
    for e in &r.errors {
        let _ = writeln!(out, "    error: {e}");
    }
    for n in &r.notes {
        let _ = writeln!(out, "    note:  {n}");
    }
}

pub fn render_extract(dir: &Path, out_dir: &Path, index: &Index) -> String {
    let mut out = format!("Extracted: {} -> {}\n", dir.display(), out_dir.display());
    render_edition(&mut out, &index.edition);
    for (kind, report) in &index.assets {
        render_kind(&mut out, kind, report);
    }
    let _ = writeln!(
        out,
        "Wrote {} files and {} to {}.",
        index.files.len(),
        extract::INDEX_FILE,
        out_dir.display()
    );
    if index.success() {
        let _ = writeln!(
            out,
            "Use them in the game (native builds): eiketsuden --original \"{}\"",
            out_dir.display()
        );
    } else {
        let _ = writeln!(
            out,
            "Some asset kinds were not extracted (marked with !); see above."
        );
    }
    out
}

/// The directory `to` relative to the directory `from` (which need not exist yet), with `/`
/// separators, for `extends`.
pub fn relative_dir(from: &Path, to: &Path) -> Result<String, String> {
    let to = to
        .canonicalize()
        .map_err(|e| format!("{}: {e}", to.display()))?;
    // Canonicalize the part of `from` that exists and append the rest.
    let mut existing = from.to_path_buf();
    let mut rest = Vec::new();
    while !existing.exists() {
        let name = existing
            .file_name()
            .ok_or_else(|| format!("{}: not a usable folder path", from.display()))?
            .to_os_string();
        rest.push(name);
        existing = match existing.parent() {
            Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
            _ => PathBuf::from("."),
        };
    }
    let mut from_abs = existing
        .canonicalize()
        .map_err(|e| format!("{}: {e}", existing.display()))?;
    for name in rest.into_iter().rev() {
        from_abs.push(name);
    }
    let a: Vec<Component> = from_abs.components().collect();
    let b: Vec<Component> = to.components().collect();
    let common = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    if common == 0 {
        return Err(format!(
            "{} and {} are on different drives; `extends` must be a relative path, so put the \
             pack next to the base pack (e.g. data/original next to data/base)",
            from.display(),
            to.display()
        ));
    }
    let mut parts: Vec<String> = vec!["..".to_string(); a.len() - common];
    for c in &b[common..] {
        let Component::Normal(name) = c else {
            return Err(format!("{}: unexpected path component", to.display()));
        };
        parts.push(
            name.to_str()
                .ok_or_else(|| format!("{}: the path is not UTF-8", to.display()))?
                .to_string(),
        );
    }
    if b.len() == common || a.len() == common {
        return Err(format!(
            "{} and the pack it extends, {}, must be separate folders (neither inside the other)",
            from.display(),
            to.display()
        ));
    }
    Ok(parts.join("/"))
}

/// `original pack`: write the original-mode pack on top of `base` (default: `base` next to
/// `out`) and validate it. `Ok(false)` when a kind failed or the written pack has errors.
pub fn run_pack(
    dir: &Path,
    out: &Path,
    base: Option<&Path>,
    edition: Option<EditionId>,
) -> Result<bool, String> {
    let base = match base {
        Some(b) => b.to_path_buf(),
        None => out
            .parent()
            .map(|p| p.join("base"))
            .ok_or_else(|| format!("{}: cannot find the base pack next to it", out.display()))?,
    };
    let parent = crate::load_pack(&base)?;
    let extends = relative_dir(out, &base)?;
    let fingerprint = pack::base_fingerprint(&parent, &DirSource { root: base.clone() })
        .map_err(|e| format!("cannot read the base pack {}: {e}", base.display()))?;
    let options = PackOptions {
        // Written once, so the music (several seconds to render) is worth it here.
        music: true,
        base_fingerprint: Some(fingerprint),
        ..PackOptions::for_pack(&parent, extends, edition)
    };
    let index = pack::write_pack(dir, out, &options).map_err(|e| e.to_string())?;
    print!("{}", render_pack(dir, out, &index));
    let written = crate::load_pack(out)?;
    let issues = crate::validate::check(out, &written)?;
    println!("\nValidation of the written pack:");
    print!("{}", crate::validate::render(&written, &issues));
    let valid = !issues.iter().any(|i| i.severity == Severity::Error);
    if index.success() && valid {
        println!(
            "Play it (native builds): eiketsuden --data \"{}\"",
            out.display()
        );
    }
    Ok(index.success() && valid)
}

pub fn render_pack(dir: &Path, out_dir: &Path, index: &PackIndex) -> String {
    let mut out = format!(
        "Original pack: {} -> {} (extends {}, canvas {}×{})\n",
        dir.display(),
        out_dir.display(),
        index.extends,
        index.canvas[0],
        index.canvas[1]
    );
    render_edition(&mut out, &index.edition);
    for (kind, report) in &index.assets {
        render_kind(&mut out, kind, report);
    }
    let _ = writeln!(
        out,
        "Wrote {} files and {} to {}.",
        index.files.len(),
        pack::PACK_INDEX,
        out_dir.display()
    );
    if !index.success() {
        let _ = writeln!(
            out,
            "Some asset kinds were not converted (marked with !); see above."
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use hero_import::text::{build_messages, TextEncoding};
    use hero_import::{ls11, palette, scenario, table6};
    use std::path::PathBuf;

    struct Temp(PathBuf);

    impl Temp {
        fn new(label: &str) -> Temp {
            let p = std::env::temp_dir().join(format!(
                "hero-tools-original-{}-{label}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            Temp(p)
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A tiny synthetic Korean DOS/V install made with the importer's own encoders.
    fn synthetic_install(dir: &Path) {
        let enc = |s: &str| TextEncoding::EucKr.encode(s).unwrap();
        std::fs::write(dir.join("DISK1.R3I"), enc("DOS/V 삼국지영걸전 1")).unwrap();
        let mut exe = b"MZ".to_vec();
        exe.extend(palette::build_bank(&[[[1, 2, 3]; 16]; palette::SLOTS]));
        std::fs::write(dir.join("MAIN.EXE"), exe).unwrap();
        // One scene whose script narrates the section's only string.
        let text = build_messages(&[vec![enc("가나다라마바사")]]);
        std::fs::write(dir.join("SNR0M.R3"), text).unwrap();
        let scene = scenario::build_scene(&[vec![([0; 8], vec![0x08, 0, 0, 0xff])]]);
        std::fs::write(dir.join("SNR0D.R3"), ls11::build(&[&scene])).unwrap();
        std::fs::write(dir.join("HEXBCHR.R3"), ls11::build(&[&[0x55; 128 * 9]])).unwrap();
        // One 8×1 TF-DCE image: planes 0–2 filled with 0x80, 0, 0 (methods 1, 1, 1, 0).
        let face: &[u8] = &[2, b'T', 1, 1, 0, 0x11, 0x01, 0xE4, 0, 0x80, 0, 0];
        std::fs::write(dir.join("FACEDAT.R3"), table6::build(&[face]).unwrap()).unwrap();
    }

    #[test]
    fn probe_writes_a_manifest_outside_the_install() {
        let tmp = Temp::new("probe");
        let game = tmp.0.join("GAME");
        std::fs::create_dir(&game).unwrap();
        synthetic_install(&game);

        let manifest = tmp.0.join("manifest.json");
        assert_eq!(run_probe(&game, Some(manifest.as_path())), Ok(true));
        let json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
        assert_eq!(json["edition"]["id"], "korean-dos");
        assert_eq!(json["format"], "eiketsuden-original-probe");

        let err = run_probe(&game, Some(game.join("m.json").as_path())).unwrap_err();
        assert!(err.contains("inside the probed folder"), "{err}");
        assert!(!game.join("m.json").exists());

        let text = render_probe(&game, &probe::probe(&game).unwrap());
        assert!(text.contains("[korean-dos], confidence high"), "{text}");
        assert!(
            text.contains("text, sprites and portraits can be extracted"),
            "{text}"
        );
    }

    #[test]
    fn extract_reports_every_kind_and_exit_status() {
        let tmp = Temp::new("extract");
        let game = tmp.0.join("GAME");
        std::fs::create_dir(&game).unwrap();
        synthetic_install(&game);

        let out = tmp.0.join("overlay");
        assert_eq!(run_extract(&game, &out, None, None), Ok(true));
        assert!(out.join("index.json").is_file());
        assert!(out.join("text/snr0.json").is_file());
        assert!(out.join("gfx/original/hexbchr/000.png").is_file());

        // Portraits alone.
        let portraits = Some(Selection {
            portraits: true,
            ..Selection::default()
        });
        assert_eq!(run_extract(&game, &out, portraits, None), Ok(true));
        assert!(out.join("gfx/original/facedat/000.png").is_file());

        let index = extract::extract(&game, &out, &Options::default()).unwrap();
        let text = render_extract(&game, &out, &index);
        assert!(text.contains("portraits  extracted"), "{text}");
        assert!(text.contains("eiketsuden --original"), "{text}");

        // Explicitly asking for a kind that fails is a failure.
        std::fs::write(game.join("FACEDAT.R3"), table6::build(&[b"x"]).unwrap()).unwrap();
        assert_eq!(run_extract(&game, &out, portraits, None), Ok(false));
        let index = extract::extract(&game, &out, &Options::default()).unwrap();
        let text = render_extract(&game, &out, &index);
        assert!(text.contains("portraits  FAILED"), "{text}");

        // Not an install at all: a clear error.
        let err = run_extract(&tmp.0.join("nope"), &out, None, None).unwrap_err();
        assert!(err.contains("nope"), "{err}");
    }

    fn copy_dir(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for entry in std::fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            let target = to.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy_dir(&entry.path(), &target);
            } else {
                std::fs::copy(entry.path(), target).unwrap();
            }
        }
    }

    #[test]
    fn relative_dirs_for_extends() {
        let tmp = Temp::new("relative");
        let base = tmp.0.join("data/base");
        std::fs::create_dir_all(&base).unwrap();
        assert_eq!(
            relative_dir(&tmp.0.join("data/original"), &base),
            Ok("../base".into())
        );
        assert_eq!(
            relative_dir(&tmp.0.join("mods/deep/original"), &base),
            Ok("../../../data/base".into())
        );
        std::fs::create_dir_all(tmp.0.join("data/original")).unwrap();
        assert_eq!(
            relative_dir(&tmp.0.join("data/original"), &base),
            Ok("../base".into())
        );
        for inside in [base.clone(), base.join("sub"), tmp.0.join("data")] {
            let err = relative_dir(&inside, &base).unwrap_err();
            assert!(err.contains("separate folders"), "{err}");
        }
        assert!(relative_dir(&tmp.0.join("x"), &tmp.0.join("missing")).is_err());
    }

    #[test]
    fn pack_is_written_next_to_its_base_and_validated() {
        let tmp = Temp::new("pack");
        let game = tmp.0.join("GAME");
        std::fs::create_dir(&game).unwrap();
        synthetic_install(&game);
        copy_dir(&crate::tests::fixture_dir(), &tmp.0.join("data/base"));
        let out = tmp.0.join("data/original");
        // The tiny install has no battle maps and the fixture's classes are not the original's:
        // the pack is written and loads, but the conversion reports failures.
        assert_eq!(run_pack(&game, &out, None, None), Ok(false));
        let manifest = std::fs::read_to_string(out.join("pack.toml")).unwrap();
        assert!(manifest.contains("extends = \"../base\""), "{manifest}");
        let pack = crate::load_pack(&out).unwrap();
        assert_eq!(pack.layers.len(), 2);
        assert_eq!(pack.manifest.presentation.canvas, [640, 400]);
        // Written from the base pack as it is: up to date, until the base pack changes.
        assert_eq!(pack::stale_pack(&out), Ok(None));
        let stale = |issues: &[hero_core::pack::Issue]| {
            issues
                .iter()
                .any(|i| i.context.ends_with(pack::PACK_INDEX) && i.severity == Severity::Warning)
        };
        assert!(!stale(&crate::validate::check(&out, &pack).unwrap()));
        let base_manifest = tmp.0.join("data/base/pack.toml");
        let mut text = std::fs::read_to_string(&base_manifest).unwrap();
        text.push_str("\n# changed\n");
        std::fs::write(&base_manifest, text).unwrap();
        let why = pack::stale_pack(&out).unwrap().unwrap();
        assert!(why.contains("changed after it was written"), "{why}");
        assert!(stale(&crate::validate::check(&out, &pack).unwrap()));
        // A mod on top of the original pack: the original pack in its chain is checked too.
        let modded = tmp.0.join("data/mod");
        std::fs::create_dir_all(&modded).unwrap();
        std::fs::write(
            modded.join("pack.toml"),
            "id = \"mod\"\nname = \"mod\"\nversion = \"1\"\nextends = \"../original\"\n",
        )
        .unwrap();
        let mod_pack = crate::load_pack(&modded).unwrap();
        let found = pack::stale_packs(&modded, &mod_pack);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].0, tmp.0.join("data/original"), "{found:?}");
        let issues = crate::validate::check(&modded, &mod_pack).unwrap();
        assert!(
            issues.iter().any(|i| i.severity == Severity::Warning
                && i.context.ends_with(pack::PACK_INDEX)
                && i.msg.contains("changed after it was written")),
            "{issues:?}"
        );
        // An index of another pack format: written by another converter.
        let index_path = out.join(pack::PACK_INDEX);
        let mut json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&index_path).unwrap()).unwrap();
        json["format_version"] = (pack::PACK_FORMAT_VERSION - 1).into();
        std::fs::write(&index_path, serde_json::to_vec(&json).unwrap()).unwrap();
        let why = pack::stale_pack(&out).unwrap().unwrap();
        assert!(why.contains("another version of the converter"), "{why}");
        // Not an original pack: nothing to say.
        assert_eq!(pack::stale_pack(&tmp.0.join("data/base")), Ok(None));
        let index = pack::write_pack(
            &game,
            &out,
            &PackOptions {
                extends: "../base".into(),
                ..PackOptions::default()
            },
        )
        .unwrap();
        let text = render_pack(&game, &out, &index);
        assert!(text.contains("tiles"), "{text}");
        assert!(text.contains("FAILED"), "{text}");
        // No base pack next to the output folder.
        let err = run_pack(&game, &tmp.0.join("elsewhere/original"), None, None).unwrap_err();
        assert!(err.contains("no pack.toml"), "{err}");
    }

    /// An install with two battle maps (one of them with a cell of unknown terrain code) and
    /// the `MAIN.EXE` tables and palette the map conversion needs.
    fn map_install(dir: &Path) {
        use hero_import::maps::{self, BattleMap};
        synthetic_install(dir);
        let names: Vec<&[u8]> = maps::TERRAIN_IDS.iter().map(|s| s.as_bytes()).collect();
        let mut exe = maps::build_exe_fixture(
            &maps::ExeFixture {
                second_set_maps: &[1],
                backdrop: [0; maps::TERRAIN_COUNT],
                ground: [5; maps::TERRAIN_COUNT],
                terrain_names: &names,
                campaign_sizes: [(16, 4); maps::CHAPTERS],
            },
            0,
        );
        exe.extend(palette::build_bank(&[[[1, 2, 3]; 16]; palette::SLOTS]));
        std::fs::write(dir.join("MAIN.EXE"), exe).unwrap();
        let cells = |n: usize| -> Vec<u8> { (0..n * 128).map(|i| (i % 7) as u8).collect() };
        let (common, first, second) = (cells(80), cells(4), cells(4));
        std::fs::write(
            dir.join("HEXZCHP.R3"),
            ls11::build(&[&common, &first, &second]),
        )
        .unwrap();
        // Cell (x, y) of terrain code c shows chips 4c .. 4c+3 (all shared chips).
        let map = |terrain: &[&[u8]]| {
            let (w, h) = (terrain[0].len(), terrain.len());
            let mut chips = vec![0; 4 * w * h];
            for (y, row) in terrain.iter().enumerate() {
                for (x, &code) in row.iter().enumerate() {
                    for (i, (dx, dy)) in [(0, 0), (1, 0), (0, 1), (1, 1)].into_iter().enumerate() {
                        chips[(2 * y + dy) * 2 * w + 2 * x + dx] = 4 * code.min(19) + i as u8;
                    }
                }
            }
            BattleMap {
                width: 2 * w,
                height: 2 * h,
                chips,
                terrain: terrain.concat(),
            }
        };
        let a = map(&[&[0, 1, 2, 0], &[3, 4, 3, 3], &[6, 5, 8, 0]]);
        let mut b = map(&[&[0, 6], &[0, 6]]);
        // A code without pack terrain (18, fire) on castle chips: the stand-in is castle.
        b.terrain[1] = 18;
        b.chips[2..4].copy_from_slice(&[24, 25]);
        b.chips[6..8].copy_from_slice(&[26, 27]);
        let enc = |s: &str| TextEncoding::EucKr.encode(s).unwrap();
        let mut names = enc("평원1\r\n성");
        names.extend_from_slice(b"\r\n\r\n\x1a");
        std::fs::write(
            dir.join("HEXZMAP.R3"),
            ls11::build(&[&a.encode(), &b.encode(), &names]),
        )
        .unwrap();
    }

    #[test]
    fn original_maps_load_and_validate_on_the_base_pack() {
        let tmp = Temp::new("maps");
        let game = tmp.0.join("GAME");
        std::fs::create_dir(&game).unwrap();
        map_install(&game);
        copy_dir(&crate::tests::fixture_dir(), &tmp.0.join("data/base"));
        let out = tmp.0.join("data/original");
        // Portraits and unit sheets fail on this install and fixture; the maps do not.
        assert_eq!(run_pack(&game, &out, None, None), Ok(false));
        let index: serde_json::Value =
            serde_json::from_slice(&std::fs::read(out.join(pack::PACK_INDEX)).unwrap()).unwrap();
        assert_eq!(index["assets"]["maps"]["status"], "extracted", "{index:#}");
        assert_eq!(index["maps"][1]["stand_ins"][0]["used"], 6, "{index:#}");

        let written = crate::load_pack(&out).unwrap();
        assert_eq!(written.maps.len(), 2);
        let a = &written.maps["hexz_00"];
        assert_eq!(a.name, "평원1");
        assert_eq!(a.rows, "0120\n3433\n6580\n");
        assert_eq!(a.image.as_deref(), Some("hexz_00"));
        let b = &written.maps["hexz_01"];
        assert_eq!(b.rows, "06\n06\n");
        assert_eq!(b.legend["6"], "castle");
        // The rules grid reads as the base pack's terrain.
        let grid = hero_core::map::BattleMap::parse(&a.rows, &a.legend, &written.terrain).unwrap();
        assert_eq!(
            grid.terrain_at(hero_core::geom::Pos::new(1, 0)),
            Some("forest")
        );
        assert_eq!(
            grid.terrain_at(hero_core::geom::Pos::new(1, 1)),
            Some("bridge")
        );
        assert_eq!(
            grid.terrain_at(hero_core::geom::Pos::new(2, 2)),
            Some("village")
        );
        // Nothing about the maps is wrong: map files, legends, pictures and their size.
        let issues = crate::validate::check(&out, &written).unwrap();
        let about_maps: Vec<_> = issues
            .iter()
            .filter(|i| {
                i.context.starts_with("map ")
                    || i.context.contains("maps/")
                    || i.msg.contains("gfx/maps/")
            })
            .collect();
        assert!(about_maps.is_empty(), "{about_maps:#?}");
        assert!(
            crate::info::render(&written).contains("Maps:        2 in 1 files (0 used by battles)")
        );
    }

    /// What changed from the original pack at `prev` to the one at `new`: battles, scenes,
    /// campaign nodes and officers added, removed or changed (for a battle, which of its fields),
    /// and the battle notes of `original-pack.json` that came or went. For comparing the
    /// conversion before and after a change (`golden_original_pack`).
    fn pack_diff(prev: &Path, new: &Path) -> String {
        use std::collections::{BTreeMap, BTreeSet};
        use std::fmt::Write as _;
        fn keys<T: PartialEq>(
            out: &mut String,
            what: &str,
            a: &BTreeMap<String, T>,
            b: &BTreeMap<String, T>,
            detail: impl Fn(&T, &T) -> String,
        ) {
            let gone: Vec<&String> = a.keys().filter(|k| !b.contains_key(*k)).collect();
            let came: Vec<&String> = b.keys().filter(|k| !a.contains_key(*k)).collect();
            let changed: Vec<String> = a
                .iter()
                .filter_map(|(k, x)| b.get(k).filter(|y| *y != x).map(|y| (k, x, y)))
                .map(|(k, x, y)| format!("{k}{}", detail(x, y)))
                .collect();
            let _ = writeln!(
                out,
                "{what}: {} removed, {} added, {} changed",
                gone.len(),
                came.len(),
                changed.len()
            );
            for k in gone {
                let _ = writeln!(out, "  - {k}");
            }
            for k in came {
                let _ = writeln!(out, "  + {k}");
            }
            for k in changed {
                let _ = writeln!(out, "  ~ {k}");
            }
        }
        let load = |dir: &Path| crate::load_pack(dir);
        let (a, b) = match (load(prev), load(new)) {
            (Ok(a), Ok(b)) => (a, b),
            (Err(e), _) | (_, Err(e)) => return format!("pack diff: a pack does not load: {e}\n"),
        };
        let mut out = String::from("Changes from the previous golden run's original pack:\n");
        keys(&mut out, "battles", &a.battles, &b.battles, |x, y| {
            let (x, y) = (
                serde_json::to_value(x).unwrap(),
                serde_json::to_value(y).unwrap(),
            );
            let fields: Vec<&String> = x
                .as_object()
                .into_iter()
                .chain(y.as_object())
                .flat_map(|o| o.keys())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .filter(|k| x.get(k.as_str()) != y.get(k.as_str()))
                .collect();
            format!(
                " ({})",
                fields
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        });
        keys(&mut out, "scenes", &a.scenes, &b.scenes, |_, _| {
            String::new()
        });
        let nodes = |p: &hero_core::pack::Pack| -> BTreeMap<String, String> {
            p.campaign
                .nodes
                .iter()
                .map(|n| (n.id().to_string(), format!("{n:?}")))
                .collect()
        };
        keys(
            &mut out,
            "campaign nodes",
            &nodes(&a),
            &nodes(&b),
            |_, _| String::new(),
        );
        keys(&mut out, "officers", &a.officers, &b.officers, |_, _| {
            String::new()
        });
        let notes = |dir: &Path| -> BTreeSet<String> {
            // (A pack without the index, such as a hand-made one, has no notes.)
            let json: serde_json::Value = std::fs::read(dir.join(pack::PACK_INDEX))
                .ok()
                .and_then(|b| serde_json::from_slice(&b).ok())
                .unwrap_or_default();
            json["battles"]
                .as_array()
                .into_iter()
                .flatten()
                .flat_map(|b| {
                    let id = b["id"].as_str().unwrap_or("?").to_string();
                    b["notes"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(move |n| n.as_str().map(|n| format!("{id}: {n}")))
                        .collect::<Vec<_>>()
                })
                .collect()
        };
        let (na, nb) = (notes(prev), notes(new));
        let _ = writeln!(
            out,
            "battle notes: {} gone, {} new",
            na.difference(&nb).count(),
            nb.difference(&na).count()
        );
        for n in na.difference(&nb) {
            let _ = writeln!(out, "  - {n}");
        }
        for n in nb.difference(&na) {
            let _ = writeln!(out, "  + {n}");
        }
        out
    }

    #[test]
    fn the_pack_diff_names_what_changed() {
        fn copy(from: &Path, to: &Path) {
            std::fs::create_dir_all(to).unwrap();
            for e in std::fs::read_dir(from).unwrap() {
                let e = e.unwrap();
                if e.file_type().unwrap().is_dir() {
                    copy(&e.path(), &to.join(e.file_name()));
                } else {
                    std::fs::copy(e.path(), to.join(e.file_name())).unwrap();
                }
            }
        }
        let tmp = Temp::new("pack-diff");
        let (a, b) = (tmp.0.join("a"), tmp.0.join("b"));
        copy(&crate::tests::fixture_dir(), &a);
        copy(&crate::tests::fixture_dir(), &b);
        let same = pack_diff(&a, &b);
        assert!(
            same.contains("battles: 0 removed, 0 added, 0 changed"),
            "{same}"
        );
        let file = b.join("battles/b01.toml");
        let text = std::fs::read_to_string(&file).unwrap();
        std::fs::write(&file, text.replace("turn_limit = 20", "turn_limit = 21")).unwrap();
        let changed = pack_diff(&a, &b);
        assert!(
            changed.contains("battles: 0 removed, 0 added, 1 changed"),
            "{changed}"
        );
        assert!(changed.contains("~ b01 (turn_limit)"), "{changed}");
    }

    /// The whole conversion on a real install (`EIKETSU_ORIGINAL_DIR`, see
    /// docs/ORIGINAL_DATA.md §6), on top of the repository's base pack.
    #[test]
    fn golden_original_pack() {
        let Some(dir) = std::env::var_os("EIKETSU_ORIGINAL_DIR") else {
            eprintln!("skipped: set EIKETSU_ORIGINAL_DIR to the game's data folder");
            return;
        };
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let work = root.join("target/golden-original-pack");
        // The last run's pack is kept next to it (`.prev`, at the same depth so its `extends`
        // still finds the base pack) and compared with this run's (`.diff.txt`).
        let prev = root.join("target/golden-original-pack.prev");
        let diff_file = root.join("target/golden-original-pack.diff.txt");
        let _ = std::fs::remove_file(&diff_file);
        if work.join("original").join(pack::PACK_INDEX).exists() {
            let _ = std::fs::remove_dir_all(&prev);
            std::fs::rename(&work, &prev).unwrap_or_else(|e| {
                panic!("cannot move {} to {}: {e}", work.display(), prev.display())
            });
        }
        let _ = std::fs::remove_dir_all(&work);
        let out = work.join("original");
        let base = root.join("data/base");
        assert_eq!(run_pack(Path::new(&dir), &out, Some(&base), None), Ok(true));
        let json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(out.join(pack::PACK_INDEX)).unwrap()).unwrap();
        if prev.join("original").join(pack::PACK_INDEX).exists() {
            let summary = pack_diff(&prev.join("original"), &out);
            std::fs::write(&diff_file, &summary).unwrap();
            eprintln!("{summary}(also in {})", diff_file.display());
        }
        // Every class, every terrain tile key and most officers of the base pack.
        let files: Vec<&str> = json["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f.as_str().unwrap())
            .collect();
        assert_eq!(
            files
                .iter()
                .filter(|f| f.starts_with("gfx/units/") && f.ends_with(".png"))
                .count(),
            // 19 classes, 5 officer icons (Liu Bei's three, Lü Bu's, Cao Cao's) and the
            // confusion icon, 3 sides.
            (19 + 5 + 1) * 3
        );
        let portraits = json["portraits"].as_array().unwrap().len();
        assert!(portraits >= 100, "{portraits} portraits");
        assert!(json["portraits"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["officer"] == "yu_jin" && p["bakdata"] == 62));
        // The officers with their own unit icons are the original's officers 0, 4 and 8
        // (MAIN.EXE picks the icons by those numbers).
        for (officer, bakdata) in [("liu_bei", 0), ("lu_bu", 4), ("cao_cao", 8)] {
            assert!(pack::OFFICER_ICONS.iter().any(|(id, _)| *id == officer));
            assert!(
                json["portraits"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|p| p["officer"] == officer && p["bakdata"] == bakdata),
                "{officer} is not BAKDATA {bakdata}"
            );
        }
        // All 58 battle maps (FORMATS.md §10.1); the only cell of a code without terrain is the
        // code 255 of map 32 (§10.4). Their pictures passed the size check of `run_pack`.
        let maps = json["maps"].as_array().unwrap();
        assert_eq!(maps.len(), 58);
        let stand_ins: Vec<(u64, u64, u64)> = maps
            .iter()
            .flat_map(|m| {
                let number = m["number"].as_u64().unwrap();
                m["stand_ins"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(move |s| {
                        (
                            number,
                            s["code"].as_u64().unwrap(),
                            s["used"].as_u64().unwrap(),
                        )
                    })
            })
            .collect();
        // Off the map in the original: cliff.
        assert_eq!(stand_ins, [(32, 255, 9)]);
        let pack = crate::load_pack(&out).unwrap();
        assert_eq!(pack.maps.len(), 58);
        // Map 0 is 56×32 chips, 28×16 cells.
        let first = &pack.maps["hexz_00"].rows;
        assert_eq!(first.lines().count(), 16);
        assert!(first.lines().all(|l| l.chars().count() == 28), "{first}");
        let tiles = std::fs::read_to_string(out.join("gfx/tiles/terrain.toml")).unwrap();
        for t in &pack.terrain {
            assert!(
                tiles.contains(&format!("[tiles.{}]", t.tile_key())),
                "{}",
                t.id
            );
        }

        // The terrain rules follow the player's MAIN.EXE and the maps use the closed gate.
        assert_eq!(
            json["assets"]["rules"]["status"], "extracted",
            "{:#?}",
            json["assets"]["rules"]
        );
        let manifest = std::fs::read_to_string(out.join("pack.toml")).unwrap();
        assert!(
            manifest.contains(
                "[rules]\nterrain = \"rules/terrain.toml\"\nclasses = \"rules/classes.toml\"\n\
                 strategies = \"rules/strategies.toml\"\ngame = \"rules/game.toml\"\n\
                 items = \"rules/items.toml\"\n"
            ),
            "{manifest}"
        );
        // The healing items heal the original's amounts (MAIN.EXE, FORMATS §10.4).
        let item = |id: &str| pack.item(id).unwrap().effects.clone();
        assert_eq!(item("bean"), [hero_core::data::Effect::Heal { power: 600 }]);
        assert_eq!(
            item("fine_wine"),
            [hero_core::data::Effect::Morale { amount: 40 }]
        );
        assert_eq!(
            item("tea"),
            [
                hero_core::data::Effect::Heal { power: 1800 },
                hero_core::data::Effect::Morale { amount: 50 }
            ]
        );
        // The original's strategy amounts go with its formulas.
        assert_eq!(
            pack.rules.strategy_formulas,
            hero_core::data::StrategyFormulas::Original
        );
        assert!(pack
            .terrain
            .iter()
            .any(|t| t.id.as_str() == hero_import::pack::CLOSED_GATE));
        // The original's battle frame: its map hole is where the pack says (checked when
        // converting), and the pack validates with it (the picture is the canvas size).
        assert_eq!(
            json["assets"]["ui"]["status"], "extracted",
            "{:#?}",
            json["assets"]["ui"]
        );
        let frame = pack.manifest.presentation.battle_frame.as_ref().unwrap();
        assert_eq!(frame.image, hero_import::pack::BATTLE_FRAME);
        assert!(out.join(format!("gfx/{}.png", frame.image)).is_file());
        // The original's songs stand in for the base pack's music (rendered by the CLI).
        assert_eq!(
            json["assets"]["music"]["status"], "extracted",
            "{:#?}",
            json["assets"]["music"]
        );
        for (key, _, _) in hero_import::pack::MUSIC_KEYS {
            assert!(out.join(format!("bgm/{key}.wav")).is_file(), "{key}");
        }
        let camp = pack.manifest.presentation.camp_frame.as_ref().unwrap();
        assert_eq!(camp.image, hero_import::pack::CAMP_FRAME);
        assert!(out.join(format!("gfx/{}.png", camp.image)).is_file());
        let maps = std::fs::read_to_string(out.join("maps/original.toml")).unwrap();
        assert!(
            maps.contains("\"a\" = \"closed_gate\""),
            "no map has a closed gate"
        );
        // The class rules follow the player's MAIN.EXE: every original class's sprite is
        // among the pack's classes, with an attack range the engine knows.
        for sprite in hero_import::pack::CLASS_SPRITES {
            let class = pack.classes.values().find(|c| c.sprite == sprite).unwrap();
            assert!(class.range.offsets().is_some(), "{}", class.id);
        }
        // The strategy rules too: every original strategy's reach is a shape the engine knows,
        // and the classes learn only strategies the pack has.
        for id in hero_import::pack::STRATEGY_IDS {
            let strategy = pack.strategy(id).unwrap();
            assert!(strategy.range.offsets().is_some(), "{id}");
        }
        for class in pack.classes.values() {
            for learn in &class.strategies {
                assert!(pack.strategy(&learn.id).is_some(), "{}", learn.id);
            }
        }

        // The base pack's one battle, Sishui Pass, is re-staged on its original map (verified
        // values: FORMATS §13.4): the campaign does not play it (D21), but it stays in the chain.
        // (Then the original's chapters, made from the original battles: chapters 2 to 4 have
        // forty-five, the two that are fought on two maps, Changban and Wagu Pass, counting for
        // two; chapter 4's last two load their maps from their setup, issue #95.)
        // A battle of the original's chapters: `c<file>_s<scene>_b<block>[_<leg>]`.
        let is_chapter = |id: &str| {
            let mut parts = id.split('_');
            let (file, scene) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
            file.len() == 2
                && file.starts_with('c')
                && scene.starts_with('s')
                && scene[1..].parse::<u32>().is_ok()
        };
        let battles = json["battles"].as_array().unwrap();
        // (A route variant counts with the battle it varies.)
        let variant = |b: &serde_json::Value| {
            b["notes"].as_array().is_some_and(|n| {
                n.iter().any(|n| {
                    n.as_str()
                        .is_some_and(|n| n.starts_with("route variant of"))
                })
            })
        };
        let (chapters, restaged): (Vec<_>, Vec<_>) = battles
            .iter()
            .filter(|b| !variant(b))
            .partition(|b| is_chapter(b["id"].as_str().unwrap()));
        assert_eq!(restaged.len(), 1, "{battles:#?}");
        let later = chapters
            .iter()
            .filter(|b| {
                let id = b["id"].as_str().unwrap();
                ["c2_s", "c3_s", "c4_s"].iter().any(|c| id.starts_with(c))
            })
            .count();
        assert_eq!(later, 11 + 21 + 13, "{battles:#?}");
        // The prologue and chapter 1 are the original's too: each original battle the base pack
        // follows (the pairing table: the same file, scene and map; 19 battles, Jieqiao and Xiapi
        // counting once) is a battle of its chapter on its map that the campaign plays.
        let fought: std::collections::BTreeSet<(usize, usize, u8)> =
            hero_import::battles::ORIGINAL_BATTLES
                .iter()
                .map(|p| (p.file, p.scene, p.map))
                .collect();
        assert_eq!(fought.len(), 19);
        for (file, scene, map) in fought {
            let prefix = format!("c{file}_s{scene}_b");
            let map_id = format!("hexz_{map:02}");
            let found: Vec<&str> = pack
                .battles
                .values()
                .filter(|b| b.id.starts_with(&prefix))
                .filter(|b| b.map.use_map.as_deref() == Some(map_id.as_str()))
                .map(|b| b.id.as_str())
                .collect();
            assert!(!found.is_empty(), "no battle {prefix}* on {map_id}");
            for id in found {
                assert!(
                    pack.campaign.node(&format!("{id}_camp")).is_some(),
                    "{id} is not in the campaign"
                );
            }
        }
        // The battle of the original's chapter `file`, scene `scene` on battle map `map` (the
        // first leg when it has two).
        let chapter_battle = |file: usize, scene: usize, map: u8| {
            let prefix = format!("c{file}_s{scene}_b");
            let map_id = format!("hexz_{map:02}");
            pack.battles
                .values()
                .filter(|b| b.id.starts_with(&prefix))
                .find(|b| b.map.use_map.as_deref() == Some(map_id.as_str()))
                .unwrap_or_else(|| panic!("no battle {prefix}* on {map_id}"))
        };
        assert_eq!(pack.battles["p1_sishui"].turn_limit, 30);
        for (file, scene, map, turns) in [
            (0, 0, 0, 30),
            (0, 0, 1, 30),
            (1, 0, 6, 40),
            (1, 3, 14, 45),
            (1, 4, 8, 50),
        ] {
            let b = chapter_battle(file, scene, map);
            assert_eq!(b.turn_limit, turns, "{}", b.id);
        }
        // Sishui (the re-staged base battle and the prologue's): Hua Xiong commands at the pass;
        // the treasures lie on the granary and treasury.
        for sishui in [&pack.battles["p1_sishui"], chapter_battle(0, 0, 0)] {
            let hua = sishui
                .units
                .iter()
                .find(|u| u.officer.as_deref() == Some("hua_xiong"))
                .unwrap();
            assert_eq!(
                (hua.pos.x, hua.pos.y, hua.commander),
                (3, 9, true),
                "{}",
                sishui.id
            );
            let map = &pack.maps["hexz_00"];
            let row = |y: i32| map.rows.lines().nth(y as usize).unwrap().to_string();
            let glyphs: Vec<char> = sishui
                .treasures
                .iter()
                .map(|t| row(t.pos.y).chars().nth(t.pos.x as usize).unwrap())
                .collect();
            let terrain_of = |g: char| map.legend[&g.to_string()].clone();
            let mut kinds: Vec<String> = glyphs.into_iter().map(terrain_of).collect();
            kinds.sort();
            assert_eq!(kinds, ["granary", "treasury"], "{}", sishui.id);
        }
        // Xuzhou II: Cao Cao's army waits off the map until Liu Bei reaches the east edge.
        let xuzhou2 = chapter_battle(1, 4, 8);
        let cao = xuzhou2
            .units
            .iter()
            .find(|u| u.officer.as_deref() == Some("cao_cao"))
            .unwrap();
        let group = cao.group.clone().expect("Cao Cao arrives later");
        assert!(xuzhou2.events.iter().any(|e| {
            e.actions
                .contains(&hero_core::battledef::EventAction::Spawn {
                    group: group.clone(),
                })
                && matches!(
                    e.trigger,
                    hero_core::battledef::Trigger::Reach { to: Some(_), .. }
                )
        }));
        // The bandit chiefs Liu Bei wins over at Mount Tai, Pengcheng and Xiaqiu (이명, 조하,
        // 동량) are the original's persons, added as officers since they join.
        for ((file, scene, map), person) in
            [((1, 2, 10), 375), ((1, 2, 12), 223), ((1, 2, 11), 228)]
        {
            let b = chapter_battle(file, scene, map);
            let id = hero_import::pack::added_officer_id(person);
            assert!(pack.officers.contains_key(id.as_str()), "{id}");
            assert!(
                b.units
                    .iter()
                    .any(|u| u.officer.as_deref() == Some(id.as_str())),
                "{}: {id}",
                b.id
            );
            assert!(!b.events.is_empty(), "{}", b.id);
        }

        // Mid-battle events. Xuzhou II plays in stages: Che Zhou falls (his troops retreat), Cao
        // Cao arrives, then the south-western village wins.
        use hero_core::battledef::{EventAction, Trigger};
        assert!(xuzhou2.events.iter().any(|e| e.stage == Some(2)));
        assert!(xuzhou2
            .events
            .iter()
            .flat_map(|e| &e.actions)
            .any(|a| matches!(a, EventAction::Retreat { .. })));
        assert!(xuzhou2.events.iter().any(|e| e.trigger
            == Trigger::Reach {
                who: Some("liu_bei".into()),
                pos: hero_core::geom::Pos::new(1, 16),
                radius: 0,
                to: None
            }));
        // Xiapi: turn 30 or Liu Bei at (12, 12) lowers the drawbridge; the middle cell becomes a
        // bridge (the chip the game checks), the others keep their terrain with new chips.
        let xiapi = chapter_battle(1, 3, 14);
        let bridges: Vec<_> = xiapi
            .events
            .iter()
            .flat_map(|e| &e.actions)
            .filter_map(|a| match a {
                EventAction::SetTerrain {
                    pos,
                    terrain,
                    image,
                } => Some((pos.x, pos.y, terrain.as_str(), image.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(bridges.len(), 12, "two triggers × six cells");
        assert!(bridges.contains(&(12, 11, "bridge", Some("hexz_14_12_11_2".into()))));
        assert!(bridges.contains(&(11, 11, "river", Some("hexz_14_11_11_2".into()))));
        assert!(out.join("gfx/maps/hexz_14_12_11_2.png").is_file());
        // Beihai: Taishi Ci's gate opens (a gate becomes plain).
        assert!(chapter_battle(1, 1, 7)
            .events
            .iter()
            .any(|e| e.actions.contains(&EventAction::SetTerrain {
                pos: hero_core::geom::Pos::new(2, 2),
                terrain: "plain".into(),
                image: Some("hexz_07_2_2_0".into()),
            })));
        // The dialogue comes from the player's copy.
        let drama = std::fs::read_to_string(out.join("dramas/original_battles.drama")).unwrap();
        assert!(drama.contains("\nguan_chun: "), "{drama}");
        let xuzhou2_scenes = format!("orig_{}_", xuzhou2.id);
        assert!(pack.scenes.keys().any(|k| k.starts_with(&xuzhou2_scenes)));
        // Xiapi's duels are duel scenes with the original's riders.
        assert!(
            drama.contains("@duel liu_bei wei_xu terrain\n")
                && drama.contains("@duel_act right flee\n"),
            "{drama}"
        );
        for key in ["left", "right", "guan_yu", "zhang_fei", "lu_bu"] {
            assert!(out.join(format!("gfx/duel/{key}.png")).is_file(), "{key}");
        }
        // A background for the terrain of every cell of the battles (the original picks it by
        // the terrain the fighters stand on).
        for battle in pack.battles.values() {
            let map = hero_core::map::BattleMap::parse(
                &battle.map.rows,
                &battle.map.legend,
                &pack.terrain,
            )
            .unwrap();
            // And the terrain events change cells to (a gate opened, a bridge lowered).
            let changed = battle
                .events
                .iter()
                .flat_map(|e| &e.actions)
                .filter_map(|a| match a {
                    hero_core::battledef::EventAction::SetTerrain { terrain, .. } => Some(terrain),
                    _ => None,
                });
            for t in map.terrain_ids.iter().chain(changed) {
                assert!(
                    out.join(format!("gfx/duel/terrain_{t}.png")).is_file(),
                    "{} {t}",
                    battle.id
                );
            }
        }
        // Chapter 2 (SNR2): its battles and its story continue the base campaign after Xuzhou,
        // and a wrong answer at Yuan Shao's hall ends the game. Xinye's siege (block 3), which
        // the original offers by answering Zhang Fei instead of Zhuge Liang, is a choice.
        let chapter: Vec<&str> = pack
            .battles
            .keys()
            .map(|k| k.as_str())
            .filter(|k| k.starts_with("c2_s"))
            .collect();
        assert_eq!(chapter.len(), 11, "{chapter:?}");
        assert!(pack.battles.contains_key("c2_s3_b3"));
        assert!(pack.battles["c2_s3_b7"].name.starts_with("장판파"));
        // Changban is fought on two maps (two battles, the second after the first's camp): the
        // people are three allied civilians the opening marches to a village; enemies hunt them
        // by name, and one of them arriving there wins the map (telling the outro that an event
        // did, so that its script of the battle won is left out).
        for (id, map, village, next) in [
            ("c2_s3_b7", "hexz_25", (32, 22), "c2_s3_b7_2_camp"),
            ("c2_s3_b7_2", "hexz_26", (0, 10), "c2_s3_story8"),
        ] {
            use hero_core::battledef::{AiMode, Side};
            let battle = &pack.battles[id];
            assert_eq!(battle.map.use_map.as_deref(), Some(map), "{id}");
            let people: Vec<_> = battle
                .units
                .iter()
                .filter(|u| u.side == Side::Ally && u.class.as_deref() == Some("civilian"))
                .collect();
            assert_eq!(people.len(), 3, "{id}");
            let village = hero_core::geom::Pos::new(village.0, village.1);
            assert!(
                people
                    .iter()
                    .all(|u| u.ai == AiMode::March && u.ai_pos == Some(village)),
                "{id}: {people:#?}"
            );
            assert!(
                battle
                    .units
                    .iter()
                    .any(|u| u.ai_target.as_deref() == Some("person_344")),
                "{id}: the enemy that hunts the people"
            );
            // Their tiles are no deploy tiles: the setup's twelve are (Liu Bei, Zhuge Liang
            // and ten more).
            assert_eq!(battle.deploy.slots.len(), 12, "{id}");
            assert!(people.iter().all(|u| !battle.deploy.slots.contains(&u.pos)));
            assert!(pack.scene(&format!("{id}_outro")).is_some(), "{id}");
            let flag = hero_import::battles::ended_flag(id);
            let arrivals = battle
                .events
                .iter()
                .filter(|e| {
                    matches!(&e.trigger, hero_core::battledef::Trigger::Reach { who: Some(w), pos, .. }
                        if w.starts_with("person_34") && *pos == village)
                })
                .collect::<Vec<_>>();
            assert_eq!(arrivals.len(), 3, "{id}");
            for e in arrivals {
                assert!(
                    e.actions.contains(&EventAction::SetFlag {
                        flag: flag.clone(),
                        value: 1
                    }) && e.actions.last() == Some(&EventAction::Victory),
                    "{id}: {e:?}"
                );
            }
            // The people really can walk there: with every other unit holding still, they
            // cross the map on the AI's marching and one arriving wins.
            let army = crate::simulate::army_for(&pack, id).unwrap();
            let mut state = hero_core::battle::BattleState::new(&pack, id, &army, 7).unwrap();
            state.begin(&pack);
            for _ in 0..60 {
                if state.outcome.is_some() {
                    break;
                }
                for u in state.units.iter_mut().filter(|u| u.class != "civilian") {
                    (u.ai, u.ai_target, u.ai_pos) = (AiMode::Hold, None, None);
                }
                state.run_ai_phase(&pack);
            }
            assert_eq!(
                state.outcome,
                Some(hero_core::battle::Outcome::Victory),
                "{id}: turn {}",
                state.turn
            );
            assert_eq!(state.flag(&flag), 1, "{id}");
            assert!(
                (4..=20).contains(&state.turn),
                "{id}: the people take {} turns",
                state.turn
            );
            // ... and the campaign goes on to the next map's camp (or the story after).
            let node = pack.campaign.node(&format!("{id}_battle")).unwrap();
            assert!(
                matches!(node, hero_core::campaign::Node::Battle { next: n, .. } if n == next),
                "{id}: {node:?}"
            );
        }
        // Gucheng is won by any unit's contact with the stranger (Zhang Fei), as the objective says.
        assert!(pack.battles["c2_s0_b9"].events.iter().any(|e| matches!(
            &e.trigger,
            hero_core::battledef::Trigger::Adjacent { a: None, .. }
        ) && e
            .actions
            .contains(&hero_core::battledef::EventAction::Victory)));
        // The choice between Xiangyang and Jiangxia is a route: Jiangxia goes straight to
        // Changban, Xiangyang fights Cai Mao first.
        let route = pack
            .campaign
            .node("c2_s3_story5_route1")
            .expect("the route branch");
        assert!(
            matches!(route, hero_core::campaign::Node::Branch { then, otherwise, .. }
                if then == "c2_s3_b7_camp" && otherwise == "c2_s3_b6_camp"),
            "{route:?}"
        );
        // The campaign is the original's from the prologue (D21): it starts with the prologue's
        // story, the base campaign's nodes are gone, and chapter 1 goes on to chapter 2.
        use hero_core::campaign::Node;
        assert!(
            pack.campaign.start.starts_with("c0_s0_"),
            "{}",
            pack.campaign.start
        );
        assert_eq!(
            pack.campaign.starting_officers,
            ["liu_bei", "guan_yu", "zhang_fei"]
        );
        assert!(pack.campaign.node("c1_battle_xuzhou2").is_none());
        assert!(pack
            .campaign
            .nodes
            .iter()
            .all(|n| is_chapter(n.id()) || n.id().starts_with("orig_")));
        let leads_to = |target: &str| {
            pack.campaign.nodes.iter().any(|n| match n {
                Node::Drama { next, .. } | Node::Camp { next, .. } => next == target,
                Node::Battle {
                    next, on_defeat, ..
                } => next == target || on_defeat.as_deref() == Some(target),
                Node::Branch {
                    then, otherwise, ..
                } => then == target || otherwise == target,
                Node::Ending { .. } => false,
            })
        };
        assert!(leads_to("c2_s0_story0"));
        // Officers the original brings in during a battle are not deployed from the army too,
        // and the gold of the victory is the battle's reward.
        let bowang = &pack.battles["c2_s3_b2"];
        assert!(bowang.deploy.forbidden.iter().any(|o| o == "guan_yu"));
        assert!(bowang.reward_gold > 0);
        // Chapters 3 and 4 (SNR3, SNR4) follow, to the original's endings.
        // (Route variants count with the battle they vary.)
        let is_variant = |id: &str| {
            pack.battles.contains_key(id)
                && id
                    .rsplit('_')
                    .next()
                    .is_some_and(|s| s.starts_with('f') && s[1..].parse::<u8>().is_ok())
        };
        let count = |c: &str| {
            pack.battles
                .keys()
                .filter(|k| k.starts_with(c) && !is_variant(k))
                .count()
        };
        // (Chapter 4 ends with two battles that load their maps from their setup, issue #95.)
        assert_eq!((count("c3_s"), count("c4_s")), (21, 13));
        // Fu is fought in two blocks (two routes): each its own battle.
        assert_ne!(
            pack.battles["c3_s2_b6"].events.len(),
            pack.battles["c3_s2_b10"].events.len()
        );
        // Maicheng: Guan Yu's troop without Liu Bei, lost when Guan Yu retreats, its triggers
        // from the block after the setup's.
        let maicheng = &pack.battles["c3_s4_b0"];
        assert!(maicheng.deploy.required.iter().any(|o| o == "guan_yu"));
        assert!(maicheng.deploy.forbidden.iter().any(|o| o == "liu_bei"));
        assert!(maicheng.defeat.iter().any(|c| matches!(
            c,
            hero_core::battledef::Condition::UnitRetreated { target } if target == "guan_yu"
        )));
        assert!(!maicheng.events.is_empty());
        // Its troop joins as the setup says, before the camp, and leaves when it is over.
        assert!(matches!(
            pack.campaign.node("c3_s4_b0_before"),
            Some(hero_core::campaign::Node::Drama { next, .. }) if next == "c3_s4_b0_camp"
        ));
        // Xuzhou II keeps the slots under units arriving later.
        assert_eq!(xuzhou2.deploy.max, 9);
        // The deploy of the re-staged base battle (the slot filter and the deploy limit run for
        // it too), as the conversion gives it now, so that a change to the filter is seen here;
        // every battle of the prologue and chapter 1 deploys at least one officer and no more
        // than its slots.
        let deploy = &pack.battles["p1_sishui"].deploy;
        assert_eq!((deploy.max, deploy.slots.len()), (3, 3));
        for battle in pack
            .battles
            .values()
            .filter(|b| b.id.starts_with("c0_s") || b.id.starts_with("c1_s"))
        {
            let deploy = &battle.deploy;
            assert!(
                deploy.max >= 1 && deploy.max as usize <= deploy.slots.len(),
                "{}: {} of {}",
                battle.id,
                deploy.max,
                deploy.slots.len()
            );
        }
        // Losing Yiling goes on to the original's ending 4; the last scene ends in one of three.
        assert!(matches!(
            pack.campaign.node("c3_s4_b6_battle"),
            Some(hero_core::campaign::Node::Battle { on_defeat: Some(d), .. }) if d == "c3_s4_b6_defeat"
        ));
        for n in 0..4 {
            assert!(
                pack.campaign.node(&format!("orig_ending_{n}")).is_some(),
                "{n}"
            );
        }
        assert!(pack.campaign.node("orig_c4_end").is_some());
        let story = std::fs::read_to_string(out.join(pack::CHAPTER_DRAMA_FILE)).unwrap();
        assert!(
            story.contains("@set orig_game_over = 1") && story.contains("yuan_shao: "),
            "{story}"
        );
        // Chapter 1's story joins its officers: Jian Yong and the others the base pack has, and
        // the original's own people (한영, 곽적, 번궁), whom the pack adds (officers.toml); the
        // brothers are scattered at its end.
        for id in ["jian_yong", "guan_chun", "geng_wu", "sun_qian", "mi_zhu"] {
            assert!(story.contains(&format!("@join {id}\n")), "{id}");
        }
        for name in ["한영", "곽적", "번궁"] {
            let officer = pack
                .officers
                .values()
                .find(|o| o.name == name)
                .unwrap_or_else(|| panic!("{name}"));
            assert!(officer.id.starts_with("orig_p"), "{name}: {}", officer.id);
            assert!(story.contains(&format!("@join {}\n", officer.id)), "{name}");
            assert!(out
                .join(format!("gfx/portraits/{}.png", officer.id))
                .is_file());
        }
        assert!(story.contains("@away guan_yu\n"), "{story}");
        assert_eq!(json["assets"]["officers"]["status"], "extracted");
        // Zhuge Liang is asked again until the right answer.
        assert!(story.contains("@goto ask_"), "{story}");
        // After Runan's battle Liu Pi asks to come along: the epilogue's choice.
        assert!(story.contains("@join liu_pi"), "{story}");
        // Whose plan to follow at Xinye (the siege or Bowang).
        assert!(story.contains("장비의 뜻을 따른다 -> "), "{story}");
        // Zhao Yun joins as heavy cavalry, seven levels up (after his return).
        assert!(
            story.contains("@join zhao_yun\n@class zhao_yun heavy_cavalry\n@level zhao_yun 7\n"),
            "{story}"
        );
        // Jiangling goes on in the next block: one battle (no story scene for that block).
        assert!(!story.contains("== c3_s0_story3\n"), "{story}");
        assert!(pack.battles["c3_s0_b2"].events.len() > 4);
        // Officers persuaded in a battle join after it; towns one walks between are a choice.
        assert!(
            story.contains("@if orig_join_jiang_wei == 0 -> army_0\n@join jiang_wei\n"),
            "{story}"
        );
        assert!(story.contains("에게 간다 -> "), "{story}");
        let scene = |id: &str| {
            let head = format!("== {id}\n");
            let start = story.find(&head).unwrap_or_else(|| panic!("{id}")) + head.len();
            story[start..].split("\n== ").next().unwrap().to_string()
        };
        // Jiangling's outro (and gold) is the second part's: only when the battle got there; won
        // before (Chen Jiao defeated), the event's own gold only.
        assert_eq!(pack.battles["c3_s0_b2"].reward_gold, 0);
        assert!(scene("c3_s0_b2_outro").starts_with("@if orig_f255 == 0 -> outro_"));
        assert!(scene("c3_s4_b0_before").contains("@join guan_yu\n"));
        assert!(scene("c3_s4_b0_defeat").contains("@away guan_yu\n"));
        // Chapter 4's detachment comes back as Xuchang's setup says.
        assert!(scene("c4_s1_b6_before").contains("@join zhao_yun\n"));
        // What a setup says before the sortie (ROADMAP M4-1): Maicheng's pictures and narration
        // and Xuchang 2's council, not the prompt to deploy the troops.
        let maicheng_before = scene("c3_s4_b0_before");
        for picture in ["@picture orig_23\n", "@picture orig_24\n", "@narr "] {
            assert!(maicheng_before.contains(picture), "{picture}");
        }
        let council = scene("c4_s1_b7_before");
        assert!(
            council.contains("huang_zhong: 주공, 꼭 저를 데려가"),
            "{council}"
        );
        assert!(!council.contains("부대를 편성"), "{council}");
        // The words as a battle begins (ROADMAP M4-1) play in its first turn, in order with the
        // duels and retreats the opening has: Sishui's challenge, a duel's loser leaving.
        let battle_story = std::fs::read_to_string(out.join(pack::DRAMA_FILE)).unwrap();
        let opening_text = |id: &str| {
            let head = format!("== {id}\n");
            let start = battle_story.find(&head).unwrap_or_else(|| panic!("{id}")) + head.len();
            battle_story[start..]
                .split("\n== ")
                .next()
                .unwrap()
                .to_string()
        };
        let first_turn = |id: &str, flag: Option<&str>| -> Vec<hero_core::battledef::EventAction> {
            pack.battles[id]
                .events
                .iter()
                .filter(|e| {
                    matches!(
                        e.trigger,
                        hero_core::battledef::Trigger::TurnStart {
                            turn: 1,
                            side: hero_core::battledef::Side::Player
                        }
                    ) && e.when.first().map(|c| c.flag.as_str()) == flag
                })
                .flat_map(|e| e.actions.clone())
                .collect()
        };
        use hero_core::battledef::EventAction::{Drama, Retreat};
        assert_eq!(
            first_turn("c0_s0_b5", None),
            [Drama {
                scene: "orig_c0_s0_b5_2".into()
            }]
        );
        assert!(opening_text("orig_c0_s0_b5_2").starts_with("hua_xiong: "));
        assert_eq!(
            first_turn("c1_s0_b11", None),
            [
                Drama {
                    scene: "orig_c1_s0_b11_2".into()
                },
                Retreat {
                    target: "yan_gang".into()
                },
                Drama {
                    scene: "orig_c1_s0_b11_2_2".into()
                }
            ]
        );
        assert!(opening_text("orig_c1_s0_b11_2").contains("@duel qu_yi yan_gang terrain\n"));
        // Jincang's and Chang'an's openings have lines of their own for each side of flag 38
        // (Pang Tong's death), told where the original tells them: the defender's words, then
        // Pang Tong's or Zhao Yun's by the route, then the one who closes it (Jiang Wei, Xu Shu).
        for id in ["c4_s1_b2", "c4_s1_b3"] {
            let opening: Vec<(Option<String>, Vec<String>)> = pack.battles[id]
                .events
                .iter()
                .filter(|e| {
                    matches!(
                        e.trigger,
                        hero_core::battledef::Trigger::TurnStart {
                            turn: 1,
                            side: hero_core::battledef::Side::Player
                        }
                    )
                })
                .map(|e| {
                    (
                        e.when.first().map(|c| format!("{} {:?}", c.flag, c.cmp)),
                        e.actions
                            .iter()
                            .filter_map(|a| match a {
                                Drama { scene } => Some(scene.clone()),
                                _ => None,
                            })
                            .collect(),
                    )
                })
                .collect();
            let flags: Vec<Option<&str>> = opening.iter().map(|(w, _)| w.as_deref()).collect();
            assert_eq!(flags.len(), 4, "{id}: {opening:?}");
            assert!(
                flags[0].is_none()
                    && flags[1].is_some_and(|f| f.starts_with("orig_f38"))
                    && flags[2].is_some_and(|f| f.starts_with("orig_f38"))
                    && flags[3].is_none(),
                "{id}: {opening:?}"
            );
            let text = |scene: &str| opening_text(scene);
            // (Jiang Wei closes Jincang's, Xu Shu Chang'an's.)
            let last = if id == "c4_s1_b2" {
                "jiang_wei: "
            } else {
                "xu_shu: "
            };
            assert!(text(&opening[3].1[0]).starts_with(last), "{id}");
            assert!(
                [&opening[1], &opening[2]]
                    .iter()
                    .any(|(_, scenes)| text(&scenes[0]).starts_with("pang_tong: ")),
                "{id}"
            );
        }
        // Levels the story gives officers who are not in the army yet wait for their join (D24):
        // the council with Wu (flag 136 set) raises its generals by eleven levels, Gan Ning is in
        // no army, and joining later gives him the levels.
        let mut state = hero_core::campaign::CampaignState::new_game(&pack);
        state.flags.insert("orig_f136".into(), 1);
        for story in ["c4_s0_story4"] {
            let mut runner = hero_core::drama::DramaRunner::new(&pack, story).unwrap();
            loop {
                match runner.next(&pack, &mut state).unwrap() {
                    hero_core::drama::Step::End => break,
                    hero_core::drama::Step::Choice(_) => runner.choose(&pack, 0).unwrap(),
                    _ => {}
                }
            }
        }
        assert_eq!(state.pending_growth["gan_ning"].levels, 11);
        let level = pack.officer("gan_ning").unwrap().level;
        state.join(&pack, "gan_ning").unwrap();
        assert_eq!(state.officer("gan_ning").unwrap().level, level + 11);
        assert!(!state.pending_growth.contains_key("gan_ning"));
        // The original's event pictures, shown over the story (chapter 2 opens with one).
        for n in 3..=33 {
            assert!(
                out.join(format!("gfx/pictures/orig_{n:02}.png")).is_file(),
                "{n}"
            );
        }
        assert!(scene("c2_s0_story0").contains("@picture orig_12\n"));
        // Saying yes to join ends the talk (no refusal after it).
        assert!(story.contains("@goto rend_"), "{story}");
        // A battle's events test the story's flags when it is fought (Xuchang's turn 12).
        assert!(pack.battles["c4_s1_b6"]
            .events
            .iter()
            .any(|e| e.when.iter().any(|c| c.flag == "orig_f89")));
        // An officer the story has not brought in yet fights as an ally, not deployed.
        let runan = &pack.battles["c2_s0_b16"];
        assert!(!runan.deploy.required.iter().any(|o| o == "liu_pi"));
        assert!(runan
            .units
            .iter()
            .any(|u| u.officer.as_deref() == Some("liu_pi")
                && u.side == hero_core::battledef::Side::Ally));
        // The brothers start in the army: the prologue deploys them on the original's tiles,
        // although the story takes them away and brings them back later (chapters 1 and 2).
        for id in ["c0_s0_b5", "c0_s0_b7"] {
            let b = &pack.battles[id];
            assert_eq!((b.deploy.max, b.deploy.slots.len()), (3, 3), "{id}");
            for o in ["guan_yu", "zhang_fei"] {
                assert!(b.deploy.required.iter().any(|r| r == o), "{id}: {o}");
                assert!(
                    !b.units.iter().any(|u| u.officer.as_deref() == Some(o)),
                    "{id}: {o}"
                );
            }
        }
        // Membership follows the ways to a battle (issue #81): on the Xindu road the town's
        // garrison (Han Ying, Guo Ji) has not joined yet, which happens only on the Guangchuan
        // road. They hold their tiles as allies, and Liu Bei takes the first slot, his own.
        let xindu = &pack.battles["c1_s0_b6"];
        assert!(xindu.deploy.required.is_empty(), "{:?}", xindu.deploy);
        for (o, x, y) in [("orig_p245", 3, 0), ("orig_p244", 0, 0)] {
            assert!(
                xindu.units.iter().any(|u| u.officer.as_deref() == Some(o)
                    && u.side == hero_core::battledef::Side::Ally
                    && u.pos == hero_core::geom::Pos::new(x, y)),
                "{o}"
            );
        }
        assert_eq!(
            xindu.deploy.slots.first(),
            Some(&hero_core::geom::Pos::new(21, 7))
        );
        // The notes say who became an ally that way (issue #79): the garrison at Xindu. (The
        // prologue's two battles take their setup from the base pack's, so they never get the
        // note; the check guards against that changing with the brothers listed.)
        let notes = |id: &str| -> Vec<String> {
            json["battles"]
                .as_array()
                .unwrap()
                .iter()
                .find(|b| b["id"] == id)
                .and_then(|b| b["notes"].as_array())
                .map(|n| {
                    n.iter()
                        .filter_map(|n| n.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default()
        };
        let allies = |id: &str| {
            notes(id)
                .into_iter()
                .find(|n| n.contains("not in the army at this battle"))
        };
        let xindu_allies = allies("c1_s0_b6").unwrap_or_default();
        assert!(
            xindu_allies.contains("orig_p244") && xindu_allies.contains("orig_p245"),
            "{xindu_allies}"
        );
        for id in ["c0_s0_b5", "c0_s0_b7"] {
            assert!(
                allies(id).is_none_or(|n| !n.contains("guan_yu") && !n.contains("zhang_fei")),
                "{id}: {:?}",
                notes(id)
            );
        }
        // Route variants (ROADMAP M2): a battle whose setup or rosters the original picks by a
        // flag of the route is one battle per reading, and its camp branches on the flag.
        let node = |id: &str| pack.campaign.node(id).cloned().unwrap();
        let branch = |id: &str| match node(id) {
            hero_core::campaign::Node::Branch {
                flag,
                then,
                otherwise,
                ..
            } => (flag, then, otherwise),
            other => panic!("{id}: {other:?}"),
        };
        // Jieqiao: another enemy army on the Julu road (flag 133).
        assert_eq!(
            branch("c1_s0_b15_which"),
            (
                "orig_f133".to_string(),
                "c1_s0_b15_f133_camp".to_string(),
                "c1_s0_b15_camp".to_string()
            )
        );
        let enemies = |id: &str| -> std::collections::BTreeSet<String> {
            pack.battles[id]
                .units
                .iter()
                .filter(|u| u.side == hero_core::battledef::Side::Enemy)
                .filter_map(|u| u.officer.clone())
                .collect()
        };
        assert_ne!(enemies("c1_s0_b15"), enemies("c1_s0_b15_f133"));
        // Chencang and Chang'an: Pang Tong must not fall while he lives, Zhao Yun once he died
        // at Luofengpo (flag 38).
        for id in ["c4_s1_b2", "c4_s1_b3"] {
            let lost_with = |id: &str| -> Vec<String> {
                pack.battles[id]
                    .defeat
                    .iter()
                    .filter_map(|c| match c {
                        hero_core::battledef::Condition::UnitRetreated { target } => {
                            Some(target.clone())
                        }
                        _ => None,
                    })
                    .collect()
            };
            assert_eq!(lost_with(id), ["pang_tong"], "{id}");
            assert_eq!(lost_with(&format!("{id}_f38")), ["zhao_yun"], "{id}");
            assert_eq!(branch(&format!("{id}_which")).0, "orig_f38");
        }
        // Xuchang 2's opening (a phase of `run` records before the first watched one) plays when the
        // battle begins and sets flag 218, which brings Zhang Liao into the next battle's
        // enemy army (its variant).
        let xuchang_2 = &pack.battles["c4_s1_b7"];
        assert!(xuchang_2.events.iter().any(|e| matches!(
            e.trigger,
            hero_core::battledef::Trigger::TurnStart { turn: 1, .. }
        ) && e.actions.iter().any(|a| matches!(
            a,
            hero_core::battledef::EventAction::SetFlag { flag, value: 1 } if flag == "orig_f218"
        ))));
        assert_eq!(branch("c4_s2_b2_which").0, "orig_f218");
        assert!(enemies("c4_s2_b2_f218").contains("zhang_liao"));
        assert!(!enemies("c4_s2_b2").contains("zhang_liao"));
        // Allies arriving during a battle (ROADMAP M3): the original keeps their setup slot back
        // until `join_battle` (MAIN.EXE places them on it). Bowang's ambushes are the army's
        // officers: player units that arrive; Xuchang 2's Huang Zhong and Yan Yan, whom its
        // setup takes out of the army, arrive as allies on their tiles when a unit stands on the
        // wall's cell (issue #86: that cell's script was taken for a treasure).
        let unit = |b: &str, o: &str| {
            pack.battles[b]
                .units
                .iter()
                .find(|u| u.officer.as_deref() == Some(o))
                .cloned()
                .unwrap_or_else(|| panic!("{b}: {o}"))
        };
        for o in ["zhang_fei", "guan_yu"] {
            let u = unit("c2_s3_b2", o);
            assert_eq!(u.side, hero_core::battledef::Side::Player, "{o}");
            assert!(u.group.is_some(), "{o}");
        }
        for (o, x, y) in [("huang_zhong", 8, 0), ("yan_yan", 8, 1)] {
            let u = unit("c4_s1_b7", o);
            assert_eq!(
                (u.side, u.pos, u.group.as_deref()),
                (
                    hero_core::battledef::Side::Ally,
                    hero_core::geom::Pos::new(x, y),
                    Some("original_6")
                ),
                "{o}"
            );
        }
        let xuchang2 = &pack.battles["c4_s1_b7"];
        assert!(xuchang2.events.iter().any(|e| matches!(
            e.trigger,
            hero_core::battledef::Trigger::Reach { who: None, .. }
        ) && e.actions.contains(
            &hero_core::battledef::EventAction::Spawn {
                group: "original_6".into()
            }
        )));
        // Zhang Liao's `set_allegiance` after his talk with Guan Yu: he joins the army after it.
        assert!(scene("c4_s1_b7_outro").contains("@join zhang_liao"));
        // Issue #86: capturing the four camps (flags 34-37) wins c3_s1_b5; Xinye's granary.
        let sets =
            |b: &str, f: &str| {
                pack.battles[b].events.iter().any(|e| {
                    matches!(e.trigger, hero_core::battledef::Trigger::Reach { who: None, .. })
                    && e.actions.iter().any(|a| matches!(
                        a,
                        hero_core::battledef::EventAction::SetFlag { flag, value: 1 } if flag == f
                    ))
                })
            };
        for f in ["orig_f34", "orig_f35", "orig_f36", "orig_f37"] {
            assert!(sets("c3_s1_b5", f), "{f}");
        }
        assert!(pack.battles["c4_s0_b7"].events.iter().any(|e| matches!(
            e.trigger,
            hero_core::battledef::Trigger::Reach { who: None, .. }
        ) && e
            .actions
            .contains(&hero_core::battledef::EventAction::Victory)));
        // Sishui: the guests the talks before it bring (flags 0 and 1, which the story always
        // sets) fight at their tiles beside the army: no variant without them.
        assert!(!pack.battles.contains_key("c0_s0_b5_f0"));
        let sishui = &pack.battles["c0_s0_b5"];
        for o in ["gongsun_zan", "tao_qian"] {
            assert!(!sishui.deploy.required.iter().any(|r| r == o), "{o}");
            assert!(
                sishui.units.iter().any(|u| u.officer.as_deref() == Some(o)
                    && u.side == hero_core::battledef::Side::Ally),
                "{o}"
            );
        }
        // Issue #84: an allied officer who never joins the army is an ally at their tile, not a
        // required officer the camp would leave out.
        let required: Vec<(&String, &String)> = pack
            .battles
            .iter()
            .flat_map(|(id, b)| b.deploy.required.iter().map(move |o| (id, o)))
            .filter(|(_, o)| {
                [
                    "xun_yu",
                    "cao_ren",
                    "guo_jia",
                    "kong_rong",
                    "gongsun_zan",
                    "tao_qian",
                ]
                .contains(&o.as_str())
            })
            .collect();
        assert!(required.is_empty(), "{required:?}");
        // No officer of the original's data is in the army on some ways to a battle only.
        assert!(!json["battles"]
            .as_array()
            .unwrap()
            .iter()
            .any(|b| b["notes"].as_array().is_some_and(|n| n
                .iter()
                .any(|n| n.as_str().is_some_and(|n| n.contains("some ways"))))));
        let json_battles = json["battles"].as_array().unwrap();
        let events: u64 = json_battles
            .iter()
            .map(|b| b["events"].as_u64().unwrap())
            .sum();
        assert!(events >= 60, "{events} events");
    }
}
