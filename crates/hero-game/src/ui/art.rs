//! Artwork with intentional fallbacks: drama backgrounds (`gfx/bg/<key>.png`), portrait cards
//! (`gfx/portraits/<key>.png`) and unit sprite icons (`gfx/units/<sprite>_<side>.png`).
//!
//! Packs may ship without some of this art (or it may still be loading), so every helper has a
//! fallback that looks like a deliberate design rather than a missing file:
//!
//! * a missing background is painted procedurally in the mood of its key (dusky sky and ridges for
//!   `field`, lantern-lit pillars for `palace`, stars for `night`, ...);
//! * a missing portrait uses `portraits/_unknown` (through [`crate::assets::Media::portrait`]),
//!   then a name card: the officer's name written vertically in gold, like a placard;
//! * a missing unit sheet shows the usual placeholder box.
//!
//! Hi-res art is drawn with a tint, so callers can fade (`alpha`) and dim (`light`) it.

use super::theme;
use crate::app::Ctx;
use crate::assets::AssetState;
use crate::gfx::{
    draw_placeholder, draw_sprite_frame, draw_texture_fit, fill_gradient_v, fill_rect, key_color,
    Align, Fit, FontId, TextStyle,
};
use hero_core::pack::Pack;
use macroquad::prelude::*;

/// Colour with its alpha multiplied by `a`.
fn fade(c: Color, a: f32) -> Color {
    Color::new(c.r, c.g, c.b, c.a * a.clamp(0.0, 1.0))
}

/// `c` scaled towards black by `k` (0 = black, 1 = unchanged).
fn shade(c: Color, k: f32) -> Color {
    Color::new(c.r * k, c.g * k, c.b * k, c.a)
}

/// Deterministic pseudo random value in 0..1 for an integer seed.
fn hash01(seed: u32) -> f32 {
    let mut x = seed.wrapping_mul(0x9E37_79B9) ^ 0x85EB_CA6B;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    (x & 0xFFFF) as f32 / 65535.0
}

fn key_seed(key: &str) -> u32 {
    key.bytes().fold(0x811c_9dc5u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
    })
}

/// Texture key of a drama background.
pub fn background_texture_key(key: &str) -> String {
    format!("bg/{key}")
}

/// Load state of a drama background (requests it when unknown).
pub fn background_state(ctx: &Ctx, key: &str) -> AssetState {
    ctx.media.texture_state(&background_texture_key(key))
}

/// How the procedural stand-in for a background is composed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scenery {
    /// Plain colour (`black`).
    Plain,
    /// Sky gradient with mountain ridges.
    Outdoor,
    /// Night sky: ridges and stars.
    Night,
    /// Dim hall with pillars.
    Indoor,
}

/// Sky/top colour, ground/bottom colour and composition of the stand-in for `key`. The keys of
/// `docs/ASSETS.md` have hand-picked moods; other keys get stable colours derived from the key.
pub fn fallback_palette(key: &str) -> (Color, Color, Scenery) {
    let c = Color::from_hex;
    match key {
        "black" => (BLACK, BLACK, Scenery::Plain),
        "night" => (c(0x060918), c(0x1b2044), Scenery::Night),
        "palace" => (c(0x4a1a1c), c(0x160608), Scenery::Indoor),
        "castle" => (c(0x3b3f4f), c(0x13151c), Scenery::Indoor),
        "camp" => (c(0x5a3418), c(0x140b06), Scenery::Outdoor),
        "village" => (c(0x6a6a8e), c(0x2e2616), Scenery::Outdoor),
        "field" => (c(0x6d8fb0), c(0x34401f), Scenery::Outdoor),
        "river" => (c(0x5a7fa6), c(0x172a40), Scenery::Outdoor),
        "mountain" => (c(0x5d6c84), c(0x1a1f2b), Scenery::Outdoor),
        "town" => (c(0x7a6a5a), c(0x2a2018), Scenery::Indoor),
        other => {
            let k = key_color(other);
            (
                Color::new(0.35 + k.r * 0.4, 0.35 + k.g * 0.4, 0.4 + k.b * 0.4, 1.0),
                shade(k, 0.35),
                Scenery::Outdoor,
            )
        }
    }
}

