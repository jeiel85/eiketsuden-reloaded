"""Unit sprites of the trial art pack: every map icon of the original mode, built from parts.

Each sprite is two 32×32 frames facing right (three-quarter view), like the original's map icons;
`build_pack.unit_sheet` mirrors and repeats them into the 4 × 6 sheet. Figures are chibi soldiers
(big head) assembled from a head, a body, legs, arms and a weapon, so all classes share one style.
Side colours: player blue, ally green, enemy red. Made for this project, CC0.
"""

from __future__ import annotations

from dataclasses import dataclass, field

from PIL import Image, ImageDraw
from sprites import SHADOW, blank, outlined, rgba

SIDE_CLOTH = {
    "player": ("#1c3c8c", "#2f5cc4", "#6a94e8"),
    "ally": ("#1c6a2c", "#2f9a40", "#74d06a"),
    "enemy": ("#8c1c1c", "#c43030", "#ec7a6a"),
}


def palette(side: str) -> dict[str, tuple]:
    cd, c, cl = SIDE_CLOTH[side]
    p = {
        "cloth_d": cd,
        "cloth": c,
        "cloth_l": cl,
        "skin": "#f0b888",
        "skin_d": "#c07a50",
        "hair": "#2a1c14",
        "metal": "#c8ccd4",
        "metal_l": "#eef0f4",
        "metal_d": "#7a808c",
        "leather": "#8a5a30",
        "leather_d": "#5a381c",
        "wood": "#7a4e28",
        "wood_d": "#4e3018",
        "gold": "#e0b848",
        "gold_d": "#a07a28",
        "pants": "#4a3a2c",
        "boot": "#2a2018",
        "eye": "#20160e",
        "white": "#f4f2ea",
        "cream": "#e0d4b0",
    }
    return {k: rgba(v) for k, v in p.items()}


@dataclass
class Look:
    """What a figure wears and holds."""

    head: str = "helmet"  # helmet, hood, bandana, topknot, turban, cap, crown, feathers, hat, bare
    body: str = "tunic"  # tunic, armor, robe, vest, bare
    weapon: str = "none"  # sword, spear, halberd, bow, crossbow, staff, broadsword, club, dao, fist, scimitar, drum
    offhand: str = "none"  # shield, round_shield, none
    beard: bool = False
    cape: bool = False
    plume: bool = False
    headband: bool = False  # a gold band (officers)
    tint: dict = field(default_factory=dict)  # colour overrides by key


# ------------------------------------------------------------------- figure parts


