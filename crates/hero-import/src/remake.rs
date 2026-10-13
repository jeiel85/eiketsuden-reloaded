//! New art for the original mode (`docs/DECISIONS.md` D27, the view setting "그림: 새 그림").
//!
//! The converter writes, next to the original's pictures, battle-map pictures redrawn from the
//! maps' rules grids (`gfx/remake/maps/<key>.png`: the whole maps and the cell pictures battle
//! scripts swap in) and unit sheets drawn for this project (`gfx/remake/units/<sheet>.png`). The
//! game shows them instead of the original's when the setting asks for them.
//!
//! The map pictures follow the original's map layouts, so like the rest of the converted pack
//! they are made from the player's copy at conversion time and never shipped. The sprites they
//! are built from are this project's own (CC0): `assets/remake/*.png`, drawn by
//! `tools/artpack/build.py`, which also writes the `*.txt` indexes read here.
//!
//! A picture depends only on the grid and global pixel coordinates (hash noise, no random
//! state), so a window of a map ([`render_cells`]) is exactly the same pixels as that part of the
//! whole map ([`render_map`]): a cell picture drawn from the unchanged grid fits seamlessly. A
//! changed cell is redrawn alone, so what the change does to its neighbours (a wall's shadow on
//! the cell below, ground edges) stays as the whole-map picture drew it (BACKLOG).

use crate::image::PngError;
use std::collections::BTreeMap;
use std::sync::OnceLock;

/// Pixels per map cell (the original mode's maps are drawn at 32 px per cell).
pub const CELL: usize = 32;
/// Frame size of the unit sprites (the original's map icons).
pub const UNIT_FRAME: usize = 32;
/// Directory (under `gfx/`) of the new art in a converted pack.
pub const REMAKE_DIR: &str = "remake";

const OBJECTS_PNG: &[u8] = include_bytes!("../assets/remake/objects.png");
const OBJECTS_INDEX: &str = include_str!("../assets/remake/objects.txt");
const UNITS_PNG: &[u8] = include_bytes!("../assets/remake/units.png");
const UNITS_INDEX: &str = include_str!("../assets/remake/units.txt");

/// An RGBA image, row-major, 8 bits per channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RgbaImage {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<[u8; 4]>,
}

impl RgbaImage {
    pub fn new(width: usize, height: usize) -> RgbaImage {
        RgbaImage {
            width,
            height,
            pixels: vec![[0; 4]; width * height],
        }
    }

    fn crop(&self, x0: usize, y0: usize, w: usize, h: usize) -> RgbaImage {
        let mut out = RgbaImage::new(w, h);
        for y in 0..h {
            let src = (y0 + y) * self.width + x0;
            out.pixels[y * w..(y + 1) * w].copy_from_slice(&self.pixels[src..src + w]);
        }
        out
    }

    /// Draw `src` with its alpha over this image at (x, y) (may be partly outside).
    fn blend(&mut self, src: &RgbaImage, x: i64, y: i64) {
        for sy in 0..src.height {
            let ty = y + sy as i64;
            if ty < 0 || ty >= self.height as i64 {
                continue;
            }
            for sx in 0..src.width {
                let tx = x + sx as i64;
                if tx < 0 || tx >= self.width as i64 {
                    continue;
                }
                let s = src.pixels[sy * src.width + sx];
                let d = &mut self.pixels[ty as usize * self.width + tx as usize];
                *d = over(*d, s);
            }
        }
    }

    fn flipped(&self) -> RgbaImage {
        let mut out = self.clone();
        for y in 0..self.height {
            out.pixels[y * self.width..(y + 1) * self.width].reverse();
        }
        out
    }

    /// Encode as an 8-bit RGBA PNG.
    pub fn to_png(&self) -> Result<Vec<u8>, PngError> {
        let width = u32::try_from(self.width).map_err(|e| PngError(e.to_string()))?;
        let height = u32::try_from(self.height).map_err(|e| PngError(e.to_string()))?;
        let mut out = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut out, width, height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.set_compression(png::Compression::Fast);
            let mut writer = encoder
                .write_header()
                .map_err(|e| PngError(e.to_string()))?;
            let data: Vec<u8> = self.pixels.iter().flatten().copied().collect();
            writer
                .write_image_data(&data)
                .map_err(|e| PngError(e.to_string()))?;
            writer.finish().map_err(|e| PngError(e.to_string()))?;
        }
        Ok(out)
    }
}

/// `s` drawn with its alpha over `d`.
fn over(d: [u8; 4], s: [u8; 4]) -> [u8; 4] {
    match s[3] {
        0 => d,
        255 => s,
        a => {
            let a = u32::from(a);
            let da = u32::from(d[3]);
            let out_a = a + da * (255 - a) / 255;
            if out_a == 0 {
                return [0; 4];
            }
            let mix = |sc: u8, dc: u8| {
                ((u32::from(sc) * a + u32::from(dc) * da * (255 - a) / 255) / out_a) as u8
            };
            [
                mix(s[0], d[0]),
                mix(s[1], d[1]),
                mix(s[2], d[2]),
                out_a as u8,
            ]
        }
    }
}

fn decode_png(bytes: &[u8]) -> Result<RgbaImage, String> {
    let mut decoder = png::Decoder::new(bytes);
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let (w, h) = (info.width as usize, info.height as usize);
    let px = |i: usize| -> [u8; 4] {
        match info.color_type {
            png::ColorType::Rgba => [buf[4 * i], buf[4 * i + 1], buf[4 * i + 2], buf[4 * i + 3]],
            png::ColorType::Rgb => [buf[3 * i], buf[3 * i + 1], buf[3 * i + 2], 255],
            png::ColorType::GrayscaleAlpha => [buf[2 * i], buf[2 * i], buf[2 * i], buf[2 * i + 1]],
            _ => [buf[i], buf[i], buf[i], 255],
        }
    };
    Ok(RgbaImage {
        width: w,
        height: h,
        pixels: (0..w * h).map(px).collect(),
    })
}

