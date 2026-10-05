//! Battle camera: which part of the map the viewport shows.
//!
//! Positions are map pixels (with tiles `T` pixels wide, tile `(x, y)` covers `Tx .. Tx + T`;
//! `T` is the tileset's tile size). The camera position is the map pixel shown at the viewport's
//! top-left corner. Along an axis where the map is smaller than
//! the viewport the map is centred; otherwise the position is clamped so the view never leaves
//! the map. Pans towards a target are smoothed; direct pans (drag, edge scrolling, wheel) move
//! immediately.

use hero_core::geom::Pos;
use macroquad::prelude::*;

/// Pan speed of mouse edge scrolling, in virtual pixels per second.
pub const EDGE_PAN_SPEED: f32 = 220.0;
/// Width of the edge scrolling zone, in virtual pixels.
pub const EDGE_ZONE: f32 = 6.0;
/// Seconds a resting pointer keeps edge scrolling in native builds. macroquad reports no
/// "pointer left the window" event, so a pointer that left the window across an edge stays at
/// its last position inside the zone; scrolling only while the pointer moved recently makes the
/// map stop instead of scrolling until the pointer returns. One rest still scrolls 220 pixels,
/// more than any map of the base pack can scroll (128). The browser reports leaving the canvas
/// (`platform::pointer_left`), so there the rest time is not used.
pub const EDGE_HOLD: f32 = 1.0;

#[derive(Debug, Clone, PartialEq)]
pub struct Camera {
    /// Screen rectangle (virtual pixels) the map is drawn into.
    pub viewport: Rect,
    /// Map size in pixels.
    pub map_size: Vec2,
    /// Tile size in pixels.
    pub tile: f32,
    pos: Vec2,
    target: Option<Vec2>,
    /// Drawing offset (canvas pixels) while the map shakes (D25 X5). It moves what is drawn
    /// through [`Camera::to_screen`] only: picking a tile and the scrolling decisions ignore it.
    pub shake: Vec2,
}

/// Clamp one axis: centre when the map is smaller than the view, else keep the view inside.
pub fn clamp_axis(pos: f32, view: f32, map: f32) -> f32 {
    if map <= view {
        -((view - map) / 2.0).floor()
    } else {
        pos.clamp(0.0, map - view)
    }
}

impl Camera {
    /// A camera showing a `map_size` pixel map of `tile` pixel tiles in `viewport`.
    pub fn new(viewport: Rect, map_size: Vec2, tile: f32) -> Camera {
        let mut c = Camera {
            viewport,
            map_size,
            tile,
            pos: Vec2::ZERO,
            target: None,
            shake: Vec2::ZERO,
        };
        c.pos = c.clamped(Vec2::ZERO);
        c
    }

    fn clamped(&self, p: Vec2) -> Vec2 {
        vec2(
            clamp_axis(p.x, self.viewport.w, self.map_size.x),
            clamp_axis(p.y, self.viewport.h, self.map_size.y),
        )
    }

    /// Shake the drawing by `offset` (canvas pixels), as far as the map still covers the parts of
    /// the viewport it covers without shaking.
    ///
    /// Input: the scene's shake offset. Output: none; sets [`Camera::shake`].
    ///
    /// Why limit it instead of clipping the drawing: at a map edge a full shake would uncover a
    /// strip of the screen's backdrop beside the map, which a pack without a battle frame does
    /// not cover. Limited per side, the map moves only inward there and still shakes.
    pub fn set_shake(&mut self, offset: Vec2) {
        let pos = self.pos();
        let axis = |s: f32, view_at: f32, view: f32, at: f32, map: f32| {
            // Where the map's two edges are drawn without the shake.
            let (first, last) = (view_at - at, view_at - at + map);
            let most = if first <= view_at {
                view_at - first
            } else {
                f32::INFINITY
            };
            let least = if last >= view_at + view {
                view_at + view - last
            } else {
                f32::NEG_INFINITY
            };
            s.clamp(least.min(0.0), most.max(0.0))
        };
        let vp = self.viewport;
        self.shake = vec2(
            axis(offset.x, vp.x, vp.w, pos.x, self.map_size.x),
            axis(offset.y, vp.y, vp.h, pos.y, self.map_size.y),
        );
    }

    /// Current position (map pixel at the viewport's top-left), rounded to whole pixels.
    pub fn pos(&self) -> Vec2 {
        self.pos.round()
    }

    /// Whether a smooth pan is in progress.
    #[cfg(test)]
    pub fn is_panning(&self) -> bool {
        self.target.is_some()
    }

    /// Screen position of a map pixel, as drawn (shake included).
    pub fn to_screen(&self, map_px: Vec2) -> Vec2 {
        self.to_screen_still(map_px) + self.shake
    }

