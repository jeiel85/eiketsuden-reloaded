//! Save slots on top of the [`KeyValueStore`]: one autosave slot, [`QUICK_SLOTS`] quick save
//! slots (F5 / F9, see [`crate::quicksave`]) and [`MANUAL_SLOTS`] manual slots per data pack, each holding one
//! [`SaveGame`] JSON document (`hero_core::save`).
//!
//! Every pack has its own set of slots (the storage key contains the pack id, see
//! [`SaveSlot::key`]), so playing another pack (a mod, the original mode) never overwrites the
//! saves of the base pack. Saves written before the slots were split per pack live under the
//! shared keys `save_auto` / `save_1` … `save_8`; [`migrate_legacy`] moves them to the slots of
//! the pack they belong to when that pack is loaded.
//!
//! The screens use [`list`] for the slot overview, [`write()`] / [`read`] / [`delete`] for the
//! actions and [`latest`] for the title screen's "continue".

use crate::platform::storage::{KeyValueStore, StorageError};
use hero_core::save::{SaveError, SaveGame};
use std::fmt;

/// Number of manual save slots.
pub const MANUAL_SLOTS: u8 = 8;

/// Number of quick save slots; F5 and F9 use the one chosen in the settings
/// (`Settings::quick_slot`).
pub const QUICK_SLOTS: u8 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SaveSlot {
    /// Written automatically when the campaign advances.
    Auto,
    /// Written by the quick save key at any moment, including in the middle of a scene or a
    /// battle animation: `1..=QUICK_SLOTS`, the one chosen in the settings.
    Quick(u8),
    /// Player-chosen slot, `1..=MANUAL_SLOTS`.
    Manual(u8),
}

impl SaveSlot {
    /// Every slot in display order: the autosave, the quick saves, then the manual slots.
    pub fn all() -> impl Iterator<Item = SaveSlot> {
        std::iter::once(SaveSlot::Auto)
            .chain((1..=QUICK_SLOTS).map(SaveSlot::Quick))
            .chain((1..=MANUAL_SLOTS).map(SaveSlot::Manual))
    }

    /// Slots the game writes by itself (or by a key): the slot list shows them but does not
    /// write into them.
    pub fn is_system(self) -> bool {
        matches!(self, SaveSlot::Auto | SaveSlot::Quick(_))
    }

    /// Storage key of the slot for the pack `pack_id`.
    ///
    /// A pack id that is itself a valid key fragment (lowercase ASCII letters, digits, `_`,
    /// `-`, at most [`MAX_PLAIN_PACK_ID`] bytes) is used as is: `save_base_auto`, `save_base_3`.
    /// Any other id is replaced by a stable 64-bit hash behind a different prefix
    /// (`save-0123456789abcdef_auto`), so the two forms never collide. Within each form the key
    /// is unique per (pack, slot): the slot suffix contains no `_`, so it is always the text
    /// after the last `_`.
    pub fn key(self, pack_id: &str) -> String {
        let suffix = self.suffix();
        if is_plain_pack_id(pack_id) {
            format!("save_{pack_id}_{suffix}")
        } else {
            format!("save-{:016x}_{suffix}", fnv1a64(pack_id.as_bytes()))
        }
    }

    /// Key used before the slots were split per pack (shared by every pack). The quick save
    /// slot did not exist then, so it has none.
    fn legacy_key(self) -> Option<String> {
        match self {
            SaveSlot::Quick(_) => None,
            _ => Some(format!("save_{}", self.suffix())),
        }
    }

    fn suffix(self) -> String {
        match self {
            SaveSlot::Auto => "auto".into(),
            // The first keeps the key of the single quick slot there was before, so that save
            // is quick slot 1 now.
            SaveSlot::Quick(1) => "quick".into(),
            SaveSlot::Quick(n) => format!("quick{n}"),
            SaveSlot::Manual(n) => n.to_string(),
        }
    }

    /// Name shown in slot lists.
    pub fn name(self) -> String {
        match self {
            SaveSlot::Auto => "자동 기록".into(),
            SaveSlot::Quick(n) => format!("순간 저장 {n}"),
            SaveSlot::Manual(n) => format!("기록 {n}"),
        }
    }

    fn is_valid(self) -> bool {
        match self {
            SaveSlot::Auto => true,
            SaveSlot::Quick(n) => (1..=QUICK_SLOTS).contains(&n),
            SaveSlot::Manual(n) => (1..=MANUAL_SLOTS).contains(&n),
        }
    }
}

/// Longest pack id that is used verbatim in a storage key (`save_` + id + `_quick4`, the longest
/// slot suffix, stays within the 64-byte key limit of `platform::storage`).
pub const MAX_PLAIN_PACK_ID: usize = 50;

fn is_plain_pack_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_PLAIN_PACK_ID
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