type Atlas = BTreeMap<String, RgbaImage>;

/// Sprites cut from an atlas by an index of `name x y w h` lines (`#` comments).
fn cut_atlas(png: &[u8], index: &str) -> Result<Atlas, String> {
    let atlas = decode_png(png)?;
    let mut out = BTreeMap::new();
    for line in index.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        let [name, x, y, w, h] = parts[..] else {
            return Err(format!("bad atlas line `{line}`"));
        };
        let n = |s: &str| s.parse::<usize>().map_err(|e| format!("`{line}`: {e}"));
        let (x, y, w, h) = (n(x)?, n(y)?, n(w)?, n(h)?);
        if x + w > atlas.width || y + h > atlas.height {
            return Err(format!("`{line}` lies outside the atlas"));
        }
        out.insert(name.to_string(), atlas.crop(x, y, w, h));
    }
    Ok(out)
}

/// The project's map-object sprites (trees, peaks, buildings, gate).
fn objects() -> Result<&'static Atlas, String> {
    static OBJECTS: OnceLock<Result<Atlas, String>> = OnceLock::new();
    OBJECTS
        .get_or_init(|| cut_atlas(OBJECTS_PNG, OBJECTS_INDEX))
        .as_ref()
        .map_err(|e| format!("remake objects atlas: {e}"))
}

/// The project's unit frames: `<sprite key>_<side>_<frame>` → 32×32.
fn unit_frames() -> Result<&'static Atlas, String> {
    static UNITS: OnceLock<Result<Atlas, String>> = OnceLock::new();
    UNITS
        .get_or_init(|| cut_atlas(UNITS_PNG, UNITS_INDEX))
        .as_ref()
        .map_err(|e| format!("remake units atlas: {e}"))
}

/// The new unit sheet for `<sprite key>_<side>` (as the original's sheets are named), laid out
/// like the original mode's sheets (`pack::unit_sheet`: 4 columns down, up, left, right — the
/// drawn facing in the first and last, mirrored in the middle two — and 6 rows alternating the
/// two frames). `Ok(None)` when the project has no drawing for that sheet.
pub fn unit_sheet(sheet: &str) -> Result<Option<RgbaImage>, String> {
    let frames = unit_frames()?;
    let (Some(f0), Some(f1)) = (
        frames.get(&format!("{sheet}_0")),
        frames.get(&format!("{sheet}_1")),
    ) else {
        return Ok(None);
    };
    let flips = [f0.flipped(), f1.flipped()];
    let plain = [f0, f1];
    let mut out = RgbaImage::new(4 * UNIT_FRAME, 6 * UNIT_FRAME);
    for row in 0..6 {
        for col in 0..4 {
            let frame = if matches!(col, 1 | 2) {
                &flips[row % 2]
            } else {
                plain[row % 2]
            };
            out.blend(frame, (col * UNIT_FRAME) as i64, (row * UNIT_FRAME) as i64);
        }
    }
    Ok(Some(out))
}

// ------------------------------------------------------------------------------- the grid

/// The ground a cell is drawn on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ground {
    Plain,
    Grass,
    Forest,
    Wasteland,
    Cliff,
    Mountain,
    Castle,
    River,
}

const GROUNDS: [Ground; 8] = [
    Ground::Plain,
    Ground::Grass,
    Ground::Forest,
    Ground::Wasteland,
    Ground::Cliff,
    Ground::Mountain,
    Ground::Castle,
    Ground::River,
];

impl Ground {
    fn of(terrain: &str) -> Option<Ground> {
        Some(match terrain {
            "plain" => Ground::Plain,
            "grass" => Ground::Grass,
            "forest" => Ground::Forest,
            "wasteland" => Ground::Wasteland,
            "cliff" => Ground::Cliff,
            "mountain" => Ground::Mountain,
            "castle" => Ground::Castle,
            "river" => Ground::River,
            _ => return None,
        })
    }

    /// Four shades, dark to light.
    fn ramp(self) -> [[u8; 3]; 4] {
        match self {
            Ground::Plain => [
                [95, 154, 50],
                [116, 178, 60],
                [134, 194, 70],
                [162, 214, 92],
            ],
            Ground::Grass => [[60, 116, 38], [74, 138, 46], [88, 156, 54], [110, 178, 68]],
            Ground::Forest => [[46, 94, 34], [56, 110, 40], [68, 128, 46], [84, 148, 56]],
            Ground::Wasteland => [
                [142, 108, 62],
                [164, 128, 74],
                [184, 148, 88],
                [206, 172, 108],
            ],
            Ground::Cliff => [
                [126, 96, 56],
                [147, 114, 68],
                [164, 132, 80],
                [184, 154, 98],
            ],
            Ground::Mountain => [
                [110, 98, 72],
                [128, 114, 86],
                [146, 132, 102],
                [168, 154, 122],
            ],
            Ground::Castle => [
                [110, 104, 88],
                [126, 120, 102],
                [140, 134, 116],
                [158, 152, 134],
            ],
            Ground::River => [
                [44, 92, 174],
                [58, 112, 196],
                [76, 134, 214],
                [124, 176, 236],
            ],
        }
    }

    /// Blur radius (px) of the ground's edges: big = organic, 0 = along the cells.
    fn softness(self) -> usize {
        match self {
            Ground::Plain | Ground::Grass => 14,
            Ground::Forest | Ground::Mountain => 10,
            Ground::Wasteland => 12,
            Ground::Cliff => 5,
            Ground::Castle => 0,
            Ground::River => 7,
        }
    }
}

fn is_wallish(t: &str) -> bool {
    matches!(t, "wall" | "closed_gate")
}

/// Terrain the renderer draws something of its own for (besides the grounds).
const OBJECTS: [&str; 10] = [
    "wall",
    "closed_gate",
    "bridge",
    "fence",
    "house",
    "village",
    "barracks",
    "fort",
    "granary",
    "treasury",
];