    /// Screen position of a map pixel without the shake.
    fn to_screen_still(&self, map_px: Vec2) -> Vec2 {
        vec2(self.viewport.x, self.viewport.y) + map_px - self.pos()
    }

    /// Screen position of a map position measured in tiles (fractional between tiles).
    pub fn map_to_screen(&self, tiles: Vec2) -> Vec2 {
        self.to_screen(tiles * self.tile)
    }

    /// Screen position of a tile's top-left corner.
    pub fn tile_screen(&self, p: Pos) -> Vec2 {
        self.to_screen(self.tile_px(p))
    }

    /// Map pixel of a tile's top-left corner.
    fn tile_px(&self, p: Pos) -> Vec2 {
        vec2(p.x as f32, p.y as f32) * self.tile
    }

    /// Tile under a screen point, if it is inside the viewport and the map.
    pub fn tile_at(&self, screen: Vec2) -> Option<Pos> {
        if !self.viewport.contains(screen) {
            return None;
        }
        let m = screen - vec2(self.viewport.x, self.viewport.y) + self.pos();
        if m.x < 0.0 || m.y < 0.0 || m.x >= self.map_size.x || m.y >= self.map_size.y {
            return None;
        }
        Some(Pos::new((m.x / self.tile) as i32, (m.y / self.tile) as i32))
    }

    /// Jump immediately (clamped), cancelling a smooth pan.
    pub fn set_pos(&mut self, p: Vec2) {
        self.pos = self.clamped(p);
        self.target = None;
    }

    /// Move by a screen delta (drag / edge scrolling), cancelling a smooth pan. A pan the map
    /// edge stops entirely changes nothing, so it does not cancel a smooth pan either (edge
    /// scrolling against the edge must not undo keyboard tracking).
    pub fn pan(&mut self, delta: Vec2) {
        if self.clamped(self.pos + delta) != self.pos {
            self.set_pos(self.pos + delta);
        }
    }

    /// Smoothly centre on a tile.
    pub fn center_on(&mut self, p: Pos) {
        let c = self.tile_px(p) + vec2(self.tile, self.tile) / 2.0;
        self.target = Some(self.clamped(c - vec2(self.viewport.w, self.viewport.h) / 2.0));
    }

    /// Centre on a tile immediately.
    pub fn snap_to(&mut self, p: Pos) {
        self.center_on(p);
        if let Some(t) = self.target.take() {
            self.pos = t;
        }
    }

    /// Smoothly scroll just enough to keep a tile `margin` tiles away from the view edges.
    pub fn keep_visible(&mut self, p: Pos, margin: f32) {
        let base = self.target.unwrap_or(self.pos);
        let want = self.clamped(visible_pos(
            base,
            vec2(self.viewport.w, self.viewport.h),
            self.tile_px(p),
            self.tile,
            margin * self.tile,
        ));
        if (want - base).length() > 0.25 {
            self.target = Some(want);
        }
    }

    /// Whether a tile is fully inside the view (at the current position).
    pub fn is_visible(&self, p: Pos) -> bool {
        let s = self.to_screen_still(self.tile_px(p));
        let v = self.viewport;
        s.x >= v.x && s.y >= v.y && s.x + self.tile <= v.right() && s.y + self.tile <= v.bottom()
    }

    /// Advance a smooth pan.
    pub fn update(&mut self, dt: f32) {
        if let Some(t) = self.target {
            let k = (dt * 10.0).clamp(0.0, 1.0);
            self.pos += (t - self.pos) * k;
            if (t - self.pos).length() < 0.5 {
                self.pos = t;
                self.target = None;
            }
        }
    }
}

/// Camera position that shows the `size` pixel tile at map pixel `tile` with at least `margin`
/// pixels to the view edges, moving as little as possible from `pos`.
pub fn visible_pos(pos: Vec2, view: Vec2, tile: Vec2, size: f32, margin: f32) -> Vec2 {
    let axis = |p: f32, v: f32, t: f32| {
        let m = margin.min(((v - size) / 2.0).max(0.0));
        if t - m < p {
            t - m
        } else if t + size + m > p + v {
            t + size + m - v
        } else {
            p
        }
    };
    vec2(axis(pos.x, view.x, tile.x), axis(pos.y, view.y, tile.y))
}

