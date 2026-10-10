"""Assemble the trial art pack: a child of the original pack that replaces its battle-map pictures
and all its unit sheets with art made for this project.

    python tools/artpack/build_pack.py <original pack> <out dir>

`<out dir>` gets `pack.toml` with `extends` pointing at the original pack, so run the game with
`--data <out dir>`. Everything else (faces, other units, scenes, music, rules) still comes from
the original pack. See tools/artpack/README.md.
"""

from __future__ import annotations

import argparse
import os
import sys
from pathlib import Path

import render_maps
import units
from PIL import Image


def unit_sheet(draw, side: str) -> Image.Image:
    """4 columns (down, up, left, right) × 6 rows like the original mode's sheets
    (hero-import `unit_sheet`): the drawn facing in the down and right columns, mirrored in the
    other two; rows alternate the two frames (walk f0 f1 f0 f1, attack f0, hurt f1)."""
    frames = [draw(side, 0), draw(side, 1)]
    sheet = Image.new("RGBA", (128, 192), (0, 0, 0, 0))
    for row in range(6):
        f = frames[row % 2]
        for col in range(4):
            img = f.transpose(Image.Transpose.FLIP_LEFT_RIGHT) if col in (1, 2) else f
            sheet.alpha_composite(img, (col * 32, row * 32))
    return sheet


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("original", type=Path)
    ap.add_argument("out", type=Path)
    ap.add_argument("--skip-maps", action="store_true")
    args = ap.parse_args()
    if not (args.original / "pack.toml").is_file():
        sys.exit(f"{args.original}: no pack.toml (make it with `hero-tools original pack`)")
    args.out.mkdir(parents=True, exist_ok=True)
    extends = Path(os.path.relpath(args.original.resolve(), args.out.resolve())).as_posix()
    (args.out / "pack.toml").write_text(
        f"""# Trial art pack written by tools/artpack/build_pack.py: new battle-map pictures and unit
# sheets over the original pack. The map pictures follow the original's map layouts, so
# keep this folder on this computer like the original pack.

id = "art_trial"
name = "영걸전 원작 모드 (새 그림 시험)"
version = "0.0.1"
license = "LicenseRef-Private (art CC0; map layouts from the player's own copy)"
description = "원작 모드 위에 새로 그린 전투 맵 그림과 유닛을 얹은 시험 팩."
extends = "{extends}"
""",
        encoding="utf-8",
    )
    # One sheet for every sheet the original pack has (sprite key × side), so nothing of the
    # original's units shows through.
    out_units = args.out / "gfx" / "units"
    out_units.mkdir(parents=True, exist_ok=True)
    sheets = sorted(p.stem for p in (args.original / "gfx" / "units").glob("*.png"))
    missing = []
    for stem in sheets:
        key, side = stem.rsplit("_", 1)
        if key not in units.SPRITES:
            missing.append(stem)
            continue
        unit_sheet(units.SPRITES[key], side).save(out_units / f"{stem}.png")
    if missing:
        sys.exit(f"no drawing for the original pack's unit sheets: {', '.join(missing)}")
    if not args.skip_maps:
        sys.argv = [sys.argv[0], str(args.original), str(args.out)]
        render_maps.main()


if __name__ == "__main__":
    main()