def _head(d: ImageDraw.ImageDraw, p: dict, look: Look, cx: int, top: int) -> None:
    x0 = cx - 4
    # face
    d.rectangle((x0 + 1, top + 3, x0 + 7, top + 9), fill=p["skin"])
    d.rectangle((x0 + 7, top + 4, x0 + 7, top + 9), fill=p["skin_d"])
    d.point((x0 + 4, top + 6), fill=p["eye"])
    d.point((x0 + 6, top + 6), fill=p["eye"])
    d.point((x0 + 7, top + 8), fill=p["skin_d"])
    # back of the head (left, the figure faces right)
    d.rectangle((x0, top + 3, x0 + 1, top + 8), fill=p["hair"])
    h = look.head
    if h == "helmet":
        d.rectangle((x0, top + 1, x0 + 7, top + 4), fill=p["metal"])
        d.rectangle((x0 + 1, top, x0 + 6, top), fill=p["metal"])
        d.line([(x0 + 1, top + 1), (x0 + 3, top + 1)], fill=p["metal_l"])
        d.rectangle((x0, top + 4, x0 + 1, top + 8), fill=p["metal_d"])
        d.line([(x0, top + 4), (x0 + 7, top + 4)], fill=p["metal_d"])
    elif h == "hood":
        d.rectangle((x0 - 1, top + 1, x0 + 7, top + 3), fill=p["cloth"])
        d.rectangle((x0 - 1, top + 3, x0 + 1, top + 10), fill=p["cloth"])
        d.rectangle((x0 + 1, top, x0 + 6, top), fill=p["cloth"])
        d.line([(x0 + 1, top + 1), (x0 + 4, top + 1)], fill=p["cloth_l"])
        d.line([(x0 + 7, top + 2), (x0 + 7, top + 3)], fill=p["cloth_d"])
    elif h == "bandana":
        d.rectangle((x0, top + 1, x0 + 7, top + 2), fill=p["hair"])
        d.rectangle((x0, top + 3, x0 + 7, top + 3), fill=p["cloth"])
        d.line([(x0 - 2, top + 3), (x0, top + 4)], fill=p["cloth"])
        d.point((x0 - 2, top + 5), fill=p["cloth"])
    elif h == "topknot":
        d.rectangle((x0, top + 1, x0 + 7, top + 3), fill=p["hair"])
        d.rectangle((x0 + 2, top - 2, x0 + 4, top), fill=p["hair"])
        d.point((x0 + 3, top - 1), fill=p["cloth"])
    elif h == "turban":
        d.rectangle((x0 - 1, top, x0 + 7, top + 3), fill=p["cream"])
        d.line([(x0, top + 1), (x0 + 6, top + 3)], fill=p["gold_d"])
        d.point((x0 + 5, top + 1), fill=p["cloth"])
    elif h == "cap":
        d.rectangle((x0, top + 1, x0 + 7, top + 3), fill=p["cloth_d"])
        d.rectangle((x0 + 1, top - 1, x0 + 5, top), fill=p["cloth_d"])
    elif h == "hat":
        d.rectangle((x0 - 2, top + 2, x0 + 9, top + 3), fill=p["cream"])
        d.rectangle((x0 + 1, top - 1, x0 + 6, top + 1), fill=p["cream"])
        d.line([(x0 - 2, top + 3), (x0 + 9, top + 3)], fill=p["gold_d"])
    elif h == "crown":
        d.rectangle((x0, top + 1, x0 + 7, top + 4), fill=p["gold"])
        d.rectangle((x0 + 1, top - 2, x0 + 6, top), fill=p["gold"])
        d.point((x0 + 3, top - 1), fill=p["cloth"])
        d.line([(x0, top + 4), (x0 + 7, top + 4)], fill=p["gold_d"])
        d.rectangle((x0, top + 4, x0 + 1, top + 8), fill=p["gold_d"])
    elif h == "feathers":
        d.rectangle((x0, top + 1, x0 + 7, top + 4), fill=p["gold"])
        d.line([(x0, top + 4), (x0 + 7, top + 4)], fill=p["gold_d"])
        d.rectangle((x0, top + 4, x0 + 1, top + 8), fill=p["gold_d"])
        # two long pheasant tail feathers curling back
        d.line([(x0 + 2, top + 1), (x0 - 3, top), (x0 - 7, top + 3)], fill=p["cloth_l"])
        d.line([(x0 + 4, top + 1), (x0 - 1, top - 1), (x0 - 6, top)], fill=p["cloth"])
    elif h == "bare":
        d.rectangle((x0, top + 1, x0 + 7, top + 2), fill=p["hair"])
    if look.plume:
        d.line([(x0 + 3, top - 1), (x0 + 1, top - 4)], fill=p["cloth_l"], width=2)
    if look.headband:
        d.line([(x0, top + 3), (x0 + 7, top + 3)], fill=p["gold"])
    if look.beard:
        d.rectangle((x0 + 3, top + 9, x0 + 7, top + 11), fill=p["hair"])
        d.point((x0 + 5, top + 8), fill=p["hair"])