/// Height of a mountain ridge at `x`.
fn ridge(x: f32, seed: u32, base: f32, amp: f32) -> f32 {
    let p1 = hash01(seed) * std::f32::consts::TAU;
    let p2 = hash01(seed.wrapping_add(1)) * std::f32::consts::TAU;
    let p3 = hash01(seed.wrapping_add(2)) * std::f32::consts::TAU;
    base - amp
        * (0.55 * (x / 61.0 + p1).sin().abs()
            + 0.3 * (x / 27.0 + p2).sin()
            + 0.15 * (x / 11.0 + p3).sin())
}

/// A ridge filled down to the bottom of a `size` canvas.
fn draw_ridge(size: Vec2, seed: u32, base: f32, amp: f32, color: Color) {
    let step = 4.0;
    let mut x = 0.0;
    while x < size.x {
        let y0 = ridge(x, seed, base, amp);
        let y1 = ridge(x + step, seed, base, amp);
        draw_triangle(vec2(x, y0), vec2(x + step, y1), vec2(x, size.y), color);
        draw_triangle(
            vec2(x + step, y1),
            vec2(x + step, size.y),
            vec2(x, size.y),
            color,
        );
        x += step;
    }
}

/// Procedural stand-in for background `key` (see [`fallback_palette`]) over a `size` canvas. The
/// composition is designed for a 270-pixel-high canvas and scales vertically with the canvas
/// height; it spans the full width, with the pillars of a hall mirrored at both edges.
pub fn draw_fallback_background(size: Vec2, key: &str, alpha: f32) {
    let (top, bottom, scenery) = fallback_palette(key);
    let seed = key_seed(key);
    let (w, h) = (size.x, size.y);
    let k = h / 270.0;
    fill_gradient_v(
        Rect::new(0.0, 0.0, w, h),
        fade(top, alpha),
        fade(bottom, alpha),
    );
    match scenery {
        Scenery::Plain => {}
        Scenery::Outdoor | Scenery::Night => {
            if scenery == Scenery::Night {
                for i in 0..60u32 {
                    let x = (hash01(seed ^ (i * 3)) * w).floor();
                    let y = (hash01(seed ^ (i * 3 + 1)) * h * 0.55).floor();
                    let a = 0.25 + 0.6 * hash01(i * 7 + 3);
                    fill_rect(
                        Rect::new(x, y, 1.0, 1.0),
                        Color::new(1.0, 0.95, 0.85, a * alpha),
                    );
                }
            }
            let far = shade(
                Color::new(
                    (top.r + bottom.r) / 2.0,
                    (top.g + bottom.g) / 2.0,
                    (top.b + bottom.b) / 2.0,
                    1.0,
                ),
                0.8,
            );
            draw_ridge(size, seed, 165.0 * k, 55.0 * k, fade(far, alpha));
            draw_ridge(
                size,
                seed.wrapping_add(17),
                205.0 * k,
                40.0 * k,
                fade(shade(bottom, 0.9), alpha),
            );
            draw_ridge(
                size,
                seed.wrapping_add(41),
                238.0 * k,
                22.0 * k,
                fade(shade(bottom, 0.55), alpha),
            );
            // Haze over the far ridge.
            fill_gradient_v(
                Rect::new(0.0, 140.0 * k, w, 60.0 * k),
                fade(Color::new(top.r, top.g, top.b, 0.0), alpha),
                fade(Color::new(top.r, top.g, top.b, 0.25), alpha),
            );
        }
        Scenery::Indoor => {
            // Floor.
            fill_gradient_v(
                Rect::new(0.0, 190.0 * k, w, h - 190.0 * k),
                fade(shade(bottom, 1.6), alpha),
                fade(shade(bottom, 0.6), alpha),
            );
            // Pillars with a warm lantern glow between them, mirrored at both edges.
            let pillar = fade(shade(bottom, 0.7), alpha);
            let edge = fade(shade(top, 1.3), alpha * 0.5);
            for (i, x) in [36.0, 132.0, w - 150.0, w - 54.0].into_iter().enumerate() {
                fill_rect(Rect::new(x, 0.0, 18.0, 200.0 * k), pillar);
                fill_rect(Rect::new(x, 0.0, 2.0, 200.0 * k), edge);
                if i % 2 == 0 {
                    let gx = x + 48.0;
                    for (r, a) in [(26.0, 0.05), (16.0, 0.08), (8.0, 0.14)] {
                        draw_circle(gx, 70.0 * k, r, Color::new(1.0, 0.75, 0.4, a * alpha));
                    }
                }
            }
            // Beam across the top.
            fill_rect(Rect::new(0.0, 16.0 * k, w, 10.0 * k), pillar);
        }
    }
    // Vignette.
    fill_gradient_v(
        Rect::new(0.0, 0.0, w, 40.0),
        Color::new(0.0, 0.0, 0.0, 0.35 * alpha),
        Color::new(0.0, 0.0, 0.0, 0.0),
    );
    fill_gradient_v(
        Rect::new(0.0, h - 60.0, w, 60.0),
        Color::new(0.0, 0.0, 0.0, 0.0),
        Color::new(0.0, 0.0, 0.0, 0.45 * alpha),
    );
}