/// The terrain ids among `cells` the renderer has no drawing for: their cells show the ground
/// around them (for the converter's report).
pub fn unknown_terrain<'a>(
    cells: impl IntoIterator<Item = &'a str>,
) -> std::collections::BTreeSet<&'a str> {
    cells
        .into_iter()
        .filter(|t| Ground::of(t).is_none() && !OBJECTS.contains(t))
        .collect()
}

/// A map's terrain ids, one per cell, row-major.
struct Grid<'a> {
    w: usize,
    h: usize,
    cells: &'a [&'a str],
}

impl Grid<'_> {
    fn at(&self, x: i64, y: i64) -> Option<&str> {
        (x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h)
            .then(|| self.cells[y as usize * self.w + x as usize])
    }

    /// A gateway: open ground between two walls (an opened gate), drawn as paving.
    fn gateway(&self, x: i64, y: i64) -> bool {
        let wall = |dx: i64, dy: i64| self.at(x + dx, y + dy).is_some_and(is_wallish);
        (wall(-1, 0) && wall(1, 0)) || (wall(0, -1) && wall(0, 1))
    }

    /// The ground drawn under every cell: object cells take the most common ground around them
    /// (paving inside a town, water under a bridge).
    fn grounds(&self) -> Vec<Ground> {
        let mut out = Vec::with_capacity(self.cells.len());
        for y in 0..self.h as i64 {
            for x in 0..self.w as i64 {
                let t = self.cells[y as usize * self.w + x as usize];
                if let Some(g) = Ground::of(t) {
                    out.push(if g == Ground::Plain && self.gateway(x, y) {
                        Ground::Castle
                    } else {
                        g
                    });
                    continue;
                }
                if t == "bridge" {
                    out.push(Ground::River);
                    continue;
                }
                let building = matches!(t, "house" | "treasury" | "granary" | "fort");
                let mut counts = [0usize; 8];
                for reach in 1..=3i64 {
                    for dy in -reach..=reach {
                        for dx in -reach..=reach {
                            if let Some(g) = self.at(x + dx, y + dy).and_then(Ground::of) {
                                if g != Ground::River {
                                    counts[g as usize] += 1;
                                }
                            }
                        }
                    }
                    if counts.iter().any(|&c| c > 0) && (reach >= 2 || !building) {
                        break;
                    }
                }
                let castle = counts[Ground::Castle as usize] > 0;
                let g = if is_wallish(t) || (castle && t != "village") {
                    Ground::Castle
                } else {
                    // The most common ground; ties go to the earlier ground in GROUNDS.
                    let best = (0..8).max_by_key(|&i| (counts[i], 8 - i)).unwrap_or(0);
                    if counts[best] == 0 {
                        Ground::Plain
                    } else {
                        GROUNDS[best]
                    }
                };
                out.push(g);
            }
        }
        out
    }
}

// ------------------------------------------------------------------------------- noise

fn hash(mut x: u64) -> u64 {
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
    x ^= x >> 33;
    x = x.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    x ^ (x >> 33)
}

fn lattice(seed: u64, layer: u64, i: i64, j: i64) -> f32 {
    let h = hash(seed ^ hash(layer ^ hash((i as u64) ^ hash(j as u64).rotate_left(17))));
    (h >> 40) as f32 / (1u64 << 24) as f32
}

/// Smooth value noise in 0..1 at pixel (x, y), lattice spacing `scale` px.
fn noise(seed: u64, layer: u64, x: i64, y: i64, scale: i64) -> f32 {
    let (i, j) = (x.div_euclid(scale), y.div_euclid(scale));
    let fx = x.rem_euclid(scale) as f32 / scale as f32;
    let fy = y.rem_euclid(scale) as f32 / scale as f32;
    let sx = fx * fx * (3.0 - 2.0 * fx);
    let sy = fy * fy * (3.0 - 2.0 * fy);
    let a = lattice(seed, layer, i, j);
    let b = lattice(seed, layer, i + 1, j);
    let c = lattice(seed, layer, i, j + 1);
    let d = lattice(seed, layer, i + 1, j + 1);
    (a * (1.0 - sx) + b * sx) * (1.0 - sy) + (c * (1.0 - sx) + d * sx) * sy
}

/// [`noise`] over a rectangle of pixels, with the lattice values hashed once.
struct NoiseField {
    area: Rect,
    scale: i64,
    i0: i64,
    j0: i64,
    cols: usize,
    lattice: Vec<f32>,
    /// Smoothstep weight of each column and row of the area.
    wx: Vec<f32>,
    wy: Vec<f32>,
}

impl NoiseField {
    fn new(seed: u64, layer: u64, scale: i64, area: Rect) -> NoiseField {
        let i0 = area.x0.div_euclid(scale);
        let j0 = area.y0.div_euclid(scale);
        let i1 = (area.x0 + area.w as i64).div_euclid(scale) + 1;
        let j1 = (area.y0 + area.h as i64).div_euclid(scale) + 1;
        let cols = (i1 - i0 + 1) as usize;
        let rows = (j1 - j0 + 1) as usize;
        let mut lattice = Vec::with_capacity(cols * rows);
        for j in j0..=j1 {
            for i in i0..=i1 {
                lattice.push(self::lattice(seed, layer, i, j));
            }
        }
        let weight = |p: i64| {
            let f = p.rem_euclid(scale) as f32 / scale as f32;
            f * f * (3.0 - 2.0 * f)
        };
        NoiseField {
            area,
            scale,
            i0,
            j0,
            cols,
            lattice,
            wx: (0..area.w as i64).map(|x| weight(area.x0 + x)).collect(),
            wy: (0..area.h as i64).map(|y| weight(area.y0 + y)).collect(),
        }
    }