def _body(d: ImageDraw.ImageDraw, p: dict, look: Look, cx: int, top: int) -> None:
    """Torso from `top` (shoulders) to top+8 (waist)."""
    x0, x1 = cx - 4, cx + 3
    b = look.body
    if b == "robe":
        d.polygon([(x0, top), (x1, top), (x1 + 2, top + 14), (x0 - 2, top + 14)], fill=p["cloth"])
        d.polygon([(x0, top), (x0 + 2, top), (x0, top + 14), (x0 - 2, top + 14)], fill=p["cloth_l"])
        d.line([(x1, top), (x1 + 2, top + 14)], fill=p["cloth_d"])
        d.line([(cx, top + 1), (cx, top + 14)], fill=p["cloth_d"])
        d.line([(x0 - 2, top + 14), (x1 + 2, top + 14)], fill=p["cloth_d"])
        return
    colour = p["skin"] if b == "bare" else p["cloth"]
    d.rectangle((x0, top, x1, top + 8), fill=colour)
    if b != "bare":
        d.rectangle((x0, top, x0 + 1, top + 8), fill=p["cloth_l"])
        d.rectangle((x1, top + 1, x1, top + 8), fill=p["cloth_d"])
    else:
        d.rectangle((x1, top + 1, x1, top + 8), fill=p["skin_d"])
    if b == "armor":
        d.rectangle((x0 + 1, top + 1, x1 - 1, top + 5), fill=p["metal"])
        d.line([(x0 + 1, top + 3), (x1 - 1, top + 3)], fill=p["metal_d"])
        d.line([(x0 + 1, top + 1), (x0 + 3, top + 1)], fill=p["metal_l"])
        d.rectangle((x0 - 1, top, x0, top + 2), fill=p["metal"])  # pauldron
    if b == "vest":
        d.rectangle((x0, top, x0 + 2, top + 8), fill=p["leather"])
        d.rectangle((x1 - 1, top, x1, top + 8), fill=p["leather_d"])
    d.line([(x0, top + 7), (x1, top + 7)], fill=p["gold"] if b == "armor" else p["leather_d"])


def _legs(d: ImageDraw.ImageDraw, p: dict, cx: int, top: int, frame: int) -> None:
    """Legs from `top` (waist) down to the feet at top+7."""
    a, b = (-1, 1) if frame == 0 else (1, -1)
    for lx, off in ((cx - 3, a), (cx + 1, b)):
        d.rectangle((lx, top, lx + 1, top + 5), fill=p["pants"])
        d.rectangle((lx + off, top + 5, lx + off + 2, top + 7), fill=p["boot"])
    d.rectangle((cx - 3, top, cx + 2, top + 1), fill=p["pants"])