/// FNV-1a, 64 bit: a stable hash (the std hashers are not guaranteed to be stable across
/// releases, and the key must not change between versions of the game).
fn fnv1a64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// What the slot list shows for a save.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotSummary {
    pub label: String,
    /// Unix seconds (0 when unknown).
    pub saved_at: u64,
    pub play_seconds: u64,
    /// The save was made during a battle.
    pub mid_battle: bool,
    /// The save was made in the middle of a drama scene (a quick save).
    pub mid_scene: bool,
    pub pack_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotStatus {
    Empty,
    Ready(SlotSummary),
    /// Present but not loadable (corrupt, newer version, another pack, storage error).
    Unreadable(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotInfo {
    pub slot: SaveSlot,
    pub status: SlotStatus,
}

impl SlotInfo {
    pub fn summary(&self) -> Option<&SlotSummary> {
        match &self.status {
            SlotStatus::Ready(s) => Some(s),
            _ => None,
        }
    }
}

/// Why a slot could not be loaded or written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveSlotError {
    Empty(SaveSlot),
    InvalidSlot(SaveSlot),
    Storage(StorageError),
    Save(SaveError),
}

impl fmt::Display for SaveSlotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SaveSlotError::Empty(slot) => write!(f, "{}은(는) 비어 있습니다", slot.name()),
            SaveSlotError::InvalidSlot(slot) => write!(f, "잘못된 슬롯: {slot:?}"),
            SaveSlotError::Storage(e) => write!(f, "저장소 오류 ({e})"),
            SaveSlotError::Save(e) => match e {
                SaveError::Corrupt(msg) => write!(f, "기록이 손상되었습니다 ({msg})"),
                SaveError::TooNew {
                    found,
                    supported,
                    needs: Some(needs),
                } => {
                    let mut shown: String = needs.chars().take(MAX_NEEDS_CHARS).collect();
                    if shown.len() < needs.len() {
                        shown.push('…');
                    }
                    write!(
                        f,
                        "새 버전의 기록입니다: {shown} (기록 v{found}, 지원 v{supported})"
                    )
                }
                SaveError::TooNew {
                    found, supported, ..
                } => write!(f, "새 버전의 기록입니다 (기록 v{found}, 지원 v{supported})"),
                SaveError::WrongPack { found, expected } => write!(
                    f,
                    "다른 데이터 팩의 기록입니다 (기록 `{found}`, 현재 `{expected}`)"
                ),
            },
        }
    }
}

impl std::error::Error for SaveSlotError {}

/// Summaries of every slot in display order.
pub fn list(store: &dyn KeyValueStore, pack_id: &str) -> Vec<SlotInfo> {
    SaveSlot::all()
        .map(|slot| {
            let status = match read(store, slot, pack_id) {
                Ok(save) => SlotStatus::Ready(SlotSummary {
                    label: save.label.clone(),
                    saved_at: save.saved_at,
                    play_seconds: save.campaign.play_seconds,
                    mid_battle: save.battle.is_some(),
                    mid_scene: save.scene.is_some(),
                    pack_version: save.pack_version.clone(),
                }),
                Err(SaveSlotError::Empty(_)) => SlotStatus::Empty,
                Err(e) => SlotStatus::Unreadable(e.to_string()),
            };
            SlotInfo { slot, status }
        })
        .collect()
}

/// Load a slot, checking the save version and that it belongs to `pack_id`.
pub fn read(
    store: &dyn KeyValueStore,
    slot: SaveSlot,
    pack_id: &str,
) -> Result<SaveGame, SaveSlotError> {
    if !slot.is_valid() {
        return Err(SaveSlotError::InvalidSlot(slot));
    }
    let json = store
        .get(&slot.key(pack_id))
        .map_err(SaveSlotError::Storage)?
        .ok_or(SaveSlotError::Empty(slot))?;
    SaveGame::from_json(&json, pack_id).map_err(SaveSlotError::Save)
}

/// Write a save into a slot of its own pack (`save.pack_id`), replacing that slot's content
/// atomically. The slots of other packs are never touched.
pub fn write(
    store: &mut dyn KeyValueStore,
    slot: SaveSlot,
    save: &SaveGame,
) -> Result<(), SaveSlotError> {
    if !slot.is_valid() {
        return Err(SaveSlotError::InvalidSlot(slot));
    }
    store
        .set(&slot.key(&save.pack_id), &save.to_json())
        .map_err(SaveSlotError::Storage)
}

/// Delete a slot of the pack `pack_id`.
pub fn delete(
    store: &mut dyn KeyValueStore,
    slot: SaveSlot,
    pack_id: &str,
) -> Result<(), SaveSlotError> {
    if !slot.is_valid() {
        return Err(SaveSlotError::InvalidSlot(slot));
    }
    store
        .remove(&slot.key(pack_id))
        .map_err(SaveSlotError::Storage)
}