    /// The noise at pixel (px, py) of the area (area coordinates).
    fn at(&self, px: usize, py: usize) -> f32 {
        let x = self.area.x0 + px as i64;
        let y = self.area.y0 + py as i64;
        let i = (x.div_euclid(self.scale) - self.i0) as usize;
        let j = (y.div_euclid(self.scale) - self.j0) as usize;
        let k = j * self.cols + i;
        let (a, b) = (self.lattice[k], self.lattice[k + 1]);
        let (c, d) = (self.lattice[k + self.cols], self.lattice[k + self.cols + 1]);
        let (sx, sy) = (self.wx[px], self.wy[py]);
        (a * (1.0 - sx) + b * sx) * (1.0 - sy) + (c * (1.0 - sx) + d * sx) * sy
    }
}

/// A small deterministic random source per map cell (object placement).
struct CellRng(u64);

impl CellRng {
    fn new(seed: u64, x: i64, y: i64) -> CellRng {
        CellRng(hash(
            seed ^ hash(((y as u64) << 32) | (x as u64 & 0xffff_ffff)) ^ 0x5eed,
        ))
    }
    fn next(&mut self) -> u64 {
        self.0 = hash(self.0.wrapping_add(0x9e37_79b9_7f4a_7c15));
        self.0
    }
    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }
    /// A number in `lo..=hi`.
    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + (self.next() % (hi - lo + 1) as u64) as i64
    }
}

const BAYER: [[f32; 4]; 4] = [
    [0., 8., 2., 10.],
    [12., 4., 14., 6.],
    [3., 11., 1., 9.],
    [15., 7., 13., 5.],
];

// ------------------------------------------------------------------------------- rendering

/// A rectangle of pixels in map coordinates.
#[derive(Clone, Copy)]
struct Rect {
    x0: i64,
    y0: i64,
    w: usize,
    h: usize,
}

impl Rect {
    fn contains(&self, x: i64, y: i64) -> bool {
        x >= self.x0 && y >= self.y0 && x < self.x0 + self.w as i64 && y < self.y0 + self.h as i64
    }
}

/// Pixels a window's computations reach beyond it (blur, interpolation, cliff faces, objects).
const MARGIN: i64 = 2 * CELL as i64;

/// Box blur of radius `r` (separable, edges repeated).
fn box_blur(a: &[f32], w: usize, h: usize, r: usize) -> Vec<f32> {
    if r == 0 {
        return a.to_vec();
    }
    let n = (2 * r + 1) as f32;
    let ri = r as i64;
    let mut tmp = vec![0f32; a.len()];
    for y in 0..h {
        let row = &a[y * w..(y + 1) * w];
        let at = |i: i64| row[i.clamp(0, w as i64 - 1) as usize];
        let mut sum: f32 = (-ri..=ri).map(at).sum();
        for x in 0..w as i64 {
            tmp[y * w + x as usize] = sum / n;
            sum += at(x + ri + 1) - at(x - ri);
        }
    }
    let mut out = vec![0f32; a.len()];
    for x in 0..w {
        let at = |i: i64| tmp[i.clamp(0, h as i64 - 1) as usize * w + x];
        let mut sum: f32 = (-ri..=ri).map(at).sum();
        for y in 0..h as i64 {
            out[y as usize * w + x] = sum / n;
            sum += at(y + ri + 1) - at(y - ri);
        }
    }
    out
}

/// Whether any pixel within `r` (a square) of each pixel is set; edges repeated.
fn any_near(m: &[bool], w: usize, h: usize, r: usize) -> Vec<bool> {
    let f: Vec<f32> = m.iter().map(|&b| f32::from(u8::from(b))).collect();
    box_blur(&f, w, h, r)
        .into_iter()
        .map(|v| v > 1e-4)
        .collect()
}

/// The whole picture of a map whose cells hold `cells` (terrain ids, row-major, `width` per
/// row). `seed` varies the noise and object placement between maps.
pub fn render_map(
    cells: &[&str],
    width: usize,
    height: usize,
    seed: u64,
) -> Result<RgbaImage, String> {
    render_cells(cells, width, height, seed, (0, 0, width, height))
}