def _weapon(d: ImageDraw.ImageDraw, p: dict, look: Look, cx: int, top: int) -> None:
    """The weapon arm (front, right) and what it holds; `top` = shoulders."""
    hx, hy = cx + 5, top + 5  # hand
    w = look.weapon
    arm = p["skin"] if look.body == "bare" else p["cloth"]
    if w in ("bow",):
        hx, hy = cx + 6, top + 3
    d.line([(cx + 3, top + 1), (hx, hy)], fill=arm, width=2)
    if w == "sword":
        d.line([(hx, hy - 1), (hx + 2, hy - 11)], fill=p["metal"])
        d.line([(hx + 1, hy - 2), (hx + 3, hy - 11)], fill=p["metal_l"])
        d.line([(hx - 2, hy - 1), (hx + 3, hy - 2)], fill=p["gold"])
    elif w == "broadsword":
        d.polygon([(hx, hy - 1), (hx + 1, hy - 13), (hx + 4, hy - 12), (hx + 3, hy - 1)], fill=p["metal"])
        d.line([(hx + 1, hy - 2), (hx + 2, hy - 12)], fill=p["metal_l"])
        d.line([(hx - 2, hy), (hx + 5, hy - 1)], fill=p["gold_d"])
    elif w == "dao":
        d.polygon([(hx, hy - 1), (hx + 5, hy - 9), (hx + 7, hy - 8), (hx + 2, hy)], fill=p["metal"])
        d.line([(hx + 1, hy - 2), (hx + 5, hy - 8)], fill=p["metal_l"])
        d.line([(hx - 1, hy + 1), (hx + 2, hy - 2)], fill=p["wood_d"], width=2)
    elif w == "scimitar":
        d.arc((hx - 2, hy - 14, hx + 10, hy + 2), 200, 300, fill=p["metal"], width=2)
        d.line([(hx - 1, hy), (hx + 1, hy - 2)], fill=p["gold"])
    elif w == "club":
        d.line([(hx, hy), (hx + 5, hy - 9)], fill=p["wood"], width=2)
        d.ellipse((hx + 3, hy - 13, hx + 7, hy - 7), fill=p["wood"])
        d.point((hx + 4, hy - 11), fill=p["wood_d"])
    elif w == "spear":
        d.line([(cx - 5, top + 14), (cx + 11, top - 8)], fill=p["wood"])
        d.polygon([(cx + 11, top - 8), (cx + 14, top - 11), (cx + 12, top - 6)], fill=p["metal"])
        d.point((cx + 10, top - 7), fill=p["cloth"])
    elif w == "halberd":
        d.line([(cx - 5, top + 14), (cx + 10, top - 9)], fill=p["wood"])
        d.polygon([(cx + 10, top - 9), (cx + 13, top - 12), (cx + 11, top - 7)], fill=p["metal"])
        d.polygon([(cx + 7, top - 5), (cx + 12, top - 2), (cx + 9, top - 2)], fill=p["metal"])  # crescent
        d.line([(cx + 8, top - 6), (cx + 6, top - 9)], fill=p["metal"])
    elif w == "staff":
        d.line([(hx + 1, top - 8), (hx + 1, top + 20)], fill=p["wood"])
        d.ellipse((hx - 1, top - 12, hx + 3, top - 8), fill=rgba("#9ae0f8"))
        d.point((hx, top - 11), fill=p["white"])
    elif w == "fist":
        d.line([(cx + 3, top + 1), (cx + 7, top - 1)], fill=arm, width=2)
        d.rectangle((cx + 7, top - 3, cx + 9, top - 1), fill=p["cloth"])
        d.rectangle((hx, hy - 2, hx + 2, hy), fill=p["cloth"])
    elif w == "drum":
        d.ellipse((cx - 4, top + 6, cx + 6, top + 14), fill=p["leather"])
        d.ellipse((cx - 4, top + 6, cx + 6, top + 9), fill=p["cream"])
        d.line([(cx - 4, top + 8), (cx + 6, top + 12)], fill=p["gold_d"])
        d.line([(hx, hy), (hx + 3, hy - 4)], fill=p["wood"])
    elif w == "bow":
        d.arc((hx - 2, top - 7, hx + 6, top + 15), 270, 90, fill=p["wood"], width=2)
        d.line([(hx + 2, top - 6), (hx + 2, top + 14)], fill=p["cream"])
        d.line([(cx - 2, top + 4), (hx + 8, top + 4)], fill=p["wood_d"])
        d.polygon([(hx + 8, top + 3), (hx + 10, top + 4), (hx + 8, top + 5)], fill=p["metal"])
        d.line([(cx - 4, top + 3), (cx - 2, top + 5)], fill=p["white"])
    elif w == "crossbow":
        d.line([(cx - 1, top + 5), (cx + 11, top + 5)], fill=p["wood"], width=2)
        d.line([(cx + 9, top), (cx + 9, top + 10)], fill=p["wood_d"], width=2)
        d.line([(cx + 9, top), (cx + 4, top + 5), (cx + 9, top + 10)], fill=p["cream"])
        d.point((cx + 12, top + 5), fill=p["metal"])


def _offhand(d: ImageDraw.ImageDraw, p: dict, look: Look, cx: int, top: int) -> None:
    if look.offhand == "shield":
        d.rectangle((cx - 8, top + 2, cx - 3, top + 11), fill=p["cloth_d"])
        d.rectangle((cx - 7, top + 3, cx - 4, top + 10), fill=p["leather"])
        d.line([(cx - 7, top + 3), (cx - 7, top + 10)], fill=p["gold"])
        d.point((cx - 5, top + 6), fill=p["gold"])
    elif look.offhand == "round_shield":
        d.ellipse((cx - 9, top + 2, cx - 2, top + 10), fill=p["leather"])
        d.ellipse((cx - 7, top + 4, cx - 4, top + 8), fill=p["gold_d"])
        d.point((cx - 6, top + 5), fill=p["gold"])


def _banner(d: ImageDraw.ImageDraw, p: dict, x: int, top: int, bottom: int) -> None:
    """A commander's banner on a pole behind the figure."""
    d.line([(x, top), (x, bottom)], fill=p["wood_d"])
    d.rectangle((x - 9, top + 1, x - 1, top + 9), fill=p["cloth"])
    d.rectangle((x - 9, top + 1, x - 8, top + 9), fill=p["cloth_l"])
    d.rectangle((x - 6, top + 3, x - 3, top + 7), fill=p["gold"])
    d.point((x - 5, top + 5), fill=p["cloth_d"])
    for k in range(x - 9, x, 2):
        d.point((k, top + 10), fill=p["cloth_d"])
    d.point((x, top - 1), fill=p["gold"])


