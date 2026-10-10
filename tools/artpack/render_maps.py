"""Redraw the original mode's battle-map pictures from their terrain grids with new art.

The original's pictures are composed of KOEI's 16-px chips. This draws a picture of the same size
for every map in `<original pack>/maps/original.toml` from the map's rules grid alone (one terrain
per 32-px cell): soft ground boundaries with noise, cliff faces, castle walls, and y-sorted trees,
peaks and buildings from tools/artpack/sprites.py. The map layout is the original's, so the
output is derived from the player's copy: keep it local like the original pack.
"""

from __future__ import annotations

import random
import tomllib
from pathlib import Path

import numpy as np
import sprites
from PIL import Image, ImageDraw

CELL = 32

# Ground ramps, dark → light (4 shades). Object cells take the ground of their neighbours.
RAMPS = {
    "plain": ["#5f9a32", "#74b23c", "#86c246", "#a2d65c"],
    "grass": ["#3c7426", "#4a8a2e", "#589c36", "#6eb244"],
    "forest": ["#2e5e22", "#386e28", "#44802e", "#549438"],
    "wasteland": ["#8e6c3e", "#a4804a", "#b89458", "#ceac6c"],
    "cliff": ["#7e6038", "#937244", "#a48450", "#b89a62"],
    "mountain": ["#6e6248", "#807256", "#928466", "#a89a7a"],
    "castle": ["#6e6858", "#7e7866", "#8c8674", "#9e9886"],
    "river": ["#2c5cae", "#3a70c4", "#4c86d6", "#7cb0ec"],
}
GROUNDS = list(RAMPS)
# Blur radius (px) of each ground's mask: big = organic edges, 0 = cell-aligned.
SOFTNESS = {
    "plain": 14,
    "grass": 14,
    "forest": 10,
    "wasteland": 12,
    "cliff": 5,
    "mountain": 10,
    "castle": 0,
    "river": 7,
}
OBJECTS = {"house", "village", "fort", "barracks", "granary", "treasury", "bridge", "fence", "wall", "closed_gate"}
WALLISH = {"wall", "closed_gate"}

BAYER = (np.array([[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]]) + 0.5) / 16.0


def hex_rgb(h: str) -> np.ndarray:
    h = h.lstrip("#")
    return np.array([int(h[i : i + 2], 16) for i in (0, 2, 4)], dtype=np.float32)


def box_blur(a: np.ndarray, r: int) -> np.ndarray:
    if r <= 0:
        return a
    for axis in (0, 1):
        pad = [(0, 0), (0, 0)]
        pad[axis] = (r, r)
        p = np.pad(a, pad, mode="edge")
        c = np.cumsum(p, axis=axis, dtype=np.float64)
        c = np.insert(c, 0, 0, axis=axis)
        n = a.shape[axis]
        hi = np.take(c, np.arange(2 * r + 1, 2 * r + 1 + n), axis=axis)
        lo = np.take(c, np.arange(0, n), axis=axis)
        a = ((hi - lo) / (2 * r + 1)).astype(np.float32)
    return a


def value_noise(h: int, w: int, scale: int, rng: np.random.Generator) -> np.ndarray:
    gh, gw = h // scale + 2, w // scale + 2
    g = rng.random((gh, gw)).astype(np.float32)
    ys = np.arange(h) / scale
    xs = np.arange(w) / scale
    y0 = ys.astype(int)
    x0 = xs.astype(int)
    fy = (ys - y0)[:, None]
    fx = (xs - x0)[None, :]
    fy = fy * fy * (3 - 2 * fy)
    fx = fx * fx * (3 - 2 * fx)
    a = g[y0][:, x0]
    b = g[y0][:, x0 + 1]
    c = g[y0 + 1][:, x0]
    d = g[y0 + 1][:, x0 + 1]
    return (a * (1 - fx) + b * fx) * (1 - fy) + (c * (1 - fx) + d * fx) * fy