/// The pixels of cells `(x, y, w, h)` of that map, exactly as [`render_map`] draws them.
pub fn render_cells(
    cells: &[&str],
    width: usize,
    height: usize,
    seed: u64,
    (cx, cy, cw, ch): (usize, usize, usize, usize),
) -> Result<RgbaImage, String> {
    if cells.len() != width * height || width == 0 || height == 0 {
        return Err(format!("{} cells for a {width}×{height} map", cells.len()));
    }
    if cx + cw > width || cy + ch > height || cw == 0 || ch == 0 {
        return Err(format!(
            "cells {cx},{cy} {cw}×{ch} lie outside the {width}×{height} map"
        ));
    }
    let objects = objects()?;
    let get = |name: &str| {
        objects
            .get(name)
            .ok_or_else(|| format!("remake object `{name}` missing"))
    };
    let grid = Grid {
        w: width,
        h: height,
        cells,
    };
    let c = CELL as i64;
    let (map_w, map_h) = (width as i64 * c, height as i64 * c);
    // The window and the area computed around it, clipped to the map.
    let win = Rect {
        x0: cx as i64 * c,
        y0: cy as i64 * c,
        w: cw * CELL,
        h: ch * CELL,
    };
    let ax0 = (win.x0 - MARGIN).max(0);
    let ay0 = (win.y0 - MARGIN).max(0);
    let ax1 = (win.x0 + win.w as i64 + MARGIN).min(map_w);
    let ay1 = (win.y0 + win.h as i64 + MARGIN).min(map_h);
    let area = Rect {
        x0: ax0,
        y0: ay0,
        w: (ax1 - ax0) as usize,
        h: (ay1 - ay0) as usize,
    };
    let (aw, ah) = (area.w, area.h);
    let grounds = grid.grounds();

    // ---- ground classes: interpolated cell masks, softened, plus noise; the best wins
    let field1 = NoiseField::new(seed, 1, 9, area);
    let field2 = NoiseField::new(seed, 2, 23, area);
    let mut noise_at = vec![0f32; aw * ah];
    let mut n1 = vec![0f32; aw * ah];
    let mut n2 = vec![0f32; aw * ah];
    for py in 0..ah {
        for px in 0..aw {
            let i = py * aw + px;
            n1[i] = field1.at(px, py);
            n2[i] = field2.at(px, py);
            noise_at[i] = n1[i] * 0.6 + n2[i] * 0.4 - 0.5;
        }
    }
    let mut best = vec![(f32::MIN, Ground::Plain); aw * ah];
    for g in GROUNDS {
        if !grounds.contains(&g) {
            continue;
        }
        let r = g.softness();
        let cell = |x: i64, y: i64| -> f32 {
            let x = x.clamp(0, width as i64 - 1) as usize;
            let y = y.clamp(0, height as i64 - 1) as usize;
            f32::from(u8::from(grounds[y * width + x] == g))
        };
        let mut m = vec![0f32; aw * ah];
        for py in 0..ah {
            let y = area.y0 + py as i64;
            for px in 0..aw {
                let x = area.x0 + px as i64;
                m[py * aw + px] = if r == 0 {
                    cell(x / c, y / c)
                } else {
                    // Bilinear between cell centres.
                    let u = (x as f32 + 0.5) / CELL as f32 - 0.5;
                    let v = (y as f32 + 0.5) / CELL as f32 - 0.5;
                    let (i, j) = (u.floor() as i64, v.floor() as i64);
                    let (fu, fv) = (u - i as f32, v - j as f32);
                    (cell(i, j) * (1.0 - fu) + cell(i + 1, j) * fu) * (1.0 - fv)
                        + (cell(i, j + 1) * (1.0 - fu) + cell(i + 1, j + 1) * fu) * fv
                };
            }
        }
        if r > 0 {
            m = box_blur(&m, aw, ah, r / 3 + 1);
        }
        let jitter = match r {
            0 => 0.0,
            1..=9 => 0.2,
            _ => 0.35,
        };
        for (i, b) in best.iter_mut().enumerate() {
            let mut s = m[i];
            if s > 0.02 {
                s += noise_at[i] * jitter;
            }
            if s > b.0 {
                *b = (s, g);
            }
        }
    }
    let cls: Vec<Ground> = best.into_iter().map(|(_, g)| g).collect();

    // ---- texture: each ground's ramp, shade from noise and ordered dither
    let rgb = |c: [u8; 3]| [f32::from(c[0]), f32::from(c[1]), f32::from(c[2])];
    let mut img = vec![[0f32; 3]; aw * ah];
    let river: Vec<bool> = cls.iter().map(|&g| g == Ground::River).collect();
    let cliff: Vec<bool> = cls.iter().map(|&g| g == Ground::Cliff).collect();
    let fine = NoiseField::new(seed, 3, 3, area);
    for py in 0..ah {
        let y = area.y0 + py as i64;
        for px in 0..aw {
            let x = area.x0 + px as i64;
            let i = py * aw + px;
            let bayer = (BAYER[(y & 3) as usize][(x & 3) as usize] + 0.5) / 16.0;
            let f = 0.55 * fine.at(px, py) + 0.45 * n1[i] + (bayer - 0.5) * 0.35;
            let shade = ((f * 4.0) as i64).clamp(0, 3) as usize;
            img[i] = rgb(cls[i].ramp()[shade]);
            if cls[i] == Ground::Castle {
                // 8×8 paving, every other row offset by half a stone
                let off = (y.div_euclid(8) % 2) * 4;
                if y % 8 == 0 || (x + off) % 8 == 0 {
                    img[i] = rgb([92, 86, 72]);
                }
            } else if river[i]
                && (x as f32 * 0.45 + n2[i] * 9.0).sin() + (y as f32 * 0.9 + n1[i] * 7.0).sin()
                    > 1.55
            {
                // ripples
                img[i] = rgb([168, 208, 244]);
            }
        }
    }
    // water banks: a dark rim on the water, sand on the land
    let land: Vec<bool> = river.iter().map(|&r| !r).collect();
    let near_land = any_near(&land, aw, ah, 2);
    let near_water = any_near(&river, aw, ah, 2);
    for i in 0..aw * ah {
        if river[i] && near_land[i] {
            img[i] = rgb([36, 72, 138]);
        } else if land[i] && near_water[i] {
            img[i] = rgb([200, 180, 122]);
        }
    }
    // a darker rim along grass, forest and wasteland edges
    for g in [Ground::Grass, Ground::Forest, Ground::Wasteland] {
        let other: Vec<bool> = cls.iter().map(|&c| c != g).collect();
        let near = any_near(&other, aw, ah, 1);
        for i in 0..aw * ah {
            if cls[i] == g && near[i] {
                img[i] = img[i].map(|v| v * 0.85);
            }
        }
    }
    // cliffs: the plateau's lowest 14–22 px are a rock face, its top rim is lit
    let not_cliff: Vec<bool> = cliff.iter().map(|&c| !c).collect();
    let near_open = any_near(&not_cliff, aw, ah, 1);
    let mut face = vec![false; aw * ah];
    let columns = NoiseField::new(seed, 5, 5, area);
    for px in 0..aw {
        let x = area.x0 + px as i64;
        let height = 14 + (noise(seed, 4, x, 0, 11) * 9.0) as usize;
        for py in 0..ah {
            let i = py * aw + px;
            if !cliff[i] {
                continue;
            }
            let depth = (1..=22).find(|&k| py + k < ah && !cliff[(py + k) * aw + px]);
            match depth {
                Some(d) if d <= height => {
                    face[i] = true;
                    let colx = (x + (columns.at(px, py) * 4.0) as i64).rem_euclid(7);
                    img[i] = rgb(match colx {
                        0 | 1 => [142, 112, 70],
                        6 => [62, 44, 24],
                        _ => [106, 80, 48],
                    });
                    if d <= 4 {
                        img[i] = img[i].map(|v| v * 0.7);
                    }
                }
                _ if near_open[i] => img[i] = rgb([207, 176, 122]),
                _ => {}
            }
        }
    }
    for i in aw..aw * ah {
        if face[i - aw] && !cliff[i] {
            img[i] = img[i].map(|v| v * 0.6);
        }
    }

    let mut canvas = RgbaImage {
        width: aw,
        height: ah,
        pixels: img
            .iter()
            .map(|p| {
                let b = |v: f32| v.clamp(0.0, 255.0) as u8;
                [b(p[0]), b(p[1]), b(p[2]), 255]
            })
            .collect(),
    };
    let mut paint = Painter {
        canvas: &mut canvas,
        area,
    };

    // ---- cells drawn flat: walls (with their shadow), bridges and fences
    let cx0 = (area.x0 / c - 1).max(0);
    let cy0 = (area.y0 / c - 1).max(0);
    let cx1 = ((area.x0 + aw as i64) / c + 1).min(width as i64 - 1);
    let cy1 = ((area.y0 + ah as i64) / c + 1).min(height as i64 - 1);
    let wall = |x: i64, y: i64| grid.at(x, y).is_some_and(is_wallish);
    for y in cy0..=cy1 {
        for x in cx0..=cx1 {
            if wall(x, y) && y + 1 < height as i64 && !wall(x, y + 1) {
                for k in 0..4 {
                    let shadow = [0, 0, 0, (70 - k * 15) as u8];
                    paint.rect(
                        x * c,
                        (y + 1) * c + k,
                        x * c + c - 1,
                        (y + 1) * c + k,
                        shadow,
                    );
                }
            }
        }
    }
    for y in cy0..=cy1 {
        for x in cx0..=cx1 {
            let Some(t) = grid.at(x, y) else { continue };
            let (px, py) = (x * c, y * c);
            if is_wallish(t) {
                draw_wall(&mut paint, &wall, x, y, height as i64);
                if t == "closed_gate" {
                    paint.sprite(get("gate")?, px + 5, py + 10);
                }
            } else if t == "bridge" {
                draw_bridge(&mut paint, &grid, x, y);
            } else if t == "fence" {
                draw_fence(&mut paint, &grid, x, y);
            }
        }
    }

    // ---- standing objects, sorted by where they stand
    let trees = [get("tree0")?, get("tree1")?];
    let leaf = get("broadleaf")?;
    let peaks_big = [get("peak_big0")?, get("peak_big1")?, get("peak_big2")?];
    let peaks_small = [
        get("peak_small0")?,
        get("peak_small1")?,
        get("peak_small2")?,
    ];
    let houses = [get("house_blue")?, get("house_red")?, get("house_brown")?];
    let (field, tent, gatehouse) = (get("field")?, get("tent")?, get("gatehouse")?);
    let (granary, treasury) = (get("granary")?, get("treasury")?);
    let mut standing: Vec<(i64, i64, u64, &RgbaImage)> = Vec::new();
    for y in cy0..=cy1 {
        for x in cx0..=cx1 {
            let Some(t) = grid.at(x, y) else { continue };
            let (px, py) = (x * c, y * c);
            let mut rng = CellRng::new(seed, x, y);
            let order = |k: u64| ((y as u64) << 40) | ((x as u64) << 8) | k;
            let mut put = |img: &'static RgbaImage, fx: i64, fy: i64, k: u64| {
                standing.push((fy, fx, order(k), img));
            };
            match t {
                "forest" => {
                    let mut k = 0;
                    for sy in [4, 15, 26] {
                        for sx in [5, 16, 27] {
                            k += 1;
                            if rng.unit() < 0.82 {
                                let img = if rng.unit() < 0.12 {
                                    leaf
                                } else {
                                    trees[rng.range(0, 1) as usize]
                                };
                                let jx = rng.range(-3, 3);
                                let jy = rng.range(-2, 3);
                                put(img, px + sx + jx, py + sy + jy, k);
                            }
                        }
                    }
                }
                "mountain" => {
                    let v = rng.range(0, 2) as usize;
                    let jx = rng.range(-3, 3);
                    let jy = rng.range(-2, 2);
                    put(peaks_big[v], px + 16 + jx, py + 24 + jy, 1);
                    if rng.unit() < 0.6 {
                        let v = rng.range(0, 2) as usize;
                        let sx = rng.range(0, 20);
                        put(peaks_small[v], px + 6 + sx, py + 30, 2);
                    }
                }
                "house" => {
                    let roof = if rng.unit() < 0.35 {
                        0
                    } else if rng.unit() < 0.5 {
                        1
                    } else {
                        2
                    };
                    put(houses[roof], px + 16, py + 26, 1);
                }
                "village" => {
                    put(field, px + 9, py + 30, 1);
                    put(houses[2], px + 13, py + 18, 2);
                    put(houses[1], px + 21, py + 30, 3);
                }
                "barracks" => put(tent, px + 16, py + 27, 1),
                "fort" => put(gatehouse, px + 16, py + 29, 1),
                "granary" => put(granary, px + 16, py + 27, 1),
                "treasury" => put(treasury, px + 16, py + 27, 1),
                _ => {}
            }
        }
    }
    standing.sort_by_key(|&(fy, fx, k, _)| (fy, fx, k));
    for (fy, fx, _, img) in standing {
        let (w0, h0) = (img.width as i64, img.height as i64);
        let left = fx - w0 / 2;
        paint.ellipse(left + 1, fy - 2, left + w0 - 2, fy + 1, [0, 0, 0, 60]);
        paint.sprite(img, left, fy - h0);
    }

    Ok(canvas.crop(
        (win.x0 - area.x0) as usize,
        (win.y0 - area.y0) as usize,
        win.w,
        win.h,
    ))
}