/// Move the saves of `pack_id` from the shared pre-split keys (`save_auto`, `save_1` …) into
/// the pack's own slots. Returns how many saves were moved.
///
/// A legacy save is moved only when its `pack_id` field names this pack; saves of other packs,
/// unreadable documents and saves whose new slot is already taken stay where they are (they are
/// never deleted). The document is copied verbatim, so a save written by a newer game version
/// keeps its content. The legacy key is removed only after the copy succeeded; on a storage
/// error the remaining saves are retried the next time the pack is loaded.
pub fn migrate_legacy(
    store: &mut dyn KeyValueStore,
    pack_id: &str,
) -> Result<usize, SaveSlotError> {
    let mut moved = 0;
    for slot in SaveSlot::all() {
        let Some(legacy) = slot.legacy_key() else {
            continue;
        };
        let Some(json) = store.get(&legacy).map_err(SaveSlotError::Storage)? else {
            continue;
        };
        let owner = serde_json::from_str::<serde_json::Value>(&json)
            .ok()
            .and_then(|v| v.get("pack_id")?.as_str().map(str::to_owned));
        if owner.as_deref() != Some(pack_id) {
            continue;
        }
        let key = slot.key(pack_id);
        match store.get(&key).map_err(SaveSlotError::Storage)? {
            None => {
                store.set(&key, &json).map_err(SaveSlotError::Storage)?;
                moved += 1;
            }
            // An earlier migration copied it but could not remove the legacy key.
            Some(current) if current == json => {}
            // The slot was written since; keep both rather than guess which one matters.
            Some(_) => continue,
        }
        store.remove(&legacy).map_err(SaveSlotError::Storage)?;
    }
    Ok(moved)
}

/// Which slot wins when two saves carry the same `saved_at`: the quick saves, then the autosave,
/// then the manual slots (each group in list order).
///
/// Why: `saved_at` counts whole seconds, so saves made within one second tie. The quick save is
/// only ever written by a key press in a running game, which comes after the autosave made on
/// arriving at a node, so on a tie it is the newer one; and a quick save that loses ties would
/// be ignored by 이어하기 right after an autosave, which is when it is most often made.
fn tie_rank(slot: SaveSlot) -> u8 {
    match slot {
        SaveSlot::Quick(_) => 0,
        SaveSlot::Auto => 1,
        SaveSlot::Manual(_) => 2,
    }
}

/// The most recently saved loadable slot of `pack_id` (for "continue").
pub fn latest(store: &dyn KeyValueStore, pack_id: &str) -> Option<SaveSlot> {
    let mut candidates: Vec<(u64, SaveSlot)> = list(store, pack_id)
        .into_iter()
        .filter_map(|info| info.summary().map(|s| (s.saved_at, info.slot)))
        .collect();
    // Stable, so slots of the same rank stay in list order.
    candidates.sort_by_key(|(_, slot)| tie_rank(*slot));
    candidates
        .into_iter()
        // Newest first; on equal timestamps the slot that comes first wins.
        .fold(
            None,
            |best: Option<(u64, SaveSlot)>, (at, slot)| match best {
                Some((b, _)) if b >= at => best,
                _ => Some((at, slot)),
            },
        )
        .map(|(_, slot)| slot)
}

/// Whether any slot holds a loadable save of `pack_id`.
pub fn any(store: &dyn KeyValueStore, pack_id: &str) -> bool {
    SaveSlot::all().any(|slot| read(store, slot, pack_id).is_ok())
}

/// Whether any slot of `pack_id` holds something, loadable or not: the load screen is worth
/// opening then, since it says why a record cannot be loaded (a newer game's save, say).
pub fn any_record(store: &dyn KeyValueStore, pack_id: &str) -> bool {
    SaveSlot::all().any(|slot| !matches!(read(store, slot, pack_id), Err(SaveSlotError::Empty(_))))
}

/// How many slots of `pack_id` hold something, loadable or not.
pub fn record_count(store: &dyn KeyValueStore, pack_id: &str) -> usize {
    SaveSlot::all()
        .filter(|slot| !matches!(read(store, *slot, pack_id), Err(SaveSlotError::Empty(_))))
        .count()
}

/// Remove every record of `pack_id`: manual, quick and automatic alike. Returns how many were
/// removed.
///
/// Why it is one call: tidying old records meant deleting them one by one from the save screen
/// (the only place with a Delete), and a stale autosave or quick save is not even selectable
/// there while saving. A slot already empty is not an error, so a retry after a partial failure
/// finishes the job; a failing slot stops the walk and is reported, and the count says how many
/// were removed before it.
pub fn clear(store: &mut dyn KeyValueStore, pack_id: &str) -> Result<usize, SaveSlotError> {
    let mut removed = 0;
    for slot in SaveSlot::all() {
        if matches!(read(store, slot, pack_id), Err(SaveSlotError::Empty(_))) {
            continue;
        }
        match store.remove(&slot.key(pack_id)) {
            Ok(()) => removed += 1,
            Err(e) => {
                return Err(SaveSlotError::Storage(e));
            }
        }
    }
    Ok(removed)
}

