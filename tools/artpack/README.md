# New art of the original mode

Sprites for the original mode's new art (`docs/DECISIONS.md` D27, the setting "그림: 새 그림"). The
converter (`crates/hero-import/src/remake.rs`) draws every battle map from its terrain grid with the map
objects here and writes the unit sheets from the unit frames here, next to the original's pictures in
the converted pack (`gfx/remake/`). The game shows them when the setting asks for them.

* `units.py` builds all 25 unit sprite keys of the original mode (19 classes, Liu Bei's three class
  icons, Cao Cao, Lü Bu and the confusion icon) from parts (head, body, legs, weapon, offhand, horse,
  wheels, banner) so they share one style; two 32×32 frames each, like the original's map icons, in
  the three side colours.
* `sprites.py` draws the map objects: trees, peaks, houses, tent, gatehouse, granary, treasury, field
  and the closed gate.
* `build.py` packs them into `crates/hero-import/assets/remake/{objects,units}.png` with `.txt`
  indexes (`name x y w h`) that the converter cuts the atlases by.

## Building

Needs Python 3.12+ and Pillow.

```sh
python tools/artpack/build.py           # redraw the atlases after changing a sprite
python tools/artpack/build.py --check   # what CI runs: the committed atlases match the code
```

`--check` compares pixels, not PNG bytes (Pillow builds compress differently). After redrawing, run the
converter again (the game does at every launch; a pack written with `hero-tools original pack` needs a
new run) to see the change.

## Licence

The drawing code and the sprites are made for this project and dedicated to the public domain (CC0),
like `tools/assets/art.py`. The map pictures the converter draws with them follow the original's map
layouts, so they are made from the player's copy at conversion time and stay on their computer.
