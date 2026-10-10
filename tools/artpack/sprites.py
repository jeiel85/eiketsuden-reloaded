"""Small pixel sprites for the trial art pack, drawn with primitives and finished by an outline pass.

Everything here is made for this project (CC0, see tools/artpack/README.md). Light falls from the
top left, like the original's art.
"""

from __future__ import annotations

import random

from PIL import Image, ImageDraw

OUTLINE = (24, 20, 16, 255)
SHADOW = (0, 0, 0, 90)


def rgba(hex_colour: str, alpha: int = 255) -> tuple[int, int, int, int]:
    h = hex_colour.lstrip("#")
    return (int(h[0:2], 16), int(h[2:4], 16), int(h[4:6], 16), alpha)


def outlined(img: Image.Image, colour=OUTLINE) -> Image.Image:
    """Add a one-pixel outline (4-neighbourhood) around every opaque pixel."""
    w, h = img.size
    src = img.load()
    out = img.copy()
    dst = out.load()
    for y in range(h):
        for x in range(w):
            if src[x, y][3]:
                continue
            for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)):
                nx, ny = x + dx, y + dy
                if 0 <= nx < w and 0 <= ny < h and src[nx, ny][3] > 128:
                    dst[x, y] = colour
                    break
    return out


def blank(w: int, h: int) -> tuple[Image.Image, ImageDraw.ImageDraw]:
    img = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    return img, ImageDraw.Draw(img)


# ---------------------------------------------------------------- map objects


def tree(rng: random.Random, kind: int = 0) -> Image.Image:
    """A conifer seen from above at an angle: stacked canopy tiers, ~14×18."""
    greens = [
        ("#173d1c", "#25602a", "#3a8a36", "#6cbf4a"),
        ("#163a24", "#235a34", "#347f45", "#5fae5a"),
    ][kind % 2]
    img, d = blank(16, 20)
    d.rectangle((7, 15, 8, 18), fill=rgba("#4a3020"))
    for top, half in ((1, 3), (5, 5), (9, 6)):
        y0 = top
        y1 = top + 7
        d.polygon([(8, y0), (8 - half - 1, y1), (8 + half, y1)], fill=rgba(greens[1]))
        d.polygon([(8, y0), (8 - half - 1, y1), (7, y1)], fill=rgba(greens[2]))
        d.line([(8, y0 + 1), (8 - half + 1, y1 - 1)], fill=rgba(greens[3]))
        d.line([(9, y0 + 2), (8 + half - 1, y1)], fill=rgba(greens[0]))
    return outlined(img, rgba(greens[0]))


def broadleaf(rng: random.Random) -> Image.Image:
    img, d = blank(18, 18)
    d.rectangle((8, 12, 9, 16), fill=rgba("#4a3020"))
    d.ellipse((1, 1, 16, 13), fill=rgba("#2f6d2c"))
    d.ellipse((2, 2, 11, 9), fill=rgba("#4c9a3a"))
    d.ellipse((3, 3, 7, 6), fill=rgba("#7cc457"))
    d.arc((1, 1, 16, 13), 20, 160, fill=rgba("#1d4a1e"))
    return outlined(img, rgba("#173d1c"))