def figure(d: ImageDraw.ImageDraw, p: dict, look: Look, cx: int, feet: int, frame: int) -> None:
    """A standing soldier whose feet touch row `feet`."""
    bob = frame
    legs_top = feet - 7
    body_top = legs_top - 8 + bob
    head_top = body_top - 10
    if look.cape:
        d.polygon([(cx - 4, body_top), (cx - 8, feet - 1), (cx - 2, feet - 2)], fill=p["cloth_d"])
    if look.body != "robe":
        _legs(d, p, cx, legs_top, frame)
    else:
        d.rectangle((cx - 3, feet - 1, cx + 3, feet), fill=p["boot"])
    _body(d, p, look, cx, body_top)
    _offhand(d, p, look, cx, body_top)
    _head(d, p, look, cx, head_top)
    _weapon(d, p, look, cx, body_top)


# ------------------------------------------------------------------- mounts and machines

HORSES = {
    "brown": ("#5a381c", "#8a5a30", "#b07a44", "#2a1a10"),
    "white": ("#9aa0aa", "#e4e6ea", "#ffffff", "#8a8a90"),
    "gold": ("#9a7028", "#d0a048", "#f0cc78", "#6a4a1c"),
    "red": ("#7a2410", "#b8401c", "#e07040", "#3a1408"),
    "black": ("#1e1a1c", "#3a3438", "#5a5258", "#101010"),
}


def horse(d: ImageDraw.ImageDraw, coat: str, frame: int, x: int = 0, barding: dict | None = None) -> None:
    """A horse facing right whose hooves touch row 29, body around x+6..x+25."""
    dk, md, lt, mane = (rgba(c) for c in HORSES[coat])
    if frame == 0:
        legs = [((9, 21), (7, 28)), ((12, 21), (13, 28)), ((20, 21), (19, 28)), ((23, 21), (25, 27))]
    else:
        legs = [((9, 21), (10, 28)), ((12, 21), (10, 27)), ((20, 21), (22, 28)), ((23, 21), (21, 28))]
    for i, (a, b) in enumerate(legs):
        d.line([(a[0] + x, a[1]), (b[0] + x, b[1])], fill=dk if i in (0, 2) else md, width=2)
        d.line([(b[0] + x, b[1] + 1), (b[0] + x + 1, b[1] + 1)], fill=mane)
    d.ellipse((6 + x, 14, 25 + x, 23), fill=md)
    d.ellipse((7 + x, 14, 19 + x, 19), fill=lt)
    d.arc((6 + x, 14, 25 + x, 23), 20, 160, fill=dk)
    d.line([(6 + x, 16), (3 + x, 20), (4 + x, 24)], fill=mane, width=2)
    d.polygon([(21 + x, 16), (24 + x, 9), (28 + x, 8), (26 + x, 17)], fill=md)
    d.polygon([(24 + x, 8), (30 + x, 10), (30 + x, 13), (26 + x, 13)], fill=md)
    d.line([(24 + x, 9), (22 + x, 15)], fill=lt)
    d.line([(22 + x, 8), (21 + x, 14)], fill=mane, width=2)
    d.point((27 + x, 10), fill=mane)
    d.point((24 + x, 7), fill=dk)
    if barding:
        d.rectangle((9 + x, 15, 22 + x, 21), fill=barding["metal"])
        for k in range(10 + x, 22 + x, 3):
            d.line([(k, 15), (k, 21)], fill=barding["metal_d"])
        d.line([(9 + x, 21), (22 + x, 21)], fill=barding["cloth"])
        d.polygon([(24 + x, 8), (29 + x, 10), (27 + x, 12), (24 + x, 11)], fill=barding["metal"])