/// Draw background `key` over the whole canvas with opacity `alpha`: the image when it is loaded,
/// the procedural stand-in when it does not exist, nothing while it loads. Returns whether
/// something was drawn.
pub fn draw_background(ctx: &Ctx, key: &str, alpha: f32) -> bool {
    let tex_key = background_texture_key(key);
    match ctx.media.texture_state(&tex_key) {
        AssetState::Ready => match ctx.media.texture(&tex_key) {
            Some(t) => {
                draw_texture_fit(&t, ctx.gfx.screen(), Fit::Cover, fade(WHITE, alpha));
                true
            }
            None => false,
        },
        AssetState::Loading => false,
        AssetState::Missing => {
            draw_fallback_background(ctx.gfx.size(), key, alpha);
            true
        }
    }
}

/// Portrait art for a key.
pub enum PortraitArt {
    /// The portrait, or `portraits/_unknown` when the officer has none.
    Ready(Texture2D),
    /// Still loading.
    Loading,
    /// Neither the portrait nor `_unknown` exists.
    Missing,
}

/// Look up portrait `key` (falling back to `portraits/_unknown`).
pub fn portrait_art(ctx: &Ctx, key: &str) -> PortraitArt {
    let own = format!("portraits/{key}");
    match ctx.media.texture_state(&own) {
        // Right after the face setting changed: the face shown until then, while the new loads.
        AssetState::Loading => ctx
            .media
            .other_face(&own)
            .map_or(PortraitArt::Loading, PortraitArt::Ready),
        AssetState::Ready => ctx
            .media
            .texture(&own)
            .map_or(PortraitArt::Loading, PortraitArt::Ready),
        AssetState::Missing => match ctx.media.texture_state(crate::assets::UNKNOWN_PORTRAIT) {
            AssetState::Loading => PortraitArt::Loading,
            AssetState::Ready => ctx
                .media
                .texture(crate::assets::UNKNOWN_PORTRAIT)
                .map_or(PortraitArt::Loading, PortraitArt::Ready),
            AssetState::Missing => PortraitArt::Missing,
        },
    }
}

