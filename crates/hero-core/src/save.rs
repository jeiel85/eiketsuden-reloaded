//! Save games: a versioned JSON document holding the campaign and, optionally, a battle
//! in progress and a drama scene played half-way. Storage (files natively, localStorage on the
//! web) is the frontend's job.

use crate::battle::BattleState;
use crate::campaign::CampaignState;
use crate::drama::{DramaRunner, Step};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Newest save layout this game reads. Bump when the save layout changes incompatibly; add a
/// migration in [`SaveGame::from_json`].
pub const SAVE_VERSION: u32 = OPTIONS_SAVE_VERSION;

/// Layout of a save without a half-played scene or queued growth. Such a save is still written
/// with this version (see [`SaveGame::stamp_version`]), so a game older than [`SAVE_VERSION`]
/// keeps loading it.
pub const PLAIN_SAVE_VERSION: u32 = 1;

/// Layout of a save with a half-played scene ([`SceneResume`]) or battle scenes still queued.
pub const SCENE_SAVE_VERSION: u32 = 2;

/// Layout of a save whose campaign holds growth of officers not in the army yet
/// ([`CampaignState::pending_growth`]).
pub const GROWTH_SAVE_VERSION: u32 = 3;

// A save that needs a newer layout must carry a version that a game older than this one refuses.
const _: () = assert!(SCENE_SAVE_VERSION > PLAIN_SAVE_VERSION);
/// Layout of a save whose campaign was started with choices of the new game (DECISIONS D25:
/// difficulty, free editing, extended rules; [`CampaignState::off_original`]).
pub const OPTIONS_SAVE_VERSION: u32 = 4;

const _: () = assert!(GROWTH_SAVE_VERSION > SCENE_SAVE_VERSION);
const _: () = assert!(OPTIONS_SAVE_VERSION > GROWTH_SAVE_VERSION);

/// Field of the save JSON holding [`version_needs`] of its version (written by
/// [`SaveGame::to_json`], read only when the save is too new, [`SaveError::TooNew`]).
pub const NEEDS_FIELD: &str = "needs";

/// What a save of layout `version` holds that a game predating that layout cannot play, in
/// words for the load screen. The save carries it ([`NEEDS_FIELD`]) because only a game that
/// knows the layout knows the reason: a game too old to read the save shows the text it
/// finds there. Keep it short: the load screen shows at most 30 characters of it (hero-game
/// `saves::MAX_NEEDS_CHARS`) inside a message that should stay on one line of its detail
/// panel (two at most). `None` for the plain layout. Every layout above
/// [`PLAIN_SAVE_VERSION`] needs an entry (a test checks it), so a new layout comes with its
/// reason.
pub fn version_needs(version: u32) -> Option<&'static str> {
    match version {
        SCENE_SAVE_VERSION => Some("장면 도중 저장"),
        GROWTH_SAVE_VERSION => Some("합류 전 무장의 성장"),
        OPTIONS_SAVE_VERSION => Some("새 게임 선택 기능(난이도·조정·확장)"),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SaveGame {
    pub version: u32,
    /// Pack the save belongs to (`pack.toml` id and version).
    pub pack_id: String,
    pub pack_version: String,
    /// Human readable summary for the load screen, e.g. `제2장 · 광종 전투 준비`.
    pub label: String,
    /// Unix seconds when saved, supplied by the frontend (0 when unknown).
    #[serde(default)]
    pub saved_at: u64,
    pub campaign: CampaignState,
    /// Present for a mid-battle save.
    #[serde(default)]
    pub battle: Option<BattleState>,
    /// Present for a save made in the middle of a drama scene (a quick save): where to go on.
    #[serde(default)]
    pub scene: Option<SceneResume>,
    /// Battle scenes that were queued but had not started when a mid-battle quick save was made.
    /// The battle state has already moved past them, so they are played after loading.
    #[serde(default)]
    pub pending_scenes: Vec<String>,
}

/// How a drama scene that is resumed half-way ends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SceneKind {
    /// The scene of a campaign `Drama` node.
    Node,
    /// The scene of a campaign `Ending` node.
    Ending { title: String },
    /// A scene shown over a battle (intro, outro, event).
    Overlay,
}