/// A wall cell: a stone top, and a brick face where the cell below is not wall.
fn draw_wall(paint: &mut Painter, wall: &dyn Fn(i64, i64) -> bool, x: i64, y: i64, rows: i64) {
    let c = CELL as i64;
    let open_below = !wall(x, y + 1) && y + 1 < rows;
    let (open_left, open_right, open_above) = (!wall(x - 1, y), !wall(x + 1, y), !wall(x, y - 1));
    for dy in 0..c {
        for dx in 0..c {
            let col = if (open_left && dx == 0) || (open_right && dx == c - 1) {
                [60, 52, 42]
            } else if open_below && dy >= 18 {
                if dy == 18 {
                    [70, 60, 48]
                } else if (dy - 18) % 5 == 0 || (dx + ((dy - 18) / 5) % 2 * 4) % 8 == 0 {
                    [110, 96, 78]
                } else {
                    [150, 132, 104]
                }
            } else if open_above && dy < 3 {
                if (dx / 4) % 2 == 0 {
                    [214, 206, 182]
                } else {
                    [120, 110, 92]
                }
            } else if (dx + dy) % 9 == 0 {
                [160, 150, 126]
            } else {
                [186, 176, 150]
            };
            paint.put(x * c + dx, y * c + dy, [col[0], col[1], col[2], 255]);
        }
    }
}