/// Edge-scrolling direction for a pointer at `p` inside `viewport` that has not moved for
/// `rest` seconds (each axis -1, 0 or 1; none once it rested [`EDGE_HOLD`]).
pub fn edge_direction(viewport: Rect, p: Vec2, rest: f32) -> Vec2 {
    if rest >= EDGE_HOLD || !viewport.contains(p) {
        return Vec2::ZERO;
    }
    let axis = |v: f32, lo: f32, hi: f32| {
        if v < lo + EDGE_ZONE {
            -1.0
        } else if v >= hi - EDGE_ZONE {
            1.0
        } else {
            0.0
        }
    };
    vec2(
        axis(p.x, viewport.x, viewport.right()),
        axis(p.y, viewport.y, viewport.bottom()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIEW: Rect = Rect {
        x: 0.0,
        y: 14.0,
        w: 480.0,
        h: 256.0,
    };

    /// At a map edge the shake moves the map inward only, so no backdrop shows beside it; away
    /// from the edges and on a map smaller than the view it shakes fully.
    #[test]
    fn the_shake_never_uncovers_the_screen_beside_the_map() {
        // Larger than the view both ways, scrolled to the top-left corner.
        let mut c = Camera::new(VIEW, vec2(640.0, 640.0), 16.0);
        c.set_shake(vec2(3.0, 3.0));
        assert_eq!(c.shake, vec2(0.0, 0.0));
        c.set_shake(vec2(-3.0, -3.0));
        assert_eq!(c.shake, vec2(-3.0, -3.0));
        // In the middle: both ways.
        c.set_pos(vec2(64.0, 64.0));
        c.set_shake(vec2(3.0, -3.0));
        assert_eq!(c.shake, vec2(3.0, -3.0));
        // At the bottom-right corner: only towards it.
        c.set_pos(vec2(1000.0, 1000.0));
        c.set_shake(vec2(-3.0, 3.0));
        assert_eq!(c.shake, vec2(0.0, 3.0));
        c.set_shake(vec2(3.0, -3.0));
        assert_eq!(c.shake, vec2(3.0, 0.0));
        // Narrower than the view: centred with the backdrop beside it anyway, so it shakes.
        let mut c = Camera::new(VIEW, vec2(384.0, 288.0), 16.0);
        c.set_shake(vec2(3.0, 0.0));
        assert_eq!(c.shake.x, 3.0);
        c.set_shake(vec2(-3.0, 0.0));
        assert_eq!(c.shake.x, -3.0);
    }

    #[test]
    fn small_axes_are_centred_large_axes_clamped() {
        // 24x18 tiles: narrower than the view, taller than it.
        let mut c = Camera::new(VIEW, vec2(384.0, 288.0), 16.0);
        assert_eq!(c.pos(), vec2(-48.0, 0.0));
        c.set_pos(vec2(100.0, 100.0));
        assert_eq!(c.pos(), vec2(-48.0, 32.0));
        c.set_pos(vec2(0.0, -50.0));
        assert_eq!(c.pos(), vec2(-48.0, 0.0));
        assert_eq!(clamp_axis(10.0, 100.0, 50.0), -25.0);
        assert_eq!(clamp_axis(-10.0, 100.0, 300.0), 0.0);
        assert_eq!(clamp_axis(250.0, 100.0, 300.0), 200.0);
    }

    #[test]
    fn tile_screen_mapping_round_trips() {
        let mut c = Camera::new(VIEW, vec2(640.0, 640.0), 16.0);
        c.set_pos(vec2(32.0, 16.0));
        let p = Pos::new(5, 7);
        let s = c.tile_screen(p);
        assert_eq!(s, vec2(5.0 * 16.0 - 32.0, 14.0 + 7.0 * 16.0 - 16.0));
        assert_eq!(c.tile_at(s + vec2(3.0, 3.0)), Some(p));
        assert_eq!(c.map_to_screen(vec2(5.5, 7.0)), s + vec2(8.0, 0.0));
        // Above the viewport (the HUD bar) and outside the map give nothing.
        assert_eq!(c.tile_at(vec2(10.0, 5.0)), None);
        let small = Camera::new(VIEW, vec2(160.0, 160.0), 16.0);
        assert_eq!(small.tile_at(vec2(5.0, 20.0)), None);
    }

    #[test]
    fn bigger_tiles_scale_every_mapping() {
        // The same 40x40 tile map with 32 pixel tiles.
        let mut c = Camera::new(VIEW, vec2(1280.0, 1280.0), 32.0);
        c.set_pos(vec2(64.0, 32.0));
        let p = Pos::new(5, 7);
        let s = c.tile_screen(p);
        assert_eq!(s, vec2(5.0 * 32.0 - 64.0, 14.0 + 7.0 * 32.0 - 32.0));
        assert_eq!(c.tile_at(s + vec2(31.0, 31.0)), Some(p));
        assert_eq!(c.tile_at(s + vec2(32.0, 0.0)), Some(Pos::new(6, 7)));
        assert_eq!(c.map_to_screen(vec2(5.5, 7.0)), s + vec2(16.0, 0.0));
        c.snap_to(Pos::new(20, 20));
        let centre = c.tile_screen(Pos::new(20, 20)) + vec2(16.0, 16.0);
        assert!((centre - vec2(VIEW.x + VIEW.w / 2.0, VIEW.y + VIEW.h / 2.0)).length() < 1.0);
        assert!(c.is_visible(Pos::new(20, 20)));
        assert!(!c.is_visible(Pos::new(0, 0)));
    }

    #[test]
    fn smooth_pans_reach_their_target() {
        let mut c = Camera::new(VIEW, vec2(1600.0, 1600.0), 16.0);
        c.center_on(Pos::new(50, 50));
        assert!(c.is_panning());
        for _ in 0..200 {
            c.update(1.0 / 60.0);
        }
        assert!(!c.is_panning());
        assert!(c.is_visible(Pos::new(50, 50)));
        let centre = c.tile_screen(Pos::new(50, 50)) + vec2(8.0, 8.0);
        assert!((centre - vec2(VIEW.x + VIEW.w / 2.0, VIEW.y + VIEW.h / 2.0)).length() < 1.0);
        // Snapping is immediate and clamped at the map corner.
        c.snap_to(Pos::new(0, 0));
        assert_eq!(c.pos(), Vec2::ZERO);
    }

    #[test]
    fn keep_visible_moves_minimally() {
        let view = vec2(480.0, 256.0);
        // Already visible with margin: unchanged.
        assert_eq!(
            visible_pos(Vec2::ZERO, view, vec2(160.0, 96.0), 16.0, 32.0),
            Vec2::ZERO
        );
        // Right of the view: scroll so the tile sits `margin` from the right edge.
        let p = visible_pos(Vec2::ZERO, view, vec2(480.0, 96.0), 16.0, 32.0);
        assert_eq!(p, vec2(480.0 + 16.0 + 32.0 - 480.0, 0.0));
        // Above: scroll up.
        let p = visible_pos(vec2(0.0, 200.0), view, vec2(0.0, 180.0), 16.0, 16.0);
        assert_eq!(p.y, 164.0);
        let mut c = Camera::new(VIEW, vec2(1600.0, 1600.0), 16.0);
        c.keep_visible(Pos::new(3, 3), 2.0);
        assert!(!c.is_panning());
        c.keep_visible(Pos::new(60, 3), 2.0);
        assert!(c.is_panning());
    }

    #[test]
    fn edge_zones() {
        assert_eq!(edge_direction(VIEW, vec2(2.0, 100.0), 0.0), vec2(-1.0, 0.0));
        assert_eq!(
            edge_direction(VIEW, vec2(478.0, 268.0), 0.0),
            vec2(1.0, 1.0)
        );
        assert_eq!(edge_direction(VIEW, vec2(240.0, 140.0), 0.0), Vec2::ZERO);
        assert_eq!(edge_direction(VIEW, vec2(240.0, 5.0), 0.0), Vec2::ZERO);
    }

    /// Regression: a pointer that left the window stays at its last position in the zone; the
    /// map must stop scrolling once the pointer has rested a while.
    #[test]
    fn edge_scrolling_stops_for_a_resting_pointer() {
        let p = vec2(240.0, 268.0);
        assert_eq!(edge_direction(VIEW, p, EDGE_HOLD - 0.05), vec2(0.0, 1.0));
        assert_eq!(edge_direction(VIEW, p, EDGE_HOLD), Vec2::ZERO);
        assert_eq!(edge_direction(VIEW, p, 60.0), Vec2::ZERO);
        // One rest is enough to scroll across the tallest maps of the base pack (24 tiles of
        // 16 pixels).
        let tallest = 24.0 * 16.0 - VIEW.h;
        assert!(EDGE_PAN_SPEED * EDGE_HOLD > tallest);
    }

    #[test]
    fn pans_against_the_map_edge_keep_a_smooth_pan() {
        let mut c = Camera::new(VIEW, vec2(480.0, 384.0), 16.0);
        c.set_pos(vec2(0.0, 1000.0));
        let bottom = c.pos();
        assert_eq!(bottom, vec2(0.0, 384.0 - VIEW.h));
        // A smooth pan sideways and up; edge scrolling down (clamped) leaves it running.
        c.center_on(Pos::new(3, 3));
        assert!(c.is_panning());
        c.pan(vec2(0.0, 4.0));
        assert!(c.is_panning());
        assert_eq!(c.pos(), bottom);
        // A pan that moves the view takes over.
        c.pan(vec2(0.0, -4.0));
        assert!(!c.is_panning());
        assert_eq!(c.pos(), bottom - vec2(0.0, 4.0));
    }
}