/// Head-and-shoulders silhouette inside `r` (the same shape as
/// [`super::window::draw_silhouette`], with an opacity).
pub fn draw_silhouette_alpha(r: Rect, alpha: f32) {
    let c = Color::from_hex(0x0a0f2c).with_alpha(0.9 * alpha.clamp(0.0, 1.0));
    let cx = r.x + r.w / 2.0;
    let head_r = r.w * 0.2;
    let head_y = r.y + r.h * 0.38;
    draw_circle(cx, head_y, head_r, c);
    fill_rect(
        Rect::new(
            cx - head_r * 0.35,
            head_y - head_r * 1.45,
            head_r * 0.7,
            head_r * 0.6,
        ),
        c,
    );
    let sy = head_y + head_r * 1.2;
    draw_triangle(
        vec2(cx, sy - head_r * 0.4),
        vec2(r.x + r.w * 0.05, r.bottom()),
        vec2(r.right() - r.w * 0.05, r.bottom()),
        c,
    );
    fill_rect(
        Rect::new(
            r.x + r.w * 0.12,
            sy + head_r * 0.6,
            r.w * 0.76,
            r.bottom() - sy - head_r * 0.6,
        ),
        c,
    );
}

/// What the name card of a portrait key shows: the officer's hanja name (or Korean name when it
/// has none) and the Korean name as a caption; keys that are not officers show themselves.
pub fn name_card_text(pack: Option<&Pack>, key: &str) -> (String, String) {
    let officer = pack.and_then(|p| {
        p.officer(key)
            .or_else(|| p.officers.values().find(|o| o.portrait_key() == key))
    });
    match officer {
        Some(o) if !o.hanja.is_empty() => (o.hanja.clone(), o.name.clone()),
        Some(o) => (o.name.clone(), String::new()),
        None => (key.to_string(), String::new()),
    }
}

/// Largest text size (3, 2 or 1) at which `chars` glyphs stacked vertically fit a column of
/// `width` × `height` virtual pixels (Galmuri glyphs are 12 px per size step).
pub fn vertical_text_size(chars: usize, width: f32, height: f32) -> u8 {
    for size in [3u8, 2] {
        let s = f32::from(size);
        if chars as f32 * (12.0 * s + 2.0) <= height && 12.0 * s <= width {
            return size;
        }
    }
    1
}

/// Stand-in for missing portrait art: the name written vertically in gold on a dark card, like
/// a name placard, with the Korean name below.
fn draw_name_card(ctx: &Ctx, key: &str, inner: Rect, alpha: f32, light: f32) {
    let gfx = &ctx.gfx;
    let (big, caption) = name_card_text(ctx.pack.as_deref(), key);
    fill_gradient_v(
        inner,
        fade(shade(Color::from_hex(0x2c2552), light), alpha),
        fade(shade(Color::from_hex(0x110d26), light), alpha),
    );
    let gold = shade(theme::TEXT_ACCENT, 0.35 + 0.65 * light);
    if inner.w > 20.0 && inner.h > 20.0 {
        crate::gfx::stroke_rect(
            Rect::new(inner.x + 3.0, inner.y + 3.0, inner.w - 6.0, inner.h - 6.0),
            fade(gold, 0.35 * alpha),
        );
    }
    let show_caption = !caption.is_empty() && inner.h >= 60.0;
    let caption_h = if show_caption { 14.0 } else { 0.0 };
    let area = Rect::new(
        inner.x + 4.0,
        inner.y + 4.0,
        inner.w - 8.0,
        inner.h - 8.0 - caption_h,
    );
    let chars: Vec<char> = big.chars().collect();
    let shadow = Color::new(0.0, 0.0, 0.0, 0.8 * alpha);
    if chars.len() <= 4 {
        let size = vertical_text_size(chars.len(), area.w, area.h);
        let step = 12.0 * f32::from(size) + 2.0;
        let lh = gfx.line_height(FontId::Main, size);
        let total = chars.len() as f32 * step;
        let mut y = (area.y + (area.h - total) / 2.0 - (lh - step) / 2.0).round();
        let style = TextStyle::main(fade(gold, alpha)).size(size).shadow(shadow);
        let mut buf = [0u8; 4];
        for c in chars {
            gfx.text_aligned(
                c.encode_utf8(&mut buf),
                area.x,
                y,
                area.w,
                Align::Center,
                style,
            );
            y += step;
        }
    } else {
        let lines = gfx.wrap(&big, FontId::Small, 1, area.w);
        let lh = gfx.line_height(FontId::Small, 1);
        let y = area.y + (area.h - lines.len() as f32 * lh) / 2.0;
        for (i, line) in lines.iter().enumerate() {
            gfx.text_aligned(
                line,
                area.x,
                y + i as f32 * lh,
                area.w,
                Align::Center,
                TextStyle::small(fade(gold, alpha)),
            );
        }
    }
    if show_caption {
        gfx.text_aligned(
            &caption,
            inner.x,
            inner.bottom() - caption_h - 2.0,
            inner.w,
            Align::Center,
            TextStyle::small(fade(shade(theme::TEXT, 0.4 + 0.6 * light), alpha)),
        );
    }
}