/// Planks across the water, running towards the land.
fn draw_bridge(paint: &mut Painter, grid: &Grid, x: i64, y: i64) {
    let c = CELL as i64;
    let (px, py) = (x * c, y * c);
    let water = |dx: i64, dy: i64| grid.at(x + dx, y + dy).is_none_or(|n| n == "river");
    let along_x = !water(-1, 0) || !water(1, 0);
    let along_y = !water(0, -1) || !water(0, 1);
    let (plank, seam, rail) = ([120, 84, 48, 255], [84, 56, 30, 255], [60, 40, 22, 255]);
    if along_y && !along_x {
        paint.rect(px + 5, py - 2, px + 26, py + 33, plank);
        for k in (py - 2..py + 34).step_by(4) {
            paint.rect(px + 5, k, px + 26, k, seam);
        }
        paint.rect(px + 4, py - 2, px + 5, py + 33, rail);
        paint.rect(px + 26, py - 2, px + 27, py + 33, rail);
    } else {
        paint.rect(px - 2, py + 5, px + 33, py + 26, plank);
        for k in (px - 2..px + 34).step_by(4) {
            paint.rect(k, py + 5, k, py + 26, seam);
        }
        paint.rect(px - 2, py + 4, px + 33, py + 5, rail);
        paint.rect(px - 2, py + 26, px + 33, py + 27, rail);
    }
}

/// Wooden rails and posts joining the fence to its neighbouring fences and walls.
fn draw_fence(paint: &mut Painter, grid: &Grid, x: i64, y: i64) {
    let c = CELL as i64;
    let joins = |dx: i64, dy: i64| {
        grid.at(x + dx, y + dy)
            .is_some_and(|n| n == "fence" || is_wallish(n))
    };
    let mut links: Vec<(i64, i64)> = [(1, 0), (-1, 0), (0, 1), (0, -1)]
        .into_iter()
        .filter(|&(dx, dy)| joins(dx, dy))
        .collect();
    if links.is_empty() {
        links = vec![(1, 0), (-1, 0)];
    }
    let (fx, fy) = (x * c + 16, y * c + 18);
    for &(dx, dy) in &links {
        let (ex, ey) = (fx + dx * 16, fy + dy * 16);
        let top = [150, 106, 60, 255];
        paint.rect(fx.min(ex), fy.min(ey) - 3, fx.max(ex), fy.max(ey) - 3, top);
        paint.rect(
            fx.min(ex),
            fy.min(ey),
            fx.max(ex),
            fy.max(ey),
            [110, 76, 40, 255],
        );
    }
    let posts =
        std::iter::once((fx, fy)).chain(links.iter().map(|&(dx, dy)| (fx + dx * 8, fy + dy * 8)));
    for (qx, qy) in posts.collect::<Vec<_>>() {
        paint.rect(qx - 1, qy - 7, qx, qy + 1, [84, 56, 30, 255]);
        paint.put(qx - 1, qy - 7, [170, 130, 80, 255]);
    }
}

/// Drawing in map coordinates onto the computed area.
struct Painter<'a> {
    canvas: &'a mut RgbaImage,
    area: Rect,
}