/// The document to carry a save to another device: its JSON text, as stored.
///
/// Why the stored text and not a re-serialized [`SaveGame`]: a record written by a newer game
/// version must survive the trip unchanged (the importing game then says why it cannot open it,
/// instead of the exporting game silently dropping the fields it does not know).
pub fn export_text(
    store: &dyn KeyValueStore,
    slot: SaveSlot,
    pack_id: &str,
) -> Result<String, SaveSlotError> {
    if !slot.is_valid() {
        return Err(SaveSlotError::InvalidSlot(slot));
    }
    store
        .get(&slot.key(pack_id))
        .map_err(SaveSlotError::Storage)?
        .ok_or(SaveSlotError::Empty(slot))
}

/// Why an imported record was refused, or which slot would receive it.
#[derive(Debug, Clone, PartialEq)]
pub struct Imported {
    pub save: SaveGame,
    /// The manual slot the record would be written into: the first one that is empty, else the
    /// first manual slot (the caller asks before overwriting it).
    pub slot: SaveSlot,
}

/// Read a record pasted from another device, checking it belongs to `pack_id`.
///
/// Input: the text (surrounding whitespace is ignored, so a paste from a mail or chat window
/// works), the pack being played, and the slot list that decides where it would go.
/// Output: the save and its destination slot, or why it cannot be imported.
pub fn import_text(text: &str, pack_id: &str, slots: &[SlotInfo]) -> Result<Imported, ImportError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(ImportError::NoText);
    }
    let save = SaveGame::from_json(text, pack_id)
        // The player-facing wording (`다른 데이터 팩의 기록입니다`, `새 버전의 기록입니다` …), not
        // the developer message of the core error.
        .map_err(|e| ImportError::Refused(SaveSlotError::Save(e).to_string()))?;
    let free = slots
        .iter()
        .find(|info| matches!(info.slot, SaveSlot::Manual(_)) && info.status == SlotStatus::Empty)
        .map(|info| info.slot);
    let slot = free.unwrap_or(SaveSlot::Manual(1));
    Ok(Imported { save, slot })
}

/// Why a pasted record could not be imported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportError {
    /// The clipboard had nothing that looks like a record.
    NoText,
    /// The text is not a record of this pack, or is too new / unreadable (the reason in words).
    Refused(String),
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImportError::NoText => write!(f, "클립보드에 기록이 없습니다"),
            ImportError::Refused(why) => write!(f, "기록을 가져올 수 없습니다: {why}"),
        }
    }
}

impl std::error::Error for ImportError {}

