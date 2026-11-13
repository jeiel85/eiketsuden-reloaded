# Architecture

Eiketsuden Reloaded follows the OpenRCT2 model: an original, open-source engine plus data.
The engine never embeds content; everything the player sees comes from a **data pack**. How the engine
is developed (and how it differs from OpenRCT2's decompilation route) is in [ENGINE.md](ENGINE.md).

```
┌──────────────────────────── hero-game (bin: eiketsuden) ────────────────────────────┐
│ macroquad: window, input, audio, rendering (native Win/Linux/macOS + wasm32)        │
│  app.rs ── screen flow driven by campaign nodes                                     │
│  screens/ title · drama · camp (deploy/equip/shop/save) · battle · saveload · controls · ending │
│  ui/ widgets · gfx.rs virtual canvas · assets.rs · audio.rs · platform/ (storage)   │
└───────────────▲─────────────────────────────────────────────────────────────────────┘
                │ pure Rust API (no I/O)
┌───────────────┴──────────────────────── hero-core ───────────────────────────────────┐
│ data.rs rules schema · battledef.rs · map.rs · pack.rs (load + validate)            │
│ script.rs .drama parser · drama.rs runner · campaign.rs army/flow · save.rs          │
│ battle/ state · movement/ZOC · combat · strategies · events · AI                     │
└───────────────▲─────────────────────────────────────────────────────────────────────┘
                │
┌───────────────┴──────── hero-tools (bin) ────────┐   ┌──── data/base (base pack) ────┐
│ validate <pack> · simulate <pack> (AI vs AI)     │   │ pack.toml, rules/, officers,   │
│ original probe|extract|pack <dir> (hero-import)  │   │ campaign, battles/, dramas/,   │
└──────────────────────────────────────────────────┘   │ gfx/, bgm/, sfx/, fonts/       │
                                                        └────────────────────────────────┘
```

## Crates

| Crate | Role | Depends on |
|---|---|---|
| `hero-core` | Game rules, data schema, pack loading (including layered packs) and validation, drama scripting, campaign and save state. Deterministic (seeded [`Rng`](../crates/hero-core/src/rng.rs)); no graphics, no clock, no file I/O except the optional `DirSource`. | serde, toml, serde_json, thiserror |
| `hero-game` | The game executable (`eiketsuden` / `eiketsuden.exe` / `eiketsuden.wasm`). | hero-core, macroquad |
| `hero-tools` | Command line tools for pack authors and CI: `validate`, `simulate`, `info`; `original probe` / `extract` / `pack` for the importer. | hero-core, hero-import |
| `hero-import` | Optional, experimental importer for an owned copy of the original game: edition probe and shareable manifest, LS11 / 6-byte-table containers, TF-DCE portraits, planar sprites and palettes, maps, message text, scenario bytecode and the `BAKDATA` tables → a media overlay folder that the game reads with `--original <dir>` (native only), and the original-mode pack (`pack`). See [ORIGINAL_DATA.md](ORIGINAL_DATA.md); the verified file formats and the method are in [reverse-engineering/](reverse-engineering/README.md). | encoding_rs, png, sha2, serde, serde_json |

## Data flow

1. **Startup** — `hero-game` resolves the pack directory (`--data <dir>`, env `EIKETSUDEN_DATA`,
   `<exe dir>/data/base`, `./data/base`) and reads its `pack.toml`. If the pack `extends` another
   pack, it reads that pack's `pack.toml` too, and so on (`PackChain`, see
   [Layered packs](#layered-packs)). Then it reads every other text file from
   `PackChain::text_files()` (async `load_file`, works for both fetch and disk) and calls
   `Pack::load`. Media files are loaded lazily by key, looked up in the top pack first and then in
   each pack it extends.
2. **Campaign** — `CampaignState` holds the current node id. The app shows the screen for the node
   (`Drama` → drama screen, `Camp` → camp screen, `Battle` → battle screen, `Ending` → ending) and calls
   `CampaignState::advance` when the screen finishes. `Branch` nodes are resolved inside `advance`.
3. **Drama** — `DramaRunner::next` yields presentation `Step`s and applies side effects (flags, gold,
   items, officers joining) to the campaign.
4. **Battle** — `BattleState::new` builds units from the roster, `begin` starts turn 1. The battle
   screen turns input into `Action`s, calls `apply`, and animates the returned `BattleEvent`s. AI
   phases use `next_ai_unit` + `ai_actions`. When the battle ends (a victory, or a defeat on a node
   with `on_defeat`), `CampaignState::apply_battle_result` copies the officers' progress back and
   takes out exactly the consumables the battle recorded in `items_used`; found gold and items are
   added after a victory only.
5. **Saving** — `SaveGame` (versioned JSON) holds the campaign and an optional battle in progress.
   Natively saves live in the user data directory; on the web in `localStorage` (via `web/hero_web.js`).

## Layered packs

A pack can be built on another one: `extends = "../base"` in `pack.toml` makes the pack in that
directory its parent, and the chain of packs (at most four) is loaded as one `Pack`. The child's
rules files, officer list and campaign replace the parent's when it lists them; battles and drama
scenes are the union of all packs, a nearer pack overriding ids of a farther one; media files are
looked up in the top pack first, then in each parent; `[presentation]` (the canvas size) is inherited
field by field from the nearest pack that sets each field. Paths are resolved lexically relative to the top pack
(`../base/rules/game.toml`), so the same `FileSource` keys work for a directory (`DirSource`) and for
the web build's fetched file map. The exact rules are in [MODDING.md](MODDING.md#layered-packs-extends)
and the reasoning in [DECISIONS.md](DECISIONS.md) (D8).

### Original mode (partly implemented)

The long-term goal, like OpenRCT2 with RCT2's data, is an **original mode** that shows the game with
the original's own assets, read from the player's legally owned copy. It is a layered pack whose
`pack.toml` says `extends = "../base"` and `[presentation] canvas = [640, 480]` (the original's VGA
screen) and which ships only what has been converted from the original. Everything not converted yet
keeps coming from the base pack through the chain, so the mode can grow asset by asset:

```
data/original/pack.toml    extends = "../base", canvas 640x480, lists what was converted
        │  everything it does not provide (rules, maps, scenario, media) comes from
        ▼
data/base/pack.toml        the complete, license-clean base pack
```

The player picks the install folder in the game (title → 원작 데이터, `hero-game`
`screens/original.rs`); the folder is kept in the settings. At every launch the loading screen loads
the base pack, converts the install **in memory** on a worker thread (`hero_import::pack::build_pack`,
`hero-game` `original.rs`) and mounts the files at `<data>/original` (`platform/memfs.rs`): every file
read (`assets.rs` `fetch`) inside that directory is served from
memory, paths outside it are read from disk with `..` resolved lexically, so the chain loads exactly
as it would from disk. Nothing is written; `hero-import` is a native-only dependency of the game
(D10). `hero-tools original pack` writes the same pack to `data/original/` (git-ignored) for
inspection and validation.

Today the pack holds officer portraits, unit sheets, a 32-px terrain tileset learned from the
original battle maps, the 58 original battle maps as map files, the original's campaign from the
prologue to its endings (its battles with their mid-battle events and dialogue, its story as drama
scenes), the officers and the rules (mapping rules in `crates/hero-import/src/pack.rs`, `battles.rs`,
`chapters.rs` and ORIGINAL_DATA.md §4.5); what is not converted (fonts, sounds, some UI) comes from the
base pack, which is the engine's minimal frame (DECISIONS D21). The separate media overlay (`--original <dir>`, D6) remains for looking
at the raw extraction.

## Determinism and testing

All randomness goes through the battle's serialized `Rng`, so a battle replays identically from a
save, and `hero-tools simulate` can run every battle of the campaign AI-vs-AI across many seeds in CI
to catch unwinnable maps, stuck AI and panics.

## Rendering model

The game renders to a **virtual canvas** whose size comes from the data pack (the base pack: 480×270 with
16×16 px tiles; the tile size comes from the tileset, see [ASSETS.md](ASSETS.md#presentation-profile)) and scales
it to the window with integer factors when possible (nearest-neighbour, letterboxed). `pack.toml`
declares the canvas size its media is laid out for in `[presentation] canvas = [w, h]` (480×270 to
1280×800, default 480×270); `Pack::manifest.presentation` holds the effective value, which a layered
pack may inherit. All UI
coordinates are virtual pixels. Text uses the Galmuri pixel fonts (OFL), which cover Hangul, Latin
and the Hanja used in officer names.

See also: [RULES.md](RULES.md) (combat formulas), [MODDING.md](MODDING.md) (data formats),
[ASSETS.md](ASSETS.md) (media conventions), [DECISIONS.md](DECISIONS.md),
[ORIGINAL_DATA.md](ORIGINAL_DATA.md) (original-data importer) and
[reverse-engineering/](reverse-engineering/README.md) (original file formats, method and status).