/// A drama scene played half-way: the runner's position plus what the frontend was showing.
///
/// The campaign state that goes with it is the save's own `campaign`, which already holds the
/// side effects (`@set`, `@join`, `@gold` ...) of every step up to the runner's position. So the
/// runner is **not** replayed from the start; only the *look* of the stage is rebuilt, from
/// [`SceneResume::stage`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SceneResume {
    pub kind: SceneKind,
    /// The cursor into the scene (its position is the step after the one on screen).
    pub runner: DramaRunner,
    /// Steps that changed the stage so far (backgrounds, portraits, pictures, fades, duel
    /// moves), oldest first. Presenting them again instantly rebuilds the stage.
    #[serde(default)]
    pub stage: Vec<Step>,
    /// The step that was on screen: a line, narration, title card or banner. `None` between
    /// steps and while waiting or fading.
    #[serde(default)]
    pub shown: Option<Step>,
    /// The message kept on screen under a choice.
    #[serde(default)]
    pub last_text: Option<Step>,
    /// Texts of the choice on screen; the runner holds its labels.
    #[serde(default)]
    pub choice: Option<Vec<String>>,
    /// The music playing (`bgm/<key>`), `None` for silence.
    #[serde(default)]
    pub bgm: Option<String>,
    /// Terrain id under each officer on the field, for duels over the terrain (battle scenes).
    #[serde(default)]
    pub terrain: BTreeMap<String, String>,
    /// Recent lines for the backlog: speaker and text, oldest first.
    #[serde(default)]
    pub backlog: Vec<(Option<String>, String)>,
    /// [`crate::script::Scene::fingerprint`] of the scene when it was saved: with it the record
    /// plays on in another version of the pack whose scene is the same. Saves made before it
    /// existed have none and need the same pack version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
    /// The timed step that was playing ([`Playing`]): loading goes on with it instead of
    /// starting after it. Saves without it (and games that do not know it) start after it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub playing: Option<Playing>,
}

/// A timed step of a scene in progress when it was quick saved (BACKLOG: 순간 저장의 연출 남은
/// 시간). Its effect on the stage is already in [`SceneResume::stage`]; this is only its time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Playing {
    /// `@wait`: milliseconds left.
    Wait { ms: u32 },
    /// `@fade out` / `@fade in`: how dark the screen was, in thousandths (0 clear, 1000 black);
    /// the fade goes on from there to the end the stage records.
    Fade { permille: u16 },
    /// `@duel_act`: the move (the last of the stage's steps) is played again from its start.
    DuelAct,
}

impl SceneResume {
    /// Whether this record can be played against the scene in `pack` (a save made with another
    /// version of the pack may point past the end of a rewritten scene or name a scene that is
    /// gone). Checked before loading so a stale record falls back to the start of the node
    /// instead of misbehaving.
    pub fn fits(&self, pack: &crate::pack::Pack) -> bool {
        let Some(scene) = pack.scene(&self.runner.scene) else {
            return false;
        };
        // `pc` may equal the length (the scene is about to end).
        self.runner.pc <= scene.cmds.len()
            && self.runner.pending_choice.is_some() == self.choice.is_some()
    }
}

/// Why the scene record of a save cannot be played against the pack that is loaded
/// ([`SaveGame::check_resume`]).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ResumeError {
    #[error("saved with pack version {saved}, but pack version {current} is loaded")]
    PackVersion { saved: String, current: String },
    #[error("the saved scene `{0}` is missing or shorter in this pack")]
    SceneChanged(String),
    #[error("the saved scene does not belong to campaign node `{0}`")]
    WrongNode(String),
    #[error("a scene shown over a battle was saved without the battle")]
    NoBattle,
    #[error("a scene of the campaign was saved together with a battle")]
    UnexpectedBattle,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SaveError {
    #[error("save data is corrupt: {0}")]
    Corrupt(String),
    #[error("save version {found} is newer than this game supports ({supported})")]
    TooNew {
        found: u32,
        supported: u32,
        /// What the save holds that this game cannot play, as the newer game wrote it
        /// ([`NEEDS_FIELD`]); `None` from games that predate the field.
        needs: Option<String>,
    },
    #[error("save belongs to pack `{found}`, not `{expected}`")]
    WrongPack { found: String, expected: String },
}

