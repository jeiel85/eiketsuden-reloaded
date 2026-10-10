# Trial art pack

A first try at the BACKLOG item "교체 아트 팩 레이어": new art drawn by code, laid over the original
mode as a [layered pack](../../docs/MODDING.md#layered-packs-extends). No engine change is needed:
a child pack's media files win over the original pack's by file name.

What it replaces:

* **Battle-map pictures** (`gfx/maps/hexz_*.png`, all 58 maps and the 15 cell pictures that battle
  scripts swap in when a gate opens or a drawbridge comes down). `render_maps.py` draws each one
  from the map's terrain grid in `<original pack>/maps/original.toml`: soft ground edges with noise,
  cliff faces, castle walls and paving, and y-sorted trees, peaks and buildings. The original's
  16-px chips are not used, so the new pictures follow the cells (32 px), not the original's chip
  details.
* **Every unit sheet** (`gfx/units/<sprite>_<side>.png`, the 25 sprite keys of the original pack ×
  side colours: 19 classes, Liu Bei's three class icons, Cao Cao, Lü Bu and the confusion icon).
  `units.py` builds them from parts (head, body, legs, weapon, offhand, horse, wheels, banner) so
  they share one style; two frames each like the original's map icons, laid out the way
  hero-import's `unit_sheet` lays out the original's. The build stops if the original pack has a
  sheet no drawing covers.

Everything else (faces, scenes, duels, music, rules, stories) still comes from the original pack.

## Building

Needs Python 3.12+, Pillow and numpy, and an original pack made by `hero-tools original pack`.

```sh
python tools/artpack/build_pack.py data/original res/art-trial
target/release/hero-tools validate res/art-trial
target/release/eiketsuden --data res/art-trial
```

The pack gets its own id (`art_trial`), so it has its own save slots.

## Licence and sharing

The drawing code and the sprites it draws are made for this project and dedicated to the public
domain (CC0), like `tools/assets/art.py`. The generated map pictures follow the original's map
layouts, so they are derived from the player's copy: keep the output folder on your computer like
the original pack (`res/` is ignored by git), and do not commit or share it.