impl Painter<'_> {
    fn put(&mut self, x: i64, y: i64, c: [u8; 4]) {
        if self.area.contains(x, y) {
            let i = (y - self.area.y0) as usize * self.canvas.width + (x - self.area.x0) as usize;
            self.canvas.pixels[i] = over(self.canvas.pixels[i], c);
        }
    }

    /// A filled rectangle, corners inclusive.
    fn rect(&mut self, x0: i64, y0: i64, x1: i64, y1: i64, c: [u8; 4]) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                self.put(x, y, c);
            }
        }
    }

    /// A filled ellipse inside the box, corners inclusive.
    fn ellipse(&mut self, x0: i64, y0: i64, x1: i64, y1: i64, c: [u8; 4]) {
        let (cx, cy) = ((x0 + x1) as f32 / 2.0, (y0 + y1) as f32 / 2.0);
        let rx = ((x1 - x0) as f32 / 2.0).max(0.5);
        let ry = ((y1 - y0) as f32 / 2.0).max(0.5);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let (dx, dy) = ((x as f32 - cx) / rx, (y as f32 - cy) / ry);
                if dx * dx + dy * dy <= 1.0 {
                    self.put(x, y, c);
                }
            }
        }
    }

    fn sprite(&mut self, img: &RgbaImage, x: i64, y: i64) {
        self.canvas.blend(img, x - self.area.x0, y - self.area.y0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEGEND: &[(char, &str)] = &[
        ('.', "plain"),
        (',', "grass"),
        ('T', "forest"),
        ('~', "river"),
        ('=', "bridge"),
        ('^', "mountain"),
        ('C', "cliff"),
        ('#', "wall"),
        ('G', "closed_gate"),
        ('_', "castle"),
        ('h', "house"),
        ('v', "village"),
        ('f', "fort"),
        ('b', "barracks"),
        ('g', "granary"),
        ('$', "treasury"),
        ('|', "fence"),
        ('w', "wasteland"),
    ];

    fn sample() -> (Vec<&'static str>, usize, usize) {
        let rows = [
            "CCCCww..TTT~..",
            "CCCww,,.TT~~..",
            "..w,,..h.~~.v.",
            "..^^...b.=..|.",
            "#G##..f.~~..|.",
            "_h_#...~~..$g.",
            "__$#..~~....TT",
        ];
        let cells = rows
            .iter()
            .flat_map(|r| r.chars())
            .map(|ch| LEGEND.iter().find(|(c, _)| *c == ch).expect("legend").1)
            .collect();
        (cells, rows[0].len(), rows.len())
    }

    #[test]
    fn the_atlases_hold_every_sprite_the_renderer_uses() {
        let objects = objects().unwrap();
        for name in [
            "tree0",
            "tree1",
            "broadleaf",
            "peak_big0",
            "peak_big1",
            "peak_big2",
            "peak_small0",
            "peak_small1",
            "peak_small2",
            "house_blue",
            "house_red",
            "house_brown",
            "field",
            "tent",
            "gatehouse",
            "granary",
            "treasury",
            "gate",
        ] {
            assert!(objects.contains_key(name), "{name}");
        }
        assert!(!unit_frames().unwrap().is_empty());
    }

    #[test]
    fn a_map_picture_is_32_px_per_cell_and_opaque() {
        let (cells, w, h) = sample();
        let img = render_map(&cells, w, h, 7).unwrap();
        assert_eq!((img.width, img.height), (w * CELL, h * CELL));
        assert!(img.pixels.iter().all(|p| p[3] == 255));
    }

    #[test]
    fn a_window_is_the_same_pixels_as_that_part_of_the_whole_map() {
        let (cells, w, h) = sample();
        let whole = render_map(&cells, w, h, 7).unwrap();
        for (x, y) in (0..h).flat_map(|y| (0..w).map(move |x| (x, y))) {
            let part = render_cells(&cells, w, h, 7, (x, y, 1, 1)).unwrap();
            assert_eq!(
                part,
                whole.crop(x * CELL, y * CELL, CELL, CELL),
                "cell {x},{y}"
            );
        }
    }

    #[test]
    fn rendering_is_deterministic_and_the_seed_matters() {
        let (cells, w, h) = sample();
        let a = render_map(&cells, w, h, 7).unwrap();
        assert_eq!(a, render_map(&cells, w, h, 7).unwrap());
        assert_ne!(a, render_map(&cells, w, h, 8).unwrap());
    }

    #[test]
    fn opening_a_gate_changes_the_gate_cell_and_nothing_far_away() {
        let (mut cells, w, h) = sample();
        let before = render_map(&cells, w, h, 7).unwrap();
        cells[4 * w + 1] = "plain";
        let after = render_map(&cells, w, h, 7).unwrap();
        let at = |img: &RgbaImage, x: usize, y: usize| img.crop(x * CELL, y * CELL, CELL, CELL);
        assert_ne!(at(&before, 1, 4), at(&after, 1, 4));
        assert_eq!(at(&before, 10, 0), at(&after, 10, 0));
        // the opened gate between two walls is paved, not a green square
        let gate = at(&after, 1, 4);
        let green = gate.pixels.iter().filter(|p| p[1] > p[0] + 30).count();
        assert!(green < gate.pixels.len() / 10, "{green} green pixels");
    }

    #[test]
    fn unknown_terrain_is_listed() {
        let (cells, _, _) = sample();
        assert!(unknown_terrain(cells.iter().copied()).is_empty());
        let odd = ["plain", "road", "gate", "wall", "road"];
        assert_eq!(
            unknown_terrain(odd).into_iter().collect::<Vec<_>>(),
            ["gate", "road"]
        );
    }

    #[test]
    fn bad_sizes_are_errors() {
        let (cells, w, h) = sample();
        assert!(render_map(&cells, w + 1, h, 1).is_err());
        assert!(render_cells(&cells, w, h, 1, (w - 1, 0, 2, 1)).is_err());
    }

    #[test]
    fn unit_sheets_mirror_the_middle_columns_and_alternate_frames() {
        let frames = unit_frames().unwrap();
        let sheet = unit_sheet("archer_enemy").unwrap().expect("sheet");
        assert_eq!(
            (sheet.width, sheet.height),
            (4 * UNIT_FRAME, 6 * UNIT_FRAME)
        );
        let f0 = &frames["archer_enemy_0"];
        let f1 = &frames["archer_enemy_1"];
        let cell = |col: usize, row: usize| sheet.crop(col * 32, row * 32, 32, 32);
        assert_eq!(&cell(0, 0), f0);
        assert_eq!(&cell(3, 1), f1);
        assert_eq!(cell(1, 0), f0.flipped());
        assert_eq!(cell(2, 5), f1.flipped());
        assert!(unit_sheet("no_such_sprite_player").unwrap().is_none());
    }

    #[test]
    fn a_noise_field_is_the_plain_noise() {
        let area = Rect {
            x0: 37,
            y0: 70,
            w: 50,
            h: 41,
        };
        let field = NoiseField::new(5, 2, 9, area);
        for (px, py) in [(0, 0), (49, 40), (13, 7), (8, 30)] {
            let plain = noise(5, 2, area.x0 + px as i64, area.y0 + py as i64, 9);
            assert!((field.at(px, py) - plain).abs() < 1e-6);
        }
    }

    #[test]
    fn a_png_round_trips() {
        let (cells, w, h) = sample();
        let img = render_cells(&cells, w, h, 3, (2, 2, 3, 2)).unwrap();
        assert_eq!(decode_png(&img.to_png().unwrap()).unwrap(), img);
    }
}
