"""Draw the sprite atlases the converter builds the original mode's new art from.

    python tools/artpack/build.py           # write crates/hero-import/assets/remake/
    python tools/artpack/build.py --check   # draw into memory and compare with the committed files

`objects.png` holds the map objects (trees, peaks, buildings, the gate) and `units.png` the two
frames of every unit sprite per side colour; each comes with a `.txt` index of
`name x y w h` lines that `crates/hero-import/src/remake.rs` cuts the atlas by. Everything here is
drawn by this project's code and dedicated to the public domain (CC0). See README.md.
"""

from __future__ import annotations

import argparse
import io
import random
import sys
from pathlib import Path

from PIL import Image

import sprites
import units

OUT = Path(__file__).resolve().parents[2] / "crates" / "hero-import" / "assets" / "remake"
SLOT = 32
SIDES = ("player", "ally", "enemy")


def object_sprites() -> list[tuple[str, Image.Image]]:
    r = random.Random(1)
    out = [
        ("tree0", sprites.tree(r, 0)),
        ("tree1", sprites.tree(r, 1)),
        ("broadleaf", sprites.broadleaf(r)),
    ]
    out += [(f"peak_big{i}", sprites.peak(random.Random(10 + i), True)) for i in range(3)]
    out += [(f"peak_small{i}", sprites.peak(random.Random(20 + i), False)) for i in range(3)]
    out += [(f"house_{roof}", sprites.house(r, roof)) for roof in ("blue", "red", "brown")]
    out += [
        ("field", sprites.field_patch(r)),
        ("tent", sprites.tent(r)),
        ("gatehouse", sprites.gatehouse(r)),
        ("granary", sprites.storehouse(r, False)),
        ("treasury", sprites.storehouse(r, True)),
        ("gate", sprites.gate()),
    ]
    return out


def unit_frames() -> list[tuple[str, Image.Image]]:
    return [
        (f"{key}_{side}_{frame}", draw(side, frame))
        for key, draw in units.SPRITES.items()
        for side in SIDES
        for frame in (0, 1)
    ]


def atlas(items: list[tuple[str, Image.Image]], columns: int, header: str) -> tuple[Image.Image, str]:
    """Pack sprites into 32-px slots, `columns` per row; the index gives each one's real size."""
    rows = (len(items) + columns - 1) // columns
    img = Image.new("RGBA", (columns * SLOT, rows * SLOT), (0, 0, 0, 0))
    lines = [header]
    for i, (name, sprite) in enumerate(items):
        if sprite.width > SLOT or sprite.height > SLOT:
            sys.exit(f"{name}: {sprite.size} does not fit a {SLOT}-px slot")
        x, y = (i % columns) * SLOT, (i // columns) * SLOT
        img.alpha_composite(sprite, (x, y))
        lines.append(f"{name} {x} {y} {sprite.width} {sprite.height}")
    return img, "\n".join(lines) + "\n"


HEADER = "# name x y w h — written by tools/artpack/build.py (CC0); do not edit"


def outputs() -> dict[str, tuple[Image.Image, str]]:
    return {
        "objects": atlas(object_sprites(), 8, HEADER),
        "units": atlas(unit_frames(), 6, HEADER),
    }


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--check", action="store_true", help="compare with the committed files, write nothing")
    args = ap.parse_args()
    stale = []
    for name, (img, index) in outputs().items():
        png, txt = OUT / f"{name}.png", OUT / f"{name}.txt"
        if args.check:
            # Pixels, not bytes: PNG compression differs between Pillow builds.
            same_txt = txt.is_file() and txt.read_text(encoding="utf-8") == index
            same_png = png.is_file() and Image.open(png).convert("RGBA").tobytes() == img.tobytes()
            if not (same_txt and same_png):
                stale.append(name)
            continue
        OUT.mkdir(parents=True, exist_ok=True)
        buf = io.BytesIO()
        img.save(buf, "PNG", optimize=True)
        png.write_bytes(buf.getvalue())
        txt.write_text(index, encoding="utf-8", newline="\n")
        print(png, img.size)
    if stale:
        sys.exit(f"stale atlases (run python tools/artpack/build.py): {', '.join(stale)}")


if __name__ == "__main__":
    main()