def rider(d: ImageDraw.ImageDraw, p: dict, look: Look, cx: int, frame: int) -> None:
    """A rider's upper body sitting on a horse() drawn at x = 0."""
    bob = frame
    d.rectangle((11, 15, 18, 19), fill=p["cloth"])  # saddle cloth
    d.line([(11, 19), (18, 19)], fill=p["gold"])
    d.line([(cx, 16 + bob), (cx - 1, 21 + bob)], fill=p["pants"], width=2)
    body_top = 9 + bob
    _body(d, p, look, cx, body_top)
    _offhand(d, p, look, cx, body_top)
    _head(d, p, look, cx, body_top - 10)
    _weapon(d, p, look, cx, body_top)


def wheel(d: ImageDraw.ImageDraw, p: dict, cx: int, cy: int, r: int, frame: int) -> None:
    d.ellipse((cx - r, cy - r, cx + r, cy + r), fill=p["wood_d"])
    d.ellipse((cx - r + 1, cy - r + 1, cx + r - 1, cy + r - 1), fill=p["wood"])
    d.ellipse((cx - 1, cy - 1, cx + 1, cy + 1), fill=p["metal_d"])
    spokes = (
        [((-r + 1, 0), (r - 1, 0)), ((0, -r + 1), (0, r - 1))]
        if frame == 0
        else [
            ((-r + 2, -r + 2), (r - 2, r - 2)),
            ((-r + 2, r - 2), (r - 2, -r + 2)),
        ]
    )
    for (ax, ay), (bx, by) in spokes:
        d.line([(cx + ax, cy + ay), (cx + bx, cy + by)], fill=p["wood_d"])


# ------------------------------------------------------------------- sprites


def _finish(img: Image.Image, shadow=(5, 27, 27, 31)) -> Image.Image:
    img = outlined(img)
    out, sd = blank(32, 32)
    sd.ellipse(shadow, fill=SHADOW)
    out.alpha_composite(img)
    return out


def soldier_sprite(look: Look, banner: bool = False):
    def draw(side: str, frame: int) -> Image.Image:
        p = palette(side)
        p.update({k: rgba(v) for k, v in look.tint.items()})
        img, d = blank(32, 32)
        if banner:
            _banner(d, p, 9, 1, 26)
        figure(d, p, look, 16, 30, frame)
        return _finish(img, (8, 28, 25, 31))

    return draw


def pair_sprite(back: Look, front: Look):
    """Two figures (band, civilians): one behind on the left, one in front on the right."""

    def draw(side: str, frame: int) -> Image.Image:
        p = palette(side)
        img, d = blank(32, 32)
        figure(d, p, back, 10, 27, 1 - frame)
        layer, d2 = blank(32, 32)
        figure(d2, p, front, 21, 30, frame)
        img = outlined(img)
        img.alpha_composite(outlined(layer))
        out, sd = blank(32, 32)
        sd.ellipse((3, 27, 29, 31), fill=SHADOW)
        out.alpha_composite(img)
        return out

    return draw


def cavalry_sprite(look: Look, coat: str = "brown", armoured: bool = False, banner: bool = False):
    def draw(side: str, frame: int) -> Image.Image:
        p = palette(side)
        img, d = blank(32, 32)
        if banner:
            _banner(d, p, 11, 0, 16 + frame)
        horse(d, coat, frame, barding=p if armoured else None)
        rider(d, p, look, 15, frame)
        return _finish(img)

    return draw


def chariot_sprite(look: Look, coat: str = "brown", banner: bool = False):
    def draw(side: str, frame: int) -> Image.Image:
        p = palette(side)
        img, d = blank(32, 32)
        # two horses, the far one a little higher and darker
        far, fd = blank(32, 32)
        horse(fd, coat, 1 - frame, x=0)
        far = far.crop((0, 2, 32, 32))
        img.alpha_composite(outlined(far), (0, 0))
        horse(d, coat, frame, x=1)
        # the car: box with a big wheel, driver in it
        if banner:
            _banner(d, p, 9, 0, 18)
        d.rectangle((1, 14, 13, 21), fill=p["wood"])
        d.rectangle((1, 14, 13, 15), fill=p["cloth"])
        d.line([(1, 21), (13, 21)], fill=p["wood_d"])
        d.line([(13, 19), (18, 19)], fill=p["wood_d"])
        body_top = 6 + frame
        _body(d, p, look, 7, body_top)
        _head(d, p, look, 7, body_top - 10)
        _weapon(d, p, look, 7, body_top)
        d.rectangle((1, 14, 13, 18), fill=p["wood"])
        d.rectangle((1, 14, 13, 15), fill=p["cloth"])
        d.line([(2, 16), (12, 16)], fill=p["gold"])
        wheel(d, p, 6, 24, 6, frame)
        return _finish(img, (1, 27, 31, 31))

    return draw