/// Longest reason of a newer save ([`SaveError::TooNew`]) shown, in characters: it comes from
/// the save file. With the words around it the message should stay within the load screen's
/// detail panel (two lines; the reasons of this game's layouts fit one).
const MAX_NEEDS_CHARS: usize = 30;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::storage::MemoryStore;
    use hero_core::campaign::CampaignState;
    use hero_core::save::SAVE_VERSION;
    use std::collections::BTreeMap;

    fn save(label: &str, at: u64, pack: &str) -> SaveGame {
        SaveGame {
            version: SAVE_VERSION,
            pack_id: pack.into(),
            pack_version: "0.1.0".into(),
            label: label.into(),
            saved_at: at,
            campaign: CampaignState {
                node: "start".into(),
                roster: Vec::new(),
                inventory: BTreeMap::new(),
                gold: 100,
                flags: BTreeMap::new(),
                deployed: Vec::new(),
                battles_won: Vec::new(),
                play_seconds: 3600,
                pending_growth: BTreeMap::new(),
                difficulty: Default::default(),
                free_edit: false,
                extended_rules: false,
            },
            battle: None,
            scene: None,
            pending_scenes: Vec::new(),
            battle_replay: None,
        }
    }

    #[test]
    fn slots_and_keys() {
        let all: Vec<_> = SaveSlot::all().collect();
        assert_eq!(all.len(), 1 + QUICK_SLOTS as usize + MANUAL_SLOTS as usize);
        assert_eq!(all[0], SaveSlot::Auto);
        assert_eq!(all[1..5], (1..=4).map(SaveSlot::Quick).collect::<Vec<_>>());
        // The first quick slot keeps the key of the single quick slot there was before.
        assert_eq!(SaveSlot::Quick(1).key("base"), "save_base_quick");
        assert_eq!(SaveSlot::Quick(4).key("base"), "save_base_quick4");
        assert_eq!(SaveSlot::Quick(2).name(), "순간 저장 2");
        assert!(SaveSlot::Quick(3).is_system() && SaveSlot::Auto.is_system());
        // The longest key a verbatim pack id makes stays within the storage's limit.
        let longest = "a".repeat(MAX_PLAIN_PACK_ID);
        assert!(SaveSlot::Quick(QUICK_SLOTS).key(&longest).len() <= 64);
        assert!(!SaveSlot::Manual(1).is_system());
        assert_eq!(SaveSlot::Manual(3).key("base"), "save_base_3");
        assert_eq!(SaveSlot::Auto.key("base"), "save_base_auto");
        let mut store = MemoryStore::default();
        assert_eq!(
            write(&mut store, SaveSlot::Manual(9), &save("x", 1, "base")),
            Err(SaveSlotError::InvalidSlot(SaveSlot::Manual(9)))
        );
        assert_eq!(
            write(&mut store, SaveSlot::Quick(5), &save("x", 1, "base")),
            Err(SaveSlotError::InvalidSlot(SaveSlot::Quick(5)))
        );
    }

    #[test]
    fn a_newer_save_says_why_and_still_opens_the_load_screen() {
        let mut store = MemoryStore::default();
        let newer = |needs: &str| {
            format!(
                r#"{{"version":{},"pack_id":"base","needs":"{needs}"}}"#,
                SAVE_VERSION + 1
            )
        };
        store
            .set(&SaveSlot::Manual(1).key("base"), &newer("새 기능"))
            .unwrap();
        // Nothing to continue, but the load screen shows the record and why.
        assert!(!any(&store, "base"));
        assert!(any_record(&store, "base"));
        assert!(!any_record(&store, "other"));
        let why = read(&store, SaveSlot::Manual(1), "base")
            .unwrap_err()
            .to_string();
        assert_eq!(
            why,
            format!(
                "새 버전의 기록입니다: 새 기능 (기록 v{}, 지원 v{SAVE_VERSION})",
                SAVE_VERSION + 1
            )
        );
        // A long reason from the file is cut.
        store
            .set(&SaveSlot::Manual(1).key("base"), &newer(&"가".repeat(200)))
            .unwrap();
        let why = read(&store, SaveSlot::Manual(1), "base")
            .unwrap_err()
            .to_string();
        assert!(
            why.contains(&format!("{}…", "가".repeat(MAX_NEEDS_CHARS))),
            "{why}"
        );
        assert!(!why.contains(&"가".repeat(MAX_NEEDS_CHARS + 1)));
    }

    #[test]
    fn write_read_list_delete() {
        let mut store = MemoryStore::default();
        assert!(!any(&store, "base"));
        assert!(!any_record(&store, "base"));
        assert_eq!(latest(&store, "base"), None);

        write(&mut store, SaveSlot::Manual(2), &save("탁현", 100, "base")).unwrap();
        write(&mut store, SaveSlot::Auto, &save("자동", 200, "base")).unwrap();
        write(
            &mut store,
            SaveSlot::Manual(5),
            &save("다른 팩", 300, "other"),
        )
        .unwrap();
        store
            .set(&SaveSlot::Manual(7).key("base"), "garbage")
            .unwrap();

        let loaded = read(&store, SaveSlot::Manual(2), "base").unwrap();
        assert_eq!(loaded.label, "탁현");
        assert!(matches!(
            read(&store, SaveSlot::Manual(1), "base"),
            Err(SaveSlotError::Empty(SaveSlot::Manual(1)))
        ));

        let infos = list(&store, "base");
        assert_eq!(infos.len(), 13);
        assert_eq!(infos[0].summary().unwrap().label, "자동");
        assert_eq!(infos[0].summary().unwrap().play_seconds, 3600);
        // Indexes 1–4 are the quick save slots, 5.. the manual slots.
        assert_eq!(infos[1].slot, SaveSlot::Quick(1));
        assert_eq!(infos[1].status, SlotStatus::Empty);
        assert_eq!(infos[5].slot, SaveSlot::Manual(1));
        assert_eq!(infos[5].status, SlotStatus::Empty);
        // The other pack's save lives in the other pack's slots.
        assert_eq!(infos[9].status, SlotStatus::Empty);
        assert!(matches!(infos[11].status, SlotStatus::Unreadable(_)));
        assert_eq!(
            read(&store, SaveSlot::Manual(5), "other").unwrap().label,
            "다른 팩"
        );

        // The other pack's newer save is ignored.
        assert_eq!(latest(&store, "base"), Some(SaveSlot::Auto));
        assert!(any(&store, "base"));

        delete(&mut store, SaveSlot::Auto, "base").unwrap();
        assert_eq!(latest(&store, "base"), Some(SaveSlot::Manual(2)));
        assert_eq!(latest(&store, "other"), Some(SaveSlot::Manual(5)));
    }

    /// Regression: the autosave of one pack used to overwrite the autosave of every other pack
    /// (all packs shared `save_auto`).
    #[test]
    fn packs_do_not_share_slots() {
        let mut store = MemoryStore::default();
        write(&mut store, SaveSlot::Auto, &save("기본", 100, "base")).unwrap();
        write(&mut store, SaveSlot::Auto, &save("원작", 200, "original")).unwrap();
        assert_eq!(read(&store, SaveSlot::Auto, "base").unwrap().label, "기본");
        assert_eq!(
            read(&store, SaveSlot::Auto, "original").unwrap().label,
            "원작"
        );
        delete(&mut store, SaveSlot::Auto, "original").unwrap();
        assert_eq!(read(&store, SaveSlot::Auto, "base").unwrap().label, "기본");
    }

    #[test]
    fn keys_are_valid_and_distinct_for_any_pack_id() {
        let long = "a".repeat(MAX_PLAIN_PACK_ID + 1);
        let ids = [
            "base",
            "base_auto",
            "base-1",
            "Base",
            "원작",
            "mod.balance",
            &long,
            &long[..MAX_PLAIN_PACK_ID],
        ];
        let mut seen = std::collections::BTreeSet::new();
        for id in ids {
            for slot in SaveSlot::all() {
                let key = slot.key(id);
                crate::platform::storage::validate_key(&key)
                    .unwrap_or_else(|e| panic!("{id:?} {slot:?}: {e}"));
                assert!(seen.insert(key.clone()), "duplicate key {key}");
                assert_ne!(Some(key), slot.legacy_key());
            }
        }
        // Stable across runs and versions.
        assert_eq!(SaveSlot::Auto.key("원작"), SaveSlot::Auto.key("원작"));
        assert!(SaveSlot::Auto.key("Base").starts_with("save-"));
        // A hashed id still round-trips through write/read.
        let mut store = MemoryStore::default();
        write(&mut store, SaveSlot::Manual(1), &save("x", 1, "원작")).unwrap();
        assert_eq!(
            read(&store, SaveSlot::Manual(1), "원작").unwrap().label,
            "x"
        );
    }

    #[test]
    fn legacy_saves_move_to_their_own_pack() {
        let mut store = MemoryStore::default();
        store
            .set("save_auto", &save("옛 자동", 10, "base").to_json())
            .unwrap();
        store
            .set("save_2", &save("옛 기록", 20, "base").to_json())
            .unwrap();
        store
            .set("save_3", &save("다른 팩", 30, "other").to_json())
            .unwrap();
        store.set("save_4", "garbage").unwrap();
        // Slot 5 already has a new save of this pack: the legacy one must not replace it.
        store
            .set("save_5", &save("옛 5", 40, "base").to_json())
            .unwrap();
        write(&mut store, SaveSlot::Manual(5), &save("새 5", 50, "base")).unwrap();

        assert_eq!(migrate_legacy(&mut store, "base").unwrap(), 2);
        assert_eq!(
            read(&store, SaveSlot::Auto, "base").unwrap().label,
            "옛 자동"
        );
        assert_eq!(
            read(&store, SaveSlot::Manual(2), "base").unwrap().label,
            "옛 기록"
        );
        assert_eq!(
            read(&store, SaveSlot::Manual(5), "base").unwrap().label,
            "새 5"
        );
        assert_eq!(store.get("save_auto").unwrap(), None);
        assert_eq!(store.get("save_2").unwrap(), None);
        // Other packs' saves, unreadable data and conflicts stay untouched.
        assert!(store.get("save_3").unwrap().is_some());
        assert!(store.get("save_4").unwrap().is_some());
        assert!(store.get("save_5").unwrap().is_some());

        // Idempotent; the other pack picks its save up when it is loaded.
        assert_eq!(migrate_legacy(&mut store, "base").unwrap(), 0);
        assert_eq!(migrate_legacy(&mut store, "other").unwrap(), 1);
        assert_eq!(
            read(&store, SaveSlot::Manual(3), "other").unwrap().label,
            "다른 팩"
        );
    }

    #[test]
    fn legacy_copy_left_behind_by_a_failed_remove_is_cleaned_up() {
        let mut store = MemoryStore::default();
        let json = save("옛 자동", 10, "base").to_json();
        store.set("save_auto", &json).unwrap();
        store.set(&SaveSlot::Auto.key("base"), &json).unwrap();
        assert_eq!(migrate_legacy(&mut store, "base").unwrap(), 0);
        assert_eq!(store.get("save_auto").unwrap(), None);
        assert_eq!(
            read(&store, SaveSlot::Auto, "base").unwrap().label,
            "옛 자동"
        );
    }

    /// Each quick save slot is its own slot: 이어하기 (`latest`) takes the newest of them, and
    /// they never touch each other, the slots of other packs or the pre-split legacy keys.
    #[test]
    fn the_quick_slots_are_their_own_slots() {
        let mut store = MemoryStore::default();
        write(&mut store, SaveSlot::Quick(1), &save("하나", 100, "base")).unwrap();
        write(&mut store, SaveSlot::Quick(3), &save("셋", 300, "base")).unwrap();
        assert_eq!(
            read(&store, SaveSlot::Quick(1), "base").unwrap().label,
            "하나"
        );
        assert_eq!(
            read(&store, SaveSlot::Quick(3), "base").unwrap().label,
            "셋"
        );
        assert!(matches!(
            read(&store, SaveSlot::Quick(2), "base"),
            Err(SaveSlotError::Empty(SaveSlot::Quick(2)))
        ));
        assert_eq!(latest(&store, "base"), Some(SaveSlot::Quick(3)));
    }

    #[test]
    fn the_quick_slot_is_its_own_slot() {
        let mut store = MemoryStore::default();
        write(&mut store, SaveSlot::Auto, &save("자동", 100, "base")).unwrap();
        write(&mut store, SaveSlot::Quick(1), &save("순간", 200, "base")).unwrap();
        write(&mut store, SaveSlot::Manual(1), &save("수동", 150, "base")).unwrap();
        write(
            &mut store,
            SaveSlot::Quick(1),
            &save("다른 팩", 900, "other"),
        )
        .unwrap();

        assert_eq!(
            read(&store, SaveSlot::Quick(1), "base").unwrap().label,
            "순간"
        );
        assert_eq!(read(&store, SaveSlot::Auto, "base").unwrap().label, "자동");
        assert_eq!(latest(&store, "base"), Some(SaveSlot::Quick(1)));
        assert_eq!(latest(&store, "other"), Some(SaveSlot::Quick(1)));

        delete(&mut store, SaveSlot::Quick(1), "base").unwrap();
        assert_eq!(latest(&store, "base"), Some(SaveSlot::Manual(1)));
        assert_eq!(
            read(&store, SaveSlot::Quick(1), "other").unwrap().label,
            "다른 팩"
        );

        // No legacy key ever existed for it, so migrating leaves it alone.
        store
            .set("save_quick", &save("x", 1, "base").to_json())
            .unwrap();
        assert_eq!(migrate_legacy(&mut store, "base").unwrap(), 0);
        assert!(store.get("save_quick").unwrap().is_some());
    }

    #[test]
    fn latest_prefers_autosave_on_ties() {
        let mut store = MemoryStore::default();
        write(&mut store, SaveSlot::Manual(1), &save("a", 50, "base")).unwrap();
        write(&mut store, SaveSlot::Auto, &save("b", 50, "base")).unwrap();
        assert_eq!(latest(&store, "base"), Some(SaveSlot::Auto));
    }

    /// Regression: `saved_at` counts whole seconds, and a quick save made in the same second as
    /// the autosave of the node just entered used to lose the tie, so 이어하기 ignored it.
    #[test]
    fn a_quick_save_wins_a_tie_with_the_autosave_and_manual_slots() {
        let mut store = MemoryStore::default();
        write(&mut store, SaveSlot::Manual(1), &save("수동", 50, "base")).unwrap();
        write(&mut store, SaveSlot::Auto, &save("자동", 50, "base")).unwrap();
        write(&mut store, SaveSlot::Quick(1), &save("순간", 50, "base")).unwrap();
        assert_eq!(latest(&store, "base"), Some(SaveSlot::Quick(1)));

        // Only a tie: an older quick save loses to a newer autosave.
        write(&mut store, SaveSlot::Auto, &save("자동", 51, "base")).unwrap();
        assert_eq!(latest(&store, "base"), Some(SaveSlot::Auto));

        // Among manual slots the first one still wins a tie.
        let mut store = MemoryStore::default();
        write(&mut store, SaveSlot::Manual(3), &save("c", 70, "base")).unwrap();
        write(&mut store, SaveSlot::Manual(2), &save("b", 70, "base")).unwrap();
        assert_eq!(latest(&store, "base"), Some(SaveSlot::Manual(2)));
    }

    #[test]
    fn error_messages_are_readable() {
        let e = SaveSlotError::Save(SaveError::WrongPack {
            found: "x".into(),
            expected: "base".into(),
        });
        assert!(e.to_string().contains("다른 데이터 팩"));
        assert!(SaveSlotError::Empty(SaveSlot::Auto)
            .to_string()
            .contains("자동 기록"));
    }

    /// Clearing removes every record of the pack — the autosave and the quick saves included,
    /// which the save screen cannot even write into — and nothing of another pack.
    #[test]
    fn clearing_takes_every_record_of_the_pack_and_no_other() {
        let mut store = MemoryStore::default();
        write(&mut store, SaveSlot::Auto, &save("자동", 100, "base")).unwrap();
        write(&mut store, SaveSlot::Quick(2), &save("순간", 200, "base")).unwrap();
        write(&mut store, SaveSlot::Manual(3), &save("기록", 300, "base")).unwrap();
        store
            .set(&SaveSlot::Manual(7).key("base"), "garbage")
            .unwrap();
        write(&mut store, SaveSlot::Auto, &save("다른 팩", 400, "other")).unwrap();

        assert_eq!(record_count(&store, "base"), 4);
        assert_eq!(clear(&mut store, "base").unwrap(), 4);
        assert_eq!(record_count(&store, "base"), 0);
        assert!(!any(&store, "base"));
        assert!(!any_record(&store, "base"));
        // Another pack's saves are its own.
        assert_eq!(record_count(&store, "other"), 1);
        assert_eq!(
            read(&store, SaveSlot::Auto, "other").unwrap().label,
            "다른 팩"
        );
        // Nothing left to clear is not a failure.
        assert_eq!(clear(&mut store, "base").unwrap(), 0);
    }

    /// A save travels as the text it was stored as: a record a newer game wrote keeps its own
    /// fields, so the importing game can say what it would need instead of losing them.
    #[test]
    fn a_record_travels_as_the_text_it_was_stored_as() {
        let mut store = MemoryStore::default();
        let json = save("탁현", 100, "base").to_json();
        write(&mut store, SaveSlot::Manual(2), &save("탁현", 100, "base")).unwrap();
        assert_eq!(
            export_text(&store, SaveSlot::Manual(2), "base").unwrap(),
            json
        );
        assert!(matches!(
            export_text(&store, SaveSlot::Manual(1), "base"),
            Err(SaveSlotError::Empty(SaveSlot::Manual(1)))
        ));

        // A newer record: the fields this game does not know stay in the text.
        let newer = format!(
            r#"{{"version":{},"pack_id":"base","label":"새 기록","future":42}}"#,
            SAVE_VERSION + 1
        );
        store.set(&SaveSlot::Manual(4).key("base"), &newer).unwrap();
        assert_eq!(
            export_text(&store, SaveSlot::Manual(4), "base").unwrap(),
            newer
        );
    }

    /// A pasted record goes into the first empty manual slot, else the first manual slot (the
    /// caller asks before overwriting); trimming lets it arrive from a mail or chat window.
    #[test]
    fn an_imported_record_takes_the_first_free_manual_slot() {
        let mut store = MemoryStore::default();
        write(&mut store, SaveSlot::Manual(1), &save("하나", 100, "base")).unwrap();
        write(&mut store, SaveSlot::Manual(2), &save("둘", 200, "base")).unwrap();
        let slots = list(&store, "base");
        let text = format!("\n  {}\n\n", save("가져옴", 300, "base").to_json());

        let imported = import_text(&text, "base", &slots).unwrap();
        assert_eq!(imported.save.label, "가져옴");
        assert_eq!(imported.slot, SaveSlot::Manual(3));
        write(&mut store, imported.slot, &imported.save).unwrap();
        assert_eq!(
            read(&store, SaveSlot::Manual(3), "base").unwrap().label,
            "가져옴"
        );

        // With every manual slot taken, the first one is offered (the caller confirms).
        for n in 3..=MANUAL_SLOTS {
            write(&mut store, SaveSlot::Manual(n), &save("참", 300, "base")).unwrap();
        }
        let slots = list(&store, "base");
        let imported = import_text(&text, "base", &slots).unwrap();
        assert_eq!(imported.slot, SaveSlot::Manual(1));
    }

    /// Text that is not this pack's record is refused with the reason, never half-imported.
    #[test]
    fn an_import_refuses_what_it_cannot_play() {
        let store = MemoryStore::default();
        let slots = list(&store, "base");
        assert_eq!(
            import_text("   \n ", "base", &slots),
            Err(ImportError::NoText)
        );
        assert!(matches!(
            import_text("not a save", "base", &slots),
            Err(ImportError::Refused(_))
        ));
        let denied = import_text(&save("x", 1, "other").to_json(), "base", &slots).unwrap_err();
        assert!(denied.to_string().contains("다른 데이터 팩"), "{denied}");
        let newer = format!(r#"{{"version":{},"pack_id":"base"}}"#, SAVE_VERSION + 1);
        assert!(matches!(
            import_text(&newer, "base", &slots),
            Err(ImportError::Refused(_))
        ));
    }
}