impl SaveGame {
    /// Set `version` to the oldest layout that can hold this save: a game that predates
    /// [`SceneResume`] would restart a half-played scene with its side effects already applied
    /// (gold and items twice), one that predates [`CampaignState::pending_growth`] would
    /// ignore the queued levels and drop them when it saves again, and one that predates the
    /// new game's choices (difficulty, free editing, extended rules) would play such a campaign
    /// as the pack as it is, so such a save must be refused by it (`TooNew`) rather than
    /// loaded. Every other save stays loadable by older games.
    pub fn stamp_version(&mut self) {
        self.version = if self.campaign.off_original() {
            OPTIONS_SAVE_VERSION
        } else if !self.campaign.pending_growth.is_empty() {
            GROWTH_SAVE_VERSION
        } else if self.scene.is_some() || !self.pending_scenes.is_empty() {
            SCENE_SAVE_VERSION
        } else {
            PLAIN_SAVE_VERSION
        };
    }

    /// Input: the pack that is loaded. Output: `Ok` when the save can be loaded, else why its
    /// scene record cannot be played. A save without a scene record is always fine.
    ///
    /// Why loading is refused rather than falling back to the start of the node: the saved
    /// campaign already holds the side effects (`@gold`, `@item`, `@set` ...) of the steps
    /// before the saved position, so playing the node's scene again from its first line would
    /// apply them a second time. Nor can the position be trusted against a changed scene: the
    /// same `pc` may name another command. So a record is played only against the very pack
    /// version it was made with (a changed pack is expected to carry a new version) or a pack
    /// whose scene has the fingerprint it saved ([`SceneResume::fingerprint`]), and only when
    /// it still fits that pack's scene and campaign node.
    pub fn check_resume(&self, pack: &crate::pack::Pack) -> Result<(), ResumeError> {
        use crate::campaign::Node;
        let Some(scene) = &self.scene else {
            return Ok(());
        };
        // Another pack version is fine for the very same scene: its commands and labels, which
        // the saved position counts in, are unchanged (BACKLOG, ROADMAP M7-4).
        let same_scene = || {
            scene.fingerprint.as_deref().is_some_and(|f| {
                pack.scene(&scene.runner.scene)
                    .is_some_and(|s| s.fingerprint() == f)
            })
        };
        if self.pack_version != pack.manifest.version && !same_scene() {
            return Err(ResumeError::PackVersion {
                saved: self.pack_version.clone(),
                current: pack.manifest.version.clone(),
            });
        }
        if !scene.fits(pack) {
            return Err(ResumeError::SceneChanged(scene.runner.scene.clone()));
        }
        let same = |id: &str| id == scene.runner.scene;
        match (&scene.kind, pack.campaign.node(&self.campaign.node)) {
            (SceneKind::Overlay, _) if self.battle.is_some() => Ok(()),
            (SceneKind::Overlay, _) => Err(ResumeError::NoBattle),
            (_, _) if self.battle.is_some() => Err(ResumeError::UnexpectedBattle),
            (SceneKind::Node, Some(Node::Drama { scene: id, .. })) if same(id) => Ok(()),
            (
                SceneKind::Ending { .. },
                Some(Node::Ending {
                    scene: Some(id), ..
                }),
            ) if same(id) => Ok(()),
            _ => Err(ResumeError::WrongNode(self.campaign.node.clone())),
        }
    }

    /// The save as JSON, with the reason its layout needs ([`version_needs`]) for games too
    /// old to read it.
    pub fn to_json(&self) -> String {
        let mut v = serde_json::to_value(self).expect("save game serialization cannot fail");
        if let (Some(needs), Some(map)) = (version_needs(self.version), v.as_object_mut()) {
            map.insert(NEEDS_FIELD.into(), needs.into());
        }
        v.to_string()
    }