def catapult(side: str, frame: int) -> Image.Image:
    p = palette(side)
    img, d = blank(32, 32)
    # base beam and the A-frame
    d.rectangle((3, 19, 28, 22), fill=p["wood"])
    d.line([(3, 19), (28, 19)], fill=p["wood_d"])
    d.polygon([(10, 19), (16, 6), (22, 19), (19, 19), (16, 11), (13, 19)], fill=p["wood_d"])
    # throwing arm: cocked on frame 0, raised a little on frame 1
    tip = (27, 4) if frame == 0 else (25, 2)
    d.line([(6, 17), tip], fill=p["wood"], width=2)
    d.ellipse((tip[0] - 2, tip[1] - 2, tip[0] + 2, tip[1] + 2), fill=p["leather_d"])
    d.ellipse((tip[0] - 1, tip[1] - 2, tip[0] + 1, tip[1]), fill=rgba("#8a8478"))
    d.rectangle((3, 13, 8, 18), fill=rgba("#6a6458"))  # counterweight
    d.line([(3, 13), (8, 13)], fill=rgba("#8a8478"))
    d.ellipse((15, 9, 17, 11), fill=p["metal_d"])
    # side-coloured cloth on the base and wheels
    d.rectangle((11, 20, 20, 22), fill=p["cloth"])
    d.line([(11, 20), (20, 20)], fill=p["cloth_l"])
    wheel(d, p, 7, 25, 4, frame)
    wheel(d, p, 24, 25, 4, frame)
    return _finish(img, (2, 28, 30, 31))


def supply(side: str, frame: int) -> Image.Image:
    p = palette(side)
    img, d = blank(32, 32)
    # cart with sacks, pushed by a soldier on the right
    d.rectangle((1, 16, 19, 22), fill=p["wood"])
    d.line([(1, 16), (19, 16)], fill=p["wood_d"])
    for sx in (2, 8, 13):
        d.ellipse((sx, 8, sx + 7, 17), fill=p["cream"])
        d.line([(sx + 3, 8), (sx + 4, 10)], fill=p["gold_d"])
    d.ellipse((5, 4, 12, 12), fill=rgba("#d8c890"))
    d.rectangle((3, 18, 17, 20), fill=p["cloth"])
    d.line([(19, 18), (23, 15)], fill=p["wood_d"])
    wheel(d, p, 9, 24, 5, frame)
    look = Look(head="cap", body="vest")
    figure(d, p, look, 25, 30, frame)
    return _finish(img, (0, 28, 31, 31))


def beast(side: str, frame: int) -> Image.Image:
    """맹수사: a tiger with its handler behind."""
    p = palette(side)
    img, d = blank(32, 32)
    handler = Look(head="bandana", body="vest", weapon="club")
    figure(d, p, handler, 9, 26, frame)
    tiger, td = blank(32, 32)
    org, dk, lt = rgba("#e08a28"), rgba("#1e1610"), rgba("#f8d8a0")
    leg = [(9, 27), (13, 28), (21, 27), (25, 28)] if frame == 0 else [(10, 28), (12, 27), (22, 28), (24, 27)]
    for lx, ly in leg:
        td.line([(lx, 23), (lx, ly)], fill=org, width=2)
    td.ellipse((7, 18, 27, 26), fill=org)
    td.ellipse((9, 22, 25, 26), fill=lt)
    for sx in (11, 15, 19, 23):
        td.line([(sx, 18), (sx + 1, 22)], fill=dk)
    td.ellipse((22, 13, 31, 22), fill=org)
    td.ellipse((26, 17, 31, 22), fill=lt)
    td.point((26, 16), fill=dk)
    td.point((29, 16), fill=dk)
    td.polygon([(23, 12), (24, 14), (25, 13)], fill=org)
    td.line([(24, 15), (25, 15)], fill=dk)
    td.line([(7, 20), (3, 16), (2, 12)], fill=org, width=2)
    td.point((2, 13), fill=dk)
    img = outlined(img)
    img.alpha_composite(outlined(tiger))
    out, sd = blank(32, 32)
    sd.ellipse((3, 27, 30, 31), fill=SHADOW)
    out.alpha_composite(img)
    return out