/// A framed portrait card (stage portraits of drama scenes, officer pages). `alpha` fades the
/// whole card, `light` dims it (1 = full brightness). Missing art is replaced by a name card.
pub fn draw_portrait_card(ctx: &Ctx, key: Option<&str>, r: Rect, alpha: f32, light: f32) {
    let alpha = alpha.clamp(0.0, 1.0);
    if alpha <= 0.0 || r.w < 4.0 || r.h < 4.0 {
        return;
    }
    let light = light.clamp(0.0, 1.0);
    fill_rect(
        Rect::new(r.x + 3.0, r.y + 3.0, r.w, r.h),
        Color::new(0.0, 0.0, 0.0, 0.45 * alpha),
    );
    let inner = Rect::new(r.x + 2.0, r.y + 2.0, r.w - 4.0, r.h - 4.0);
    fill_gradient_v(
        inner,
        fade(shade(Color::from_hex(0x2a3668), light), alpha),
        fade(shade(Color::from_hex(0x10163a), light), alpha),
    );
    match key.map(|k| portrait_art(ctx, k)) {
        Some(PortraitArt::Ready(t)) => {
            draw_texture_fit(
                &t,
                inner,
                Fit::Cover,
                Color::new(light, light, light, alpha),
            );
        }
        Some(PortraitArt::Loading) => {}
        Some(PortraitArt::Missing) => {
            if let Some(key) = key {
                draw_name_card(ctx, key, inner, alpha, light);
            }
        }
        None => {
            draw_silhouette_alpha(inner, alpha);
            if light < 1.0 {
                fill_rect(inner, Color::new(0.0, 0.0, 0.0, (1.0 - light) * alpha));
            }
        }
    }
    // Frame: dark outline, light bevel, dark inner line.
    let outline = fade(theme::BORDER_OUTER, alpha);
    let bevel = fade(shade(theme::BORDER_MID, 0.4 + 0.6 * light), alpha);
    crate::gfx::stroke_rect(r, outline);
    crate::gfx::stroke_rect(Rect::new(r.x + 1.0, r.y + 1.0, r.w - 2.0, r.h - 2.0), bevel);
    fill_rect(
        Rect::new(r.x + 1.0, r.y + 1.0, r.w - 2.0, 1.0),
        fade(shade(theme::BORDER_LIGHT, 0.5 + 0.5 * light), alpha),
    );
}

/// Frame size of a unit sheet: the layout of `docs/ASSETS.md` has 4 columns and 6 rows.
pub fn unit_frame_size(sheet_w: f32, sheet_h: f32) -> Vec2 {
    vec2((sheet_w / 4.0).floor(), (sheet_h / 6.0).floor())
}

/// Tallest unit frame [`draw_unit`] draws at full size. Packs with bigger unit sprites (e.g.
/// 48×48 frames for 32-pixel map tiles) get them shrunk by a whole factor, so the officer rows of
/// the camp keep their spacing.
pub const UNIT_ICON_MAX: f32 = 32.0;

/// Whole factor a unit frame `frame_h` pixels high is shrunk by in [`draw_unit`].
pub fn unit_icon_divisor(frame_h: f32) -> f32 {
    (frame_h / UNIT_ICON_MAX).ceil().max(1.0)
}