    /// Parse and check the version. `expected_pack` guards against loading a save of another pack.
    pub fn from_json(src: &str, expected_pack: &str) -> Result<SaveGame, SaveError> {
        let v: serde_json::Value =
            serde_json::from_str(src).map_err(|e| SaveError::Corrupt(e.to_string()))?;
        let found = v.get("version").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
        if found > SAVE_VERSION {
            return Err(SaveError::TooNew {
                found,
                supported: SAVE_VERSION,
                needs: v
                    .get(NEEDS_FIELD)
                    .and_then(|x| x.as_str())
                    .map(str::to_string),
            });
        }
        let save: SaveGame =
            serde_json::from_value(v).map_err(|e| SaveError::Corrupt(e.to_string()))?;
        if save.pack_id != expected_pack {
            return Err(SaveError::WrongPack {
                found: save.pack_id,
                expected: expected_pack.to_string(),
            });
        }
        Ok(save)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script::Slot;

    fn campaign() -> CampaignState {
        CampaignState {
            node: "n".into(),
            roster: Vec::new(),
            inventory: BTreeMap::new(),
            gold: 5,
            flags: BTreeMap::new(),
            deployed: Vec::new(),
            battles_won: Vec::new(),
            play_seconds: 1,
            pending_growth: BTreeMap::new(),
            difficulty: Default::default(),
            free_edit: false,
            extended_rules: false,
        }
    }

    fn save() -> SaveGame {
        SaveGame {
            version: PLAIN_SAVE_VERSION,
            pack_id: "base".into(),
            pack_version: "0.1.0".into(),
            label: "x".into(),
            saved_at: 1,
            campaign: campaign(),
            battle: None,
            scene: None,
            pending_scenes: Vec::new(),
        }
    }

    fn resume() -> SceneResume {
        SceneResume {
            kind: SceneKind::Ending {
                title: "끝".into()
            },
            runner: DramaRunner {
                scene: "s".into(),
                pc: 7,
                pending_choice: Some(vec!["a".into(), "b".into()]),
                finished: false,
            },
            stage: vec![
                Step::Background(Some("hall".into())),
                Step::Show {
                    portrait: "liu_bei".into(),
                    slot: Slot::Left,
                },
                Step::FadeOut,
            ],
            shown: None,
            last_text: Some(Step::Narration("문이 열렸다.".into())),
            choice: Some(vec!["들어간다".into(), "물러선다".into()]),
            bgm: Some("camp".into()),
            terrain: BTreeMap::from([("liu_bei".to_string(), "plain".to_string())]),
            backlog: vec![(Some("유비".into()), "가자.".into()), (None, "…".into())],
            fingerprint: None,
            playing: None,
        }
    }

    #[test]
    fn a_scene_resume_round_trips() {
        let mut s = save();
        s.scene = Some(resume());
        s.pending_scenes = vec!["p1_outro".into()];
        s.stamp_version();
        let back = SaveGame::from_json(&s.to_json(), "base").unwrap();
        assert_eq!(back, s);
        assert_eq!(back.version, SCENE_SAVE_VERSION);
    }

    /// The timed step that was playing goes into the record and back; a record without it
    /// (an earlier game's) reads as none.
    #[test]
    fn the_playing_step_round_trips() {
        for playing in [
            Playing::Wait { ms: 457 },
            Playing::Fade { permille: 250 },
            Playing::DuelAct,
        ] {
            let mut s = save();
            let mut r = resume();
            r.playing = Some(playing);
            s.scene = Some(r);
            s.stamp_version();
            let back = SaveGame::from_json(&s.to_json(), "base").unwrap();
            assert_eq!(back.scene.unwrap().playing, Some(playing));
        }
        let mut s = save();
        s.scene = Some(resume());
        let json = s.to_json();
        assert!(!json.contains("playing"), "{json}");
        let back = SaveGame::from_json(&json, "base").unwrap();
        assert_eq!(back.scene.unwrap().playing, None);
    }

    /// Saves written before quick saves have no `scene` / `pending_scenes` and must still load.
    #[test]
    fn a_save_without_scene_fields_loads() {
        let mut v = serde_json::to_value(save()).unwrap();
        let o = v.as_object_mut().unwrap();
        o.remove("scene");
        o.remove("pending_scenes");
        let back = SaveGame::from_json(&v.to_string(), "base").unwrap();
        assert_eq!(back.scene, None);
        assert!(back.pending_scenes.is_empty());
        assert_eq!(back.version, PLAIN_SAVE_VERSION);
    }

    /// A save that needs the resume data is refused by an older game; a plain one is not.
    #[test]
    fn only_saves_with_a_scene_need_the_new_version() {
        let mut plain = save();
        plain.version = 99;
        plain.stamp_version();
        assert_eq!(plain.version, PLAIN_SAVE_VERSION);

        let mut mid_scene = save();
        mid_scene.scene = Some(resume());
        mid_scene.stamp_version();
        assert_eq!(mid_scene.version, SCENE_SAVE_VERSION);

        let mut queued = save();
        queued.pending_scenes = vec!["s".into()];
        queued.stamp_version();
        assert_eq!(queued.version, SCENE_SAVE_VERSION);
    }

    /// Growth queued for officers not in the army is dropped by a game that predates it, so a
    /// save that holds some is refused by such a game, whatever else it holds; one without any
    /// is still a plain save.
    #[test]
    fn a_save_with_queued_growth_needs_the_newest_version() {
        let mut with_growth = save();
        with_growth.campaign.pending_growth.insert(
            "gan_ning".into(),
            crate::campaign::Growth {
                levels: 11,
                class: None,
            },
        );
        with_growth.stamp_version();
        assert_eq!(with_growth.version, GROWTH_SAVE_VERSION);
        // A game with the scene layout (2) cannot read it.
        assert!(with_growth.version > SCENE_SAVE_VERSION);
        let back = SaveGame::from_json(&with_growth.to_json(), "base").unwrap();
        assert_eq!(back, with_growth);
        with_growth.scene = Some(resume());
        with_growth.stamp_version();
        assert_eq!(with_growth.version, GROWTH_SAVE_VERSION);

        let mut empty = save();
        empty.version = 99;
        empty.stamp_version();
        assert_eq!(empty.version, PLAIN_SAVE_VERSION);
    }

    /// A campaign with new game options must not load in a game that predates them; a plain
    /// one, and saves written before D25, stay plain.
    #[test]
    fn only_saves_with_new_game_options_need_their_version() {
        use crate::campaign::Difficulty;
        for edit in [false, true] {
            let mut s = save();
            s.campaign.free_edit = edit;
            s.campaign.extended_rules = !edit;
            s.stamp_version();
            assert_eq!(s.version, OPTIONS_SAVE_VERSION);
        }
        let mut hard = save();
        hard.campaign.difficulty = Difficulty::Hard;
        hard.stamp_version();
        assert_eq!(hard.version, OPTIONS_SAVE_VERSION);
        let back = SaveGame::from_json(&hard.to_json(), "base").unwrap();
        assert_eq!(back.campaign.difficulty, Difficulty::Hard);

        // Options outrank queued growth and a scene record.
        let mut both = save();
        both.campaign.extended_rules = true;
        both.campaign.pending_growth.insert(
            "x".into(),
            crate::campaign::Growth {
                levels: 1,
                class: None,
            },
        );
        both.scene = Some(resume());
        both.stamp_version();
        assert_eq!(both.version, OPTIONS_SAVE_VERSION);

        let mut v = serde_json::to_value(save()).unwrap();
        for field in ["difficulty", "free_edit", "extended_rules"] {
            v["campaign"].as_object_mut().unwrap().remove(field);
        }
        let old = SaveGame::from_json(&v.to_string(), "base").unwrap();
        assert_eq!(old.campaign.difficulty, Difficulty::Normal);
        assert!(!old.campaign.free_edit && !old.campaign.extended_rules);
        let mut normal = old;
        normal.stamp_version();
        assert_eq!(normal.version, PLAIN_SAVE_VERSION);
    }

    #[test]
    fn a_newer_save_is_refused() {
        let mut s = save();
        s.version = SAVE_VERSION + 1;
        assert_eq!(
            SaveGame::from_json(&s.to_json(), "base"),
            Err(SaveError::TooNew {
                found: SAVE_VERSION + 1,
                supported: SAVE_VERSION,
                needs: None
            })
        );
        // A newer game writes why; this one passes the words on.
        let mut v = serde_json::to_value(&s).unwrap();
        v[NEEDS_FIELD] = "새 기능".into();
        assert_eq!(
            SaveGame::from_json(&v.to_string(), "base"),
            Err(SaveError::TooNew {
                found: SAVE_VERSION + 1,
                supported: SAVE_VERSION,
                needs: Some("새 기능".into())
            })
        );
    }

    #[test]
    fn every_layout_above_the_plain_one_says_what_it_needs() {
        use crate::campaign::Difficulty;
        assert_eq!(version_needs(PLAIN_SAVE_VERSION), None);
        for version in PLAIN_SAVE_VERSION + 1..=SAVE_VERSION {
            assert!(version_needs(version).is_some(), "layout {version}");
        }
        let mut hard = save();
        hard.campaign.difficulty = Difficulty::Hard;
        hard.stamp_version();
        let json = hard.to_json();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v[NEEDS_FIELD], version_needs(OPTIONS_SAVE_VERSION).unwrap());
        // The field is only for older games: this one reads the save as before.
        assert_eq!(SaveGame::from_json(&json, "base").unwrap(), hard);
        let plain: serde_json::Value = serde_json::from_str(&save().to_json()).unwrap();
        assert!(plain.get(NEEDS_FIELD).is_none());
    }
}