def confused(side: str, frame: int) -> Image.Image:
    """혼란: a soldier swaying, with stars circling over the head."""
    p = palette(side)
    img, d = blank(32, 32)
    look = Look(head="helmet", body="tunic", weapon="none", offhand="shield")
    figure(d, p, look, 15 + (1 if frame else -1), 30, frame)
    img = outlined(img)
    stars, sd = blank(32, 32)
    pts = [(8, 4), (16, 1), (24, 4)] if frame == 0 else [(10, 2), (20, 1), (25, 6)]
    for sx, sy in pts:
        sd.line([(sx - 2, sy), (sx + 2, sy)], fill=rgba("#f8e060"))
        sd.line([(sx, sy - 2), (sx, sy + 2)], fill=rgba("#f8e060"))
        sd.point((sx, sy), fill=rgba("#ffffff"))
    img.alpha_composite(stars)
    out, sh = blank(32, 32)
    sh.ellipse((8, 28, 25, 31), fill=SHADOW)
    out.alpha_composite(img)
    return out


SOLDIER = Look(head="helmet", body="tunic", weapon="sword", offhand="shield", plume=True)
LIU_BEI = {"headband": True, "cape": True}

SPRITES = {
    "short_infantry": soldier_sprite(SOLDIER),
    "long_infantry": soldier_sprite(Look(head="helmet", body="armor", weapon="spear", plume=True)),
    "archer": soldier_sprite(Look(head="hood", body="tunic", weapon="bow")),
    "crossbow": soldier_sprite(Look(head="helmet", body="armor", weapon="crossbow")),
    "bandit": soldier_sprite(Look(head="bandana", body="vest", weapon="club")),
    "brigand": soldier_sprite(Look(head="topknot", body="vest", weapon="broadsword", beard=True)),
    "outlaw": soldier_sprite(Look(head="cap", body="tunic", weapon="dao", beard=True)),
    "martial": soldier_sprite(Look(head="bare", body="bare", weapon="fist", headband=True)),
    "tribe": soldier_sprite(Look(head="turban", body="vest", weapon="scimitar", offhand="round_shield")),
    "sorcerer": soldier_sprite(Look(head="hood", body="robe", weapon="staff", beard=True)),
    "band": pair_sprite(Look(head="hat", body="tunic", weapon="drum"), Look(head="hat", body="tunic", weapon="drum")),
    "civilian": pair_sprite(Look(head="hat", body="robe"), Look(head="bare", body="tunic")),
    "light_cavalry": cavalry_sprite(Look(head="helmet", body="tunic", weapon="spear", plume=True)),
    "heavy_cavalry": cavalry_sprite(Look(head="helmet", body="armor", weapon="halberd"), coat="black", armoured=True),
    "guard_cavalry": cavalry_sprite(
        Look(head="helmet", body="armor", weapon="spear", plume=True, cape=True), coat="white", banner=True
    ),
    "chariot": chariot_sprite(Look(head="helmet", body="armor", weapon="spear")),
    "catapult": catapult,
    "supply": supply,
    "beast": beast,
    "status_confused": confused,
    "officer_liu_bei_short_infantry": soldier_sprite(
        Look(head="topknot", body="armor", weapon="sword", offhand="shield", **LIU_BEI), banner=True
    ),
    "officer_liu_bei_long_infantry": soldier_sprite(
        Look(head="topknot", body="armor", weapon="spear", **LIU_BEI), banner=True
    ),
    "officer_liu_bei_chariot": chariot_sprite(
        Look(head="topknot", body="armor", weapon="sword", headband=True), coat="white", banner=True
    ),
    "officer_cao_cao": cavalry_sprite(
        Look(head="crown", body="armor", weapon="sword", cape=True), coat="gold", banner=True
    ),
    "officer_lu_bu": cavalry_sprite(Look(head="feathers", body="armor", weapon="halberd"), coat="red", banner=True),
}