/// Draw a unit sprite (`units/<sprite>_<side>`) standing with its feet at `feet` (bottom centre of
/// the frame), facing down, walk frame `step` (0..4); frames taller than [`UNIT_ICON_MAX`] are
/// shrunk. A placeholder box when the sheet is missing.
pub fn draw_unit(ctx: &Ctx, sprite: &str, side: &str, feet: Vec2, step: u32) {
    let key = format!("units/{sprite}_{side}");
    match ctx.media.texture_state(&key) {
        AssetState::Ready => {
            if let Some(t) = ctx.media.texture(&key) {
                let frame = unit_frame_size(t.width(), t.height());
                if frame.x >= 1.0 && frame.y >= 1.0 {
                    let size = frame / unit_icon_divisor(frame.y);
                    let pos = vec2((feet.x - size.x / 2.0).round(), feet.y - size.y);
                    let cell = (0, step % 4);
                    if size == frame {
                        draw_sprite_frame(&t, frame, cell, pos, false, WHITE);
                    } else {
                        draw_texture_ex(
                            &t,
                            pos.x,
                            pos.y.round(),
                            WHITE,
                            DrawTextureParams {
                                dest_size: Some(size),
                                source: Some(Rect::new(
                                    cell.0 as f32 * frame.x,
                                    cell.1 as f32 * frame.y,
                                    frame.x,
                                    frame.y,
                                )),
                                ..Default::default()
                            },
                        );
                    }
                }
            }
        }
        AssetState::Loading => {}
        AssetState::Missing => {
            draw_placeholder(Rect::new(feet.x - 7.0, feet.y - 14.0, 14.0, 14.0), &key)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_backgrounds_have_moods() {
        assert_eq!(fallback_palette("black").2, Scenery::Plain);
        assert_eq!(fallback_palette("night").2, Scenery::Night);
        assert_eq!(fallback_palette("palace").2, Scenery::Indoor);
        assert_eq!(fallback_palette("field").2, Scenery::Outdoor);
        // Unknown keys get a stable palette.
        assert_eq!(fallback_palette("desert"), fallback_palette("desert"));
        assert_ne!(fallback_palette("desert").0, fallback_palette("snow").0);
    }

    #[test]
    fn name_cards() {
        let pack = crate::screens::camp::test_pack();
        assert_eq!(
            name_card_text(Some(&pack), "guan_yu"),
            ("關羽".to_string(), "관우".to_string())
        );
        assert_eq!(
            name_card_text(Some(&pack), "전령"),
            ("전령".to_string(), String::new())
        );
        assert_eq!(name_card_text(None, "x").0, "x");
        // Stage cards fit two big glyphs, message portraits smaller ones.
        assert_eq!(vertical_text_size(2, 96.0, 108.0), 3);
        assert_eq!(vertical_text_size(3, 56.0, 58.0), 1);
        assert_eq!(vertical_text_size(2, 56.0, 58.0), 2);
        assert_eq!(vertical_text_size(4, 10.0, 10.0), 1);
    }

    #[test]
    fn unit_frames_follow_the_sheet_layout() {
        assert_eq!(unit_frame_size(96.0, 144.0), vec2(24.0, 24.0));
        assert_eq!(unit_frame_size(128.0, 144.0), vec2(32.0, 24.0));
        assert_eq!(unit_frame_size(64.0, 96.0), vec2(16.0, 16.0));
    }

    #[test]
    fn big_unit_frames_shrink_to_icon_size() {
        // The base pack's 24-pixel frames (and anything up to 32) are drawn as they are.
        assert_eq!(unit_icon_divisor(24.0), 1.0);
        assert_eq!(unit_icon_divisor(32.0), 1.0);
        // Original-sized 48 / 64 / 96 pixel frames are halved or thirded.
        assert_eq!(unit_icon_divisor(48.0), 2.0);
        assert_eq!(unit_icon_divisor(64.0), 2.0);
        assert_eq!(unit_icon_divisor(96.0), 3.0);
    }
}