def ground_of_objects(grid: list[list[str]]) -> list[list[str]]:
    """Object cells get the most common ground among their neighbours (castle near walls)."""
    h, w = len(grid), len(grid[0])
    out = [row[:] for row in grid]
    for y in range(h):
        for x in range(w):
            t = grid[y][x]
            if t in GROUNDS:
                continue
            if t == "bridge":
                out[y][x] = "river"
                continue
            # Look in a growing square until some ground shows up; inside a town (any castle
            # floor within two cells) buildings and walls stand on the paving.
            counts: dict[str, int] = {}
            for reach in (1, 2, 3):
                for dy in range(-reach, reach + 1):
                    for dx in range(-reach, reach + 1):
                        ny, nx = y + dy, x + dx
                        if 0 <= ny < h and 0 <= nx < w and grid[ny][nx] in GROUNDS and grid[ny][nx] != "river":
                            counts[grid[ny][nx]] = counts.get(grid[ny][nx], 0) + 1
                if counts and (reach >= 2 or t not in ("house", "treasury", "granary", "fort")):
                    break
            if t in WALLISH or (counts.get("castle") and t != "village"):
                out[y][x] = "castle"
            else:
                out[y][x] = max(counts, key=counts.get) if counts else "plain"
    return out


def render(grid: list[list[str]], seed: int) -> Image.Image:
    h, w = len(grid), len(grid[0])
    H, W = h * CELL, w * CELL
    rng = np.random.default_rng(seed)
    prng = random.Random(seed)
    ground = ground_of_objects(grid)
    cells = np.array([[GROUNDS.index(t) for t in row] for row in ground])

    # ---- ground classes: blurred masks + noise, argmax
    n1 = value_noise(H, W, 9, rng)
    n2 = value_noise(H, W, 23, rng)
    noise = (n1 * 0.6 + n2 * 0.4) - 0.5
    scores = []
    for gi, g in enumerate(GROUNDS):
        cell_mask = (cells == gi).astype(np.float32)
        r = SOFTNESS[g]
        if r:
            # Interpolate between cell centres so diagonal runs of cells give diagonal edges,
            # then soften a little more.
            m = np.asarray(Image.fromarray(cell_mask, "F").resize((W, H), Image.BILINEAR))
            m = box_blur(m, r // 3 + 1)
        else:
            m = np.kron(cell_mask, np.ones((CELL, CELL), np.float32))
        jitter = 0.35 if r >= 10 else (0.2 if r else 0.0)
        scores.append(m + noise * jitter * (m > 0.02))
    cls = np.argmax(np.stack(scores), axis=0)

    # ---- texture: per-class ramp, shade from noise + ordered dither
    fine = value_noise(H, W, 3, rng)
    shade_f = 0.55 * fine + 0.45 * (n1) + (BAYER[np.arange(H) % 4][:, np.arange(W) % 4] - 0.5) * 0.35
    shade = np.clip((shade_f * 4).astype(int), 0, 3)
    img = np.zeros((H, W, 3), np.float32)
    ramps = {g: np.stack([hex_rgb(c) for c in RAMPS[g]]) for g in GROUNDS}
    for gi, g in enumerate(GROUNDS):
        sel = cls == gi
        img[sel] = ramps[g][shade[sel]]

    river = cls == GROUNDS.index("river")
    cliff = cls == GROUNDS.index("cliff")
    castle = cls == GROUNDS.index("castle")
    yy, xx = np.mgrid[0:H, 0:W]

    # castle paving: 8×8 stones with offset rows, mortar lines
    mortar = ((yy % 8) == 0) | (((xx + (yy // 8) % 2 * 4) % 8) == 0)
    img[castle & mortar] = hex_rgb("#5c5648")

    # water: ripples and a dark bank + sandy shore
    ripple = ((np.sin(xx * 0.45 + n2 * 9) + np.sin(yy * 0.9 + n1 * 7)) > 1.55) & river
    img[ripple] = hex_rgb("#a8d0f4")
    land = ~river
    near_land = box_blur(land.astype(np.float32), 2) > 0.01
    img[river & near_land] = hex_rgb("#24488a")
    near_water = box_blur(river.astype(np.float32), 2) > 0.01
    img[land & near_water] = hex_rgb("#c8b47a")

    # boundaries between the soft greens: a darker rim on the darker side
    for g in ("grass", "forest", "wasteland"):
        gm = cls == GROUNDS.index(g)
        edge = gm & (box_blur((~gm).astype(np.float32), 1) > 0.01)
        img[edge] = img[edge] * 0.85

    # cliff: the plateau's lowest 14–22 px become a rock face, rim highlight on top
    depth = np.full(cliff.shape, 99, np.int32)  # rows to the open ground below
    for k in range(22, 0, -1):
        shifted = np.zeros_like(cliff)
        shifted[:-k] = ~cliff[k:]
        depth[shifted] = k
    # face height wobbles between 14 and 22 px along the edge
    height = (14 + (value_noise(1, W, 11, rng)[0] * 9)).astype(np.int32)[None, :]
    face = cliff & (depth <= height)
    # rock columns: lit left edges, dark right edges, darker towards the foot
    col_noise = (value_noise(H, W, 5, rng) * 4).astype(int)
    colx = (xx + col_noise) % 7
    img[face] = hex_rgb("#6a5030")
    img[face & (colx < 2)] = hex_rgb("#8e7046")
    img[face & (colx == 6)] = hex_rgb("#3e2c18")
    low = face & (depth <= 4)
    img[low] = img[low] * 0.7
    top_edge = cliff & ~face & (box_blur((~cliff).astype(np.float32), 1) > 0.01)
    img[top_edge] = hex_rgb("#cfb07a")
    foot = np.zeros_like(cliff)
    foot[1:] = face[:-1] & ~cliff[1:]
    img[foot] = img[foot] * 0.6

    canvas = Image.fromarray(img.clip(0, 255).astype(np.uint8), "RGB").convert("RGBA")

    # ---- walls (cell-aligned): top surface + front face where the cell below is not wall
    wallset = {(x, y) for y in range(h) for x in range(w) if grid[y][x] in WALLISH}
    wall_img = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    wpx = wall_img.load()
    for x, y in wallset:
        open_below = (x, y + 1) not in wallset and y + 1 < h
        open_left = (x - 1, y) not in wallset
        open_right = (x + 1, y) not in wallset
        open_above = (x, y - 1) not in wallset
        for py in range(CELL):
            for px in range(CELL):
                X, Y = x * CELL + px, y * CELL + py
                if open_below and py >= 18:
                    # brick face
                    brick = (py - 18) % 5 == 0 or ((px + ((py - 18) // 5) % 2 * 4) % 8 == 0)
                    col = (110, 96, 78) if brick else (150, 132, 104)
                    if py == 18:
                        col = (70, 60, 48)
                else:
                    stone = (px + py) % 9 == 0
                    col = (186, 176, 150) if not stone else (160, 150, 126)
                    if open_above and py < 3:
                        col = (214, 206, 182) if (px // 4) % 2 == 0 else (120, 110, 92)
                if (open_left and px == 0) or (open_right and px == CELL - 1):
                    col = (60, 52, 42)
                wpx[X, Y] = (*col, 255)
        if grid[y][x] == "closed_gate":
            gd = Image.new("RGBA", (20, 20), (0, 0, 0, 0))
            dd = ImageDraw.Draw(gd)
            dd.rectangle((1, 2, 18, 19), fill=(90, 56, 30, 255))
            dd.line([(10, 2), (10, 19)], fill=(50, 30, 16, 255))
            for yy2 in (6, 12, 17):
                dd.line([(1, yy2), (18, yy2)], fill=(60, 40, 22, 255))
            dd.point((8, 11), fill=(224, 184, 72, 255))
            dd.point((12, 11), fill=(224, 184, 72, 255))
            wall_img.alpha_composite(sprites.outlined(gd), (x * CELL + 6, y * CELL + 11))
    # a soft shadow cast to the lower right of walls
    shadow = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    sp = shadow.load()
    for x, y in wallset:
        for py in range(4):
            for px in range(CELL):
                Y = (y + 1) * CELL + py
                if y + 1 < h and (x, y + 1) not in wallset:
                    sp[x * CELL + px, Y] = (0, 0, 0, 70 - py * 15)
    canvas.alpha_composite(shadow)
    canvas.alpha_composite(wall_img)

    # ---- fences and bridges (flat, under standing objects)
    dr = ImageDraw.Draw(canvas)
    for y in range(h):
        for x in range(w):
            t = grid[y][x]
            X, Y = x * CELL, y * CELL
            if t == "bridge":
                horiz = (x > 0 and grid[y][x - 1] != "river") or (x + 1 < w and grid[y][x + 1] != "river")
                vert = (y > 0 and grid[y - 1][x] != "river") or (y + 1 < h and grid[y + 1][x] != "river")
                if vert and not horiz:
                    dr.rectangle((X + 5, Y - 2, X + 26, Y + 33), fill=(120, 84, 48))
                    for k in range(Y - 2, Y + 34, 4):
                        dr.line([(X + 5, k), (X + 26, k)], fill=(84, 56, 30))
                    dr.line([(X + 5, Y - 2), (X + 5, Y + 33)], fill=(60, 40, 22), width=2)
                    dr.line([(X + 26, Y - 2), (X + 26, Y + 33)], fill=(60, 40, 22), width=2)
                else:
                    dr.rectangle((X - 2, Y + 5, X + 33, Y + 26), fill=(120, 84, 48))
                    for k in range(X - 2, X + 34, 4):
                        dr.line([(k, Y + 5), (k, Y + 26)], fill=(84, 56, 30))
                    dr.line([(X - 2, Y + 5), (X + 33, Y + 5)], fill=(60, 40, 22), width=2)
                    dr.line([(X - 2, Y + 26), (X + 33, Y + 26)], fill=(60, 40, 22), width=2)
            elif t == "fence":
                joins = ("fence", "wall", "closed_gate")
                links = [
                    (dx, dy)
                    for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1))
                    if 0 <= x + dx < w and 0 <= y + dy < h and grid[y + dy][x + dx] in joins
                ]
                if not links:
                    links = [(1, 0), (-1, 0)]
                cx, cy = X + 16, Y + 18
                for dx, dy in links:
                    ex, ey = cx + dx * 16, cy + dy * 16
                    dr.line([(cx, cy - 3), (ex, ey - 3)], fill=(150, 106, 60), width=1)
                    dr.line([(cx, cy), (ex, ey)], fill=(110, 76, 40), width=1)
                for px, py in [(cx, cy)] + [(cx + dx * 8, cy + dy * 8) for dx, dy in links]:
                    dr.rectangle((px - 1, py - 7, px, py + 1), fill=(84, 56, 30))
                    dr.point((px - 1, py - 7), fill=(170, 130, 80))

    # ---- standing objects, sorted by their foot y
    standing: list[tuple[int, int, Image.Image]] = []

    def put(img: Image.Image, fx: int, fy: int) -> None:
        standing.append((fy, fx, img))

    trees = [sprites.tree(prng, k) for k in range(2)]
    leaf = sprites.broadleaf(prng)
    for y in range(h):
        for x in range(w):
            t = grid[y][x]
            X, Y = x * CELL, y * CELL
            r = random.Random(seed * 7919 + y * 131 + x)
            if t == "forest":
                for sy in (4, 15, 26):
                    for sx in (5, 16, 27):
                        if r.random() < 0.82:
                            img = leaf if r.random() < 0.12 else trees[r.randrange(2)]
                            put(img, X + sx + r.randint(-3, 3), Y + sy + r.randint(-2, 3))
            elif t == "mountain":
                put(sprites.peak(r, True), X + 16 + r.randint(-3, 3), Y + 24 + r.randint(-2, 2))
                if r.random() < 0.6:
                    put(sprites.peak(r, False), X + 6 + r.randint(0, 20), Y + 30)
            elif t == "house":
                roof = "blue" if r.random() < 0.35 else ("red" if r.random() < 0.5 else "brown")
                put(sprites.house(r, roof), X + 16, Y + 26)
            elif t == "village":
                put(sprites.field_patch(r), X + 9, Y + 30)
                put(sprites.house(r, "brown"), X + 13, Y + 18)
                put(sprites.house(r, "red"), X + 21, Y + 30)
            elif t == "barracks":
                put(sprites.tent(r), X + 16, Y + 27)
            elif t == "fort":
                put(sprites.gatehouse(r), X + 16, Y + 29)
            elif t == "granary":
                put(sprites.storehouse(r, False), X + 16, Y + 27)
            elif t == "treasury":
                put(sprites.storehouse(r, True), X + 16, Y + 27)
    standing.sort(key=lambda s: (s[0], s[1]))
    for fy, fx, img in standing:
        w0, h0 = img.size
        sh = Image.new("RGBA", (w0, 4), (0, 0, 0, 0))
        ImageDraw.Draw(sh).ellipse((1, 0, w0 - 2, 3), fill=(0, 0, 0, 60))
        canvas.alpha_composite(sh, (fx - w0 // 2, fy - 2))
        canvas.alpha_composite(img, (fx - w0 // 2, fy - h0))
    return canvas


def main() -> None:
    import argparse

    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("original", type=Path, help="the original pack (hero-tools original pack --out …)")
    ap.add_argument("out", type=Path, help="the art pack directory")
    ap.add_argument("--only", nargs="*", help="map ids to draw (default: all)")
    args = ap.parse_args()
    maps = tomllib.loads((args.original / "maps" / "original.toml").read_text(encoding="utf-8"))["map"]
    out = args.out / "gfx" / "maps"
    out.mkdir(parents=True, exist_ok=True)
    for i, m in enumerate(maps):
        if args.only and m["id"] not in args.only:
            continue
        rows = m["rows"].strip("\n").split("\n")
        grid = [[m["legend"][ch] for ch in row] for row in rows]
        pic = render(grid, seed=1000 + i)
        ref = args.original / "gfx" / "maps" / f"{m['image']}.png"
        if ref.is_file():
            with Image.open(ref) as r:
                if r.size != pic.size:
                    raise SystemExit(f"{m['id']}: drew {pic.size}, original picture is {r.size}")
        pic.convert("RGB").save(out / f"{m['image']}.png", optimize=True)
        print(m["id"], pic.size)
        draw_patches(args.original, out, m, grid, seed=1000 + i)


def draw_patches(original: Path, out: Path, m: dict, grid: list[list[str]], seed: int) -> None:
    """Redraw the 32-px cell pictures a battle script swaps in (`<image>_<x>_<y>_<action>.png`).

    The actions are the original's map-cell changes (FORMATS §13.5): 0 opens a gate (the cell
    becomes ground), 1 closes it, 2 lowers a drawbridge (only the middle river cell of the group
    becomes a bridge; the others are cropped from the same picture so the seams match).
    """
    groups: dict[int, list[tuple[int, int, Path]]] = {}
    for p in sorted((original / "gfx" / "maps").glob(f"{m['image']}_*_*_*.png")):
        x, y, action = (int(v) for v in p.stem[len(m["image"]) + 1 :].split("_"))
        groups.setdefault(action, []).append((x, y, p))
    for action, cells in groups.items():
        changed = [row[:] for row in grid]
        if action == 0:
            for x, y, _ in cells:
                changed[y][x] = "plain"
        elif action == 1:
            for x, y, _ in cells:
                changed[y][x] = "closed_gate"
        elif action == 2:
            river = sorted((x, y) for x, y, _ in cells if grid[y][x] == "river")
            if river:
                x, y = river[len(river) // 2]
                changed[y][x] = "bridge"
        else:
            raise SystemExit(f"{m['id']}: map-cell action {action} has no drawing")
        pic = render(changed, seed)
        for x, y, p in cells:
            pic.crop((x * CELL, y * CELL, (x + 1) * CELL, (y + 1) * CELL)).convert("RGB").save(out / p.name)
            print(" ", p.stem)


if __name__ == "__main__":
    main()