def peak(rng: random.Random, big: bool) -> Image.Image:
    """A rocky mountain top: lit left face, shaded right face, a little snow on big ones."""
    w, h = (26, 22) if big else (18, 15)
    img, d = blank(w, h)
    apex = (w // 2 + rng.randint(-2, 1), 1)
    base_l, base_r = (1, h - 2), (w - 2, h - 2)
    mid = (apex[0] + rng.randint(-1, 2), h - 2)
    d.polygon([apex, base_l, mid], fill=rgba("#9a8a6c"))
    d.polygon([apex, mid, base_r], fill=rgba("#5e5442"))
    # ridges
    d.line([apex, (apex[0] - w // 5, h // 2)], fill=rgba("#b9a986"))
    d.line([(apex[0] + 2, h // 3), (apex[0] + w // 4, h - 4)], fill=rgba("#4a4234"))
    if big:
        d.polygon([apex, (apex[0] - 4, 6), (apex[0] + 1, 5), (apex[0] + 4, 7)], fill=rgba("#e8e6dc"))
    return outlined(img, rgba("#2c261c"))


def house(rng: random.Random, roof: str) -> Image.Image:
    """A small house, 22×20: walls, door and a pitched tiled roof."""
    roofs = {
        "red": ("#7a2a20", "#a8402c", "#cf6a4a"),
        "blue": ("#24406e", "#335c9a", "#5a86c4"),
        "brown": ("#5a3c22", "#7e5630", "#a87a46"),
    }[roof]
    img, d = blank(22, 20)
    d.rectangle((3, 10, 18, 18), fill=rgba("#d9c9a3"))
    d.rectangle((3, 10, 5, 18), fill=rgba("#efe2c0"))
    d.rectangle((15, 10, 18, 18), fill=rgba("#b8a47c"))
    d.rectangle((9, 13, 12, 18), fill=rgba("#5a3a22"))
    d.rectangle((6, 12, 7, 13), fill=rgba("#3a2a1c"))
    d.rectangle((14, 12, 15, 13), fill=rgba("#3a2a1c"))
    d.polygon([(0, 11), (4, 2), (17, 2), (21, 11)], fill=rgba(roofs[1]))
    d.polygon([(0, 11), (4, 2), (8, 2), (5, 11)], fill=rgba(roofs[2]))
    for x in range(6, 20, 3):
        d.line([(x, 3), (x - 1, 10)], fill=rgba(roofs[0]))
    d.line([(4, 2), (17, 2)], fill=rgba(roofs[0]))
    return outlined(img)


def tent(rng: random.Random) -> Image.Image:
    img, d = blank(24, 22)
    d.polygon([(12, 2), (1, 19), (23, 19)], fill=rgba("#c9b48a"))
    d.polygon([(12, 2), (1, 19), (10, 19)], fill=rgba("#e6d6ae"))
    d.polygon([(12, 8), (9, 19), (15, 19)], fill=rgba("#4a3826"))
    d.line([(12, 0), (12, 3)], fill=rgba("#4a3020"))
    d.polygon([(13, 0), (19, 1), (13, 3)], fill=rgba("#c83a2a"))
    return outlined(img)


def gatehouse(rng: random.Random) -> Image.Image:
    """Fort (본진/관문): a stone base with a two-tier roof, 28×26."""
    img, d = blank(28, 26)
    d.rectangle((3, 12, 24, 24), fill=rgba("#9a9284"))
    d.rectangle((3, 12, 6, 24), fill=rgba("#b8b0a0"))
    for y in range(14, 24, 3):
        d.line([(3, y), (24, y)], fill=rgba("#7a7366"))
    d.rectangle((11, 17, 16, 24), fill=rgba("#3a2a1c"))
    d.polygon([(1, 13), (5, 6), (22, 6), (26, 13)], fill=rgba("#335c9a"))
    d.polygon([(1, 13), (5, 6), (9, 6), (6, 13)], fill=rgba("#5a86c4"))
    d.polygon([(7, 7), (10, 1), (17, 1), (20, 7)], fill=rgba("#24406e"))
    d.polygon([(7, 7), (10, 1), (12, 1), (10, 7)], fill=rgba("#5a86c4"))
    d.line([(13, 0), (14, 0)], fill=rgba("#e0c050"))
    return outlined(img)


def storehouse(rng: random.Random, gold: bool) -> Image.Image:
    """Granary (straw roof) or treasury (gold-trimmed roof), 24×20."""
    roof = ("#8a6a2a", "#c09a40", "#e8cc70") if not gold else ("#6a2a1a", "#9a3a24", "#e0b040")
    img, d = blank(24, 20)
    d.rectangle((3, 9, 20, 18), fill=rgba("#a08060"))
    d.rectangle((3, 9, 5, 18), fill=rgba("#c0a080"))
    d.rectangle((9, 12, 14, 18), fill=rgba("#4a3020"))
    if gold:
        d.rectangle((10, 13, 13, 14), fill=rgba("#e0b040"))
    d.polygon([(0, 10), (5, 1), (18, 1), (23, 10)], fill=rgba(roof[1]))
    d.polygon([(0, 10), (5, 1), (9, 1), (5, 10)], fill=rgba(roof[2]))
    d.line([(0, 10), (23, 10)], fill=rgba(roof[0]))
    return outlined(img)


def field_patch(rng: random.Random) -> Image.Image:
    img, d = blank(14, 9)
    d.rectangle((0, 0, 13, 8), fill=rgba("#8a7a3a"))
    for y in range(1, 8, 2):
        d.line([(1, y), (12, y)], fill=rgba("#b8a84a"))
    return outlined(img, rgba("#5a4a24"))


# ---------------------------------------------------------------- units


def _body_colours(side: str) -> dict[str, tuple]:
    cloth = {
        "player": ("#1c3c8c", "#2f5cc4", "#6a94e8"),
        "ally": ("#1c6a2c", "#2f9a40", "#74d06a"),
        "enemy": ("#8c1c1c", "#c43030", "#ec7a6a"),
    }[side]
    return {
        "cloth_d": rgba(cloth[0]),
        "cloth": rgba(cloth[1]),
        "cloth_l": rgba(cloth[2]),
        "skin": rgba("#f0b888"),
        "skin_d": rgba("#c07a50"),
        "metal": rgba("#c8ccd4"),
        "metal_d": rgba("#7a808c"),
        "horse": rgba("#8a5a30"),
        "horse_d": rgba("#5a381c"),
        "horse_l": rgba("#b07a44"),
        "mane": rgba("#2a1a10"),
        "wood": rgba("#6a4424"),
        "gold": rgba("#e0b848"),
    }


def light_cavalry(side: str, frame: int) -> Image.Image:
    """경기병: a rider with a raised spear on a horse in a three-quarter view facing right, 32×32.

    Two frames like the original's map icons: the horse's legs alternate and the rider bobs a
    pixel. The anchor is (16, 31), so the hooves stand on the bottom rows.
    """
    c = _body_colours(side)
    img, d = blank(32, 32)
    bob = frame  # rider one pixel lower on frame 1
    # ---- horse
    # legs (back pair darker), alternate per frame
    if frame == 0:
        legs = [((9, 21), (7, 29)), ((12, 21), (13, 29)), ((20, 21), (19, 29)), ((23, 21), (25, 28))]
    else:
        legs = [((9, 21), (10, 29)), ((12, 21), (10, 28)), ((20, 21), (22, 29)), ((23, 21), (21, 29))]
    for i, (a, b) in enumerate(legs):
        col = c["horse_d"] if i in (0, 2) else c["horse"]
        d.line([a, b], fill=col, width=2)
        d.point((b[0], b[1] + 1), fill=c["mane"])
        d.point((b[0] + 1, b[1] + 1), fill=c["mane"])
    # body
    d.ellipse((6, 14, 25, 23), fill=c["horse"])
    d.ellipse((7, 14, 19, 19), fill=c["horse_l"])
    d.arc((6, 14, 25, 23), 20, 160, fill=c["horse_d"])
    # tail
    d.line([(6, 16), (3, 20), (4, 24)], fill=c["mane"], width=2)
    # neck and head, facing right
    d.polygon([(21, 16), (24, 9), (28, 8), (26, 17)], fill=c["horse"])
    d.polygon([(24, 8), (30, 10), (30, 13), (26, 13)], fill=c["horse"])
    d.line([(24, 9), (22, 15)], fill=c["horse_l"])
    d.line([(22, 8), (21, 14)], fill=c["mane"], width=2)
    d.point((27, 10), fill=c["mane"])
    d.point((24, 7), fill=c["horse_d"])
    # saddle cloth
    d.rectangle((11, 15, 18, 19), fill=c["cloth"])
    d.line([(11, 19), (18, 19)], fill=c["gold"])
    # ---- rider
    y = bob
    # leg
    d.line([(15, 16 + y), (14, 21 + y)], fill=c["cloth_d"], width=2)
    # torso
    d.rectangle((13, 9 + y, 18, 16 + y), fill=c["cloth"])
    d.rectangle((13, 9 + y, 14, 16 + y), fill=c["cloth_l"])
    d.line([(13, 13 + y), (18, 13 + y)], fill=c["gold"])
    # arm holding the spear
    d.line([(18, 11 + y), (21, 13 + y)], fill=c["cloth"], width=2)
    # head and helmet
    d.rectangle((14, 4 + y, 18, 8 + y), fill=c["skin"])
    d.point((17, 6 + y), fill=c["skin_d"])
    d.rectangle((13, 2 + y, 18, 4 + y), fill=c["metal"])
    d.point((13, 4 + y), fill=c["metal_d"])
    d.line([(15, 0 + y), (16, 1 + y)], fill=c["cloth_l"])  # plume
    # spear: shaft from lower left to upper right, tip
    d.line([(10, 21 + y), (25, 1 + y)], fill=c["wood"])
    d.polygon([(25, 1 + y), (28, -2 + y), (26, 3 + y)], fill=c["metal"])
    d.point((21, 13 + y), fill=c["skin"])
    img = outlined(img)
    # ground shadow under the hooves, drawn beneath
    shadow, sd = blank(32, 32)
    sd.ellipse((5, 27, 28, 31), fill=SHADOW)
    shadow.alpha_composite(img)
    return shadow
