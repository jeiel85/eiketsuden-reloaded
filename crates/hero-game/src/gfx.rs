//! Rendering foundation: the virtual canvas, fonts and text, and drawing helpers.
//!
//! # Coordinate model
//!
//! Everything is drawn in **virtual pixels** on a canvas whose size is the loaded pack's
//! presentation profile (`[presentation] canvas` in `pack.toml`, [`DEFAULT_CANVAS`] 480×270 when
//! the pack does not set it and before a pack is loaded). Screens read it from [`Gfx::size`] /
//! [`Gfx::screen`] and lay themselves out relative to its edges and centre; nothing assumes a
//! fixed size. The canvas is a render target of `W·S × H·S` real pixels, where `S` is the
//! largest integer scale that fits the window (at least 1), presented centred and letterboxed
//! with nearest filtering (only a window smaller than the canvas shrinks it, with linear
//! filtering). A `Camera2D` maps the virtual rectangle onto the render target, so screens never
//! deal with window sizes:
//!
//! * pixel art drawn at virtual size ends up scaled by exactly `S` — crisp at every window size;
//! * text is rasterised at `font_size · S` and drawn with `font_scale = 1/S` — one glyph texel
//!   per real pixel, so the Galmuri pixel fonts stay sharp;
//! * high resolution art (portraits, backgrounds) drawn into a virtual box keeps up to `S` times
//!   the virtual resolution.
//!
//! Positions may be fractional; text is snapped to real pixels (`1/S` virtual pixels).
//! Mouse and touch positions are converted with [`Canvas::screen_to_virtual`] (done once per
//! frame by [`crate::input::Input`]).
//!
//! # Text
//!
//! [`Gfx::text`] draws a single line whose **top-left** corner is `(x, y)`; the line box is
//! [`Gfx::line_height`] tall with the glyphs vertically centred in it. [`FontId::Main`] is
//! Galmuri11 (nominal 12 px, 16 px lines), [`FontId::Small`] Galmuri9 (nominal 10 px, 12 px
//! lines). [`TextStyle::size`] multiplies the nominal size by an integer for titles.
//! [`wrap_text`] breaks Korean text at spaces, and between syllables only when a single word is
//! longer than the line.

use hero_core::pack::Presentation;
use macroquad::color::hsl_to_rgb;
use macroquad::prelude::*;

/// Canvas size before a pack is loaded and for packs without `[presentation] canvas`: the base
/// pack's 480×270 (`hero_core::pack::DEFAULT_CANVAS`).
pub const DEFAULT_CANVAS: Vec2 = Vec2::new(
    hero_core::pack::DEFAULT_CANVAS[0] as f32,
    hero_core::pack::DEFAULT_CANVAS[1] as f32,
);
/// Largest side of the render target in real pixels (3840 for the default canvas at `S = 8`);
/// bigger windows are letterboxed. Keeps the texture within what every GPU supports.
pub const MAX_TARGET_PX: f32 = 4096.0;

/// Virtual canvas size of a loaded pack's presentation profile. `Pack::load` has already
/// checked it against `hero_core::pack::MIN_CANVAS` ..= `MAX_CANVAS`.
pub fn canvas_size(presentation: &Presentation) -> Vec2 {
    vec2(presentation.canvas[0] as f32, presentation.canvas[1] as f32)
}

/// Largest integer scale `S` such that `canvas · S` fits `screen_w × screen_h` (min 1), and no
/// side of the render target exceeds [`MAX_TARGET_PX`].
pub fn integer_scale(screen_w: f32, screen_h: f32, canvas: Vec2) -> u32 {
    let max_scale = (MAX_TARGET_PX / canvas.x.max(canvas.y)).floor().max(1.0) as u32;
    let s = (screen_w / canvas.x).min(screen_h / canvas.y).floor();
    if s.is_finite() && s >= 1.0 {
        (s as u32).min(max_scale)
    } else {
        1
    }
}

/// Where the `canvas · scale` render target is shown on a `screen_w × screen_h` window: centred,
/// at 1:1 when it fits, otherwise shrunk to fit (only possible for windows smaller than the
/// canvas).
pub fn present_rect(screen_w: f32, screen_h: f32, canvas: Vec2, scale: u32) -> Rect {
    let (w, h) = (canvas.x * scale as f32, canvas.y * scale as f32);
    let fit = (screen_w / w).min(screen_h / h).clamp(0.0, 1.0);
    let (w, h) = (w * fit, h * fit);
    Rect::new(
        ((screen_w - w) / 2.0).floor(),
        ((screen_h - h) / 2.0).floor(),
        w,
        h,
    )
}

/// The virtual canvas: its size, render target, camera and presentation rectangle.
pub struct Canvas {
    size: Vec2,
    target: RenderTarget,
    camera: Camera2D,
    /// A part of the canvas screens draw into as if it were the whole canvas ([`Canvas::set_view`]).
    view: Option<Rect>,
    scale: u32,
    present: Rect,
    screen: (f32, f32),
}

impl Canvas {
    /// A canvas of `size` virtual pixels (see [`canvas_size`]).
    pub fn new(size: Vec2) -> Canvas {
        let (sw, sh) = (screen_width(), screen_height());
        let scale = integer_scale(sw, sh, size);
        let (target, camera) = Self::make_target(size, scale);
        let canvas = Canvas {
            size,
            target,
            camera,
            view: None,
            scale,
            present: present_rect(sw, sh, size, scale),
            screen: (sw, sh),
        };
        canvas.apply_present_filter();
        canvas
    }

    fn make_target(size: Vec2, scale: u32) -> (RenderTarget, Camera2D) {
        let target = render_target(size.x as u32 * scale, size.y as u32 * scale);
        let mut camera = Camera2D::from_display_rect(Rect::new(0.0, 0.0, size.x, size.y));
        camera.render_target = Some(target.clone());
        (target, camera)
    }

    /// Virtual canvas size in pixels; the size of the view while one is set.
    pub fn size(&self) -> Vec2 {
        self.view.map_or(self.size, |v| v.size())
    }

    /// Size of the whole canvas, whether or not a view is set.
    pub fn full_size(&self) -> Vec2 {
        self.size
    }

    /// Draw into the part `view` of the canvas as if it were the whole canvas: positions start
    /// at its top-left corner and [`Canvas::size`] is its size (a screen shown inside a frame).
    /// Nothing is clipped, so the frame is drawn over it afterwards. `None` goes back to the
    /// whole canvas. Takes effect at once when drawing into the canvas.
    pub fn set_view(&mut self, view: Option<Rect>) {
        self.view = view;
        let area = view.unwrap_or(Rect::new(0.0, 0.0, self.size.x, self.size.y));
        let mut camera =
            Camera2D::from_display_rect(Rect::new(-area.x, -area.y, self.size.x, self.size.y));
        camera.render_target = Some(self.target.clone());
        self.camera = camera;
    }

    /// Switch to another virtual size (a pack's presentation profile); recreates the render
    /// target. Screens lay themselves out from [`Gfx::size`], so the next frame uses it.
    pub fn set_size(&mut self, size: Vec2) {
        if size == self.size {
            return;
        }
        let (sw, sh) = self.screen;
        self.size = size;
        self.scale = integer_scale(sw, sh, size);
        let (target, camera) = Self::make_target(size, self.scale);
        self.target = target;
        self.camera = camera;
        self.view = None;
        self.present = present_rect(sw, sh, size, self.scale);
        self.apply_present_filter();
    }

    /// Nearest filtering when the canvas is shown 1:1 (the normal case); linear filtering when a
    /// window smaller than the canvas (a phone in portrait) forces it to shrink, where nearest
    /// sampling would drop whole pixel rows and make text unreadable.
    fn apply_present_filter(&self) {
        let shrunk = self.present.w < self.target.texture.width();
        self.target.texture.set_filter(if shrunk {
            FilterMode::Linear
        } else {
            FilterMode::Nearest
        });
    }

    /// Follow window size changes; recreates the render target when the scale changes.
    /// Returns `true` when the scale changed. Call once per frame before drawing.
    pub fn update(&mut self) -> bool {
        let (sw, sh) = (screen_width(), screen_height());
        if (sw, sh) == self.screen {
            return false;
        }
        self.screen = (sw, sh);
        let scale = integer_scale(sw, sh, self.size);
        let changed = scale != self.scale;
        if changed {
            let (target, camera) = Self::make_target(self.size, scale);
            self.target = target;
            self.camera = camera;
            self.scale = scale;
        }
        self.present = present_rect(sw, sh, self.size, self.scale);
        self.apply_present_filter();
        changed
    }

    /// Integer scale `S` (real pixels per virtual pixel in the render target).
    pub fn scale(&self) -> u32 {
        self.scale
    }

    /// Where the canvas appears in the window, in window pixels.
    pub fn present_rect(&self) -> Rect {
        self.present
    }

    /// Route drawing into the canvas (virtual coordinates).
    pub fn begin(&self) {
        set_camera(&self.camera);
    }

    /// Draw the canvas to the window, filling the letterbox area with `letterbox`.
    pub fn present(&self, letterbox: Color) {
        set_default_camera();
        clear_background(letterbox);
        draw_texture_ex(
            &self.target.texture,
            self.present.x,
            self.present.y,
            WHITE,
            DrawTextureParams {
                dest_size: Some(vec2(self.present.w, self.present.h)),
                flip_y: true,
                ..Default::default()
            },
        );
    }

    /// Window pixel position -> virtual position (may lie outside the canvas).
    pub fn screen_to_virtual(&self, p: Vec2) -> Vec2 {
        screen_to_virtual(self.present, self.size, p)
    }
}

impl Default for Canvas {
    fn default() -> Self {
        Canvas::new(DEFAULT_CANVAS)
    }
}

/// Pure mapping used by [`Canvas::screen_to_virtual`]: `present` shows a `canvas` sized canvas.
pub fn screen_to_virtual(present: Rect, canvas: Vec2, p: Vec2) -> Vec2 {
    if present.w <= 0.0 || present.h <= 0.0 {
        return Vec2::ZERO;
    }
    vec2(
        (p.x - present.x) * canvas.x / present.w,
        (p.y - present.y) * canvas.y / present.h,
    )
}

// ----- fonts ---------------------------------------------------------------------------------

/// The two UI fonts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FontId {
    /// Galmuri11, nominal 12 px — UI and dialogue.
    Main,
    /// Galmuri9, nominal 10 px — small numbers and captions.
    Small,
}

impl FontId {
    /// Size the font was designed for, in virtual pixels.
    pub fn nominal_px(self) -> u16 {
        match self {
            FontId::Main => 12,
            FontId::Small => 10,
        }
    }

    /// Line box height at size 1, in virtual pixels.
    pub fn line_height(self) -> f32 {
        match self {
            FontId::Main => 16.0,
            FontId::Small => 12.0,
        }
    }

    /// Pack-relative file of the font.
    pub fn file(self) -> &'static str {
        match self {
            FontId::Main => hero_core::pack::FONT_FILES[0],
            FontId::Small => hero_core::pack::FONT_FILES[1],
        }
    }
}

struct LoadedFont {
    font: Font,
    /// Distance from the top of the line box to the baseline at size 1.
    baseline: f32,
}

impl LoadedFont {
    fn new(font: Font, id: FontId) -> LoadedFont {
        let nominal = id.nominal_px();
        // Reference glyphs: a Hangul syllable, a capital and a descender.
        let dims = measure_text("가Ag", Some(&font), nominal, 1.0);
        let pad = ((id.line_height() - dims.height) / 2.0).round().max(0.0);
        LoadedFont {
            font,
            baseline: (pad + dims.offset_y).round(),
        }
    }
}

/// The loaded UI fonts. Missing font files fall back to macroquad's built-in font (Latin only),
/// recorded in [`Fonts::missing`].
pub struct Fonts {
    main: LoadedFont,
    small: LoadedFont,
    missing: Vec<String>,
}

impl Fonts {
    /// Fonts before the pack fonts are loaded: macroquad's built-in font for both.
    pub fn fallback() -> Fonts {
        Fonts {
            main: LoadedFont::new(get_default_font(), FontId::Main),
            small: LoadedFont::new(get_default_font(), FontId::Small),
            missing: Vec::new(),
        }
    }

    /// Install a font from TTF bytes. On failure the fallback font stays and the error is
    /// recorded.
    pub fn install(&mut self, id: FontId, bytes: Result<&[u8], String>) {
        let result = bytes
            .and_then(|b| load_ttf_font_from_bytes(b).map_err(|e| format!("{}: {e}", id.file())));
        match result {
            Ok(mut font) => {
                font.set_filter(FilterMode::Nearest);
                let loaded = LoadedFont::new(font, id);
                match id {
                    FontId::Main => self.main = loaded,
                    FontId::Small => self.small = loaded,
                }
            }
            Err(e) => {
                macroquad::logging::error!("font {:?} unavailable, using fallback: {}", id, e);
                self.missing.push(e);
            }
        }
    }

    /// Font files that could not be loaded (the built-in fallback is used for them).
    pub fn missing(&self) -> &[String] {
        &self.missing
    }

    fn get(&self, id: FontId) -> &LoadedFont {
        match id {
            FontId::Main => &self.main,
            FontId::Small => &self.small,
        }
    }
}

// ----- text ----------------------------------------------------------------------------------

/// Horizontal alignment inside a box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

/// How a line of text is drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextStyle {
    pub font: FontId,
    /// Integer multiple of the nominal size (1 = 12 px for `Main`).
    pub size: u8,
    pub color: Color,
    /// 1-pixel drop shadow colour, drawn below-right.
    pub shadow: Option<Color>,
}

impl TextStyle {
    pub const fn main(color: Color) -> TextStyle {
        TextStyle {
            font: FontId::Main,
            size: 1,
            color,
            shadow: None,
        }
    }

    pub const fn small(color: Color) -> TextStyle {
        TextStyle {
            font: FontId::Small,
            size: 1,
            color,
            shadow: None,
        }
    }

    pub const fn size(mut self, size: u8) -> TextStyle {
        self.size = if size == 0 { 1 } else { size };
        self
    }

    pub const fn shadow(mut self, color: Color) -> TextStyle {
        self.shadow = Some(color);
        self
    }

    pub const fn color(mut self, color: Color) -> TextStyle {
        self.color = color;
        self
    }
}

/// Canvas plus fonts: everything needed to draw. Owned by [`crate::app::Ctx`].
pub struct Gfx {
    pub canvas: Canvas,
    pub fonts: Fonts,
}

impl Gfx {
    /// A [`DEFAULT_CANVAS`] sized canvas with the fallback fonts.
    pub fn new() -> Gfx {
        Gfx {
            canvas: Canvas::new(DEFAULT_CANVAS),
            fonts: Fonts::fallback(),
        }
    }

    /// Integer canvas scale `S`.
    pub fn scale(&self) -> u32 {
        self.canvas.scale()
    }

    /// Virtual canvas size in pixels (the loaded pack's presentation profile).
    pub fn size(&self) -> Vec2 {
        self.canvas.size()
    }

    /// The whole virtual canvas as a rectangle at the origin.
    pub fn screen(&self) -> Rect {
        let s = self.size();
        Rect::new(0.0, 0.0, s.x, s.y)
    }

    fn params(&self, font: FontId, size: u8, color: Color) -> TextParams<'_> {
        let s = self.scale();
        TextParams {
            font: Some(&self.fonts.get(font).font),
            font_size: font.nominal_px() * u16::from(size.max(1)) * s as u16,
            font_scale: 1.0 / s as f32,
            color,
            ..Default::default()
        }
    }

    /// Snap a virtual coordinate to the real pixel grid.
    pub fn snap(&self, v: f32) -> f32 {
        let s = self.scale() as f32;
        (v * s).round() / s
    }

    /// Height of a line box.
    pub fn line_height(&self, font: FontId, size: u8) -> f32 {
        font.line_height() * f32::from(size.max(1))
    }

    /// Width of `text` in virtual pixels.
    pub fn text_width(&self, text: &str, font: FontId, size: u8) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        let p = self.params(font, size, WHITE);
        measure_text(text, p.font, p.font_size, p.font_scale).width
    }

    /// Advance width of one character.
    pub fn char_width(&self, c: char, font: FontId, size: u8) -> f32 {
        let mut buf = [0u8; 4];
        self.text_width(c.encode_utf8(&mut buf), font, size)
    }

    /// Draw one line with its line box's top-left corner at `(x, y)`. Returns the width.
    pub fn text(&self, text: &str, x: f32, y: f32, style: TextStyle) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        let size = style.size.max(1);
        let baseline = self.fonts.get(style.font).baseline * f32::from(size);
        let (x, y) = (self.snap(x), self.snap(y + baseline));
        if let Some(shadow) = style.shadow {
            draw_text_ex(
                text,
                x + f32::from(size),
                y + f32::from(size),
                self.params(style.font, size, shadow),
            );
        }
        draw_text_ex(text, x, y, self.params(style.font, size, style.color)).width
    }

    /// Draw one line aligned inside `[x, x + w]`.
    pub fn text_aligned(&self, text: &str, x: f32, y: f32, w: f32, align: Align, style: TextStyle) {
        let tx = match align {
            Align::Left => x,
            Align::Center => {
                x + ((w - self.text_width(text, style.font, style.size)) / 2.0).round()
            }
            Align::Right => x + w - self.text_width(text, style.font, style.size),
        };
        self.text(text, tx, y, style);
    }

    /// Draw pre-wrapped lines; returns the total height.
    pub fn text_lines<S: AsRef<str>>(&self, lines: &[S], x: f32, y: f32, style: TextStyle) -> f32 {
        let lh = self.line_height(style.font, style.size);
        for (i, line) in lines.iter().enumerate() {
            self.text(line.as_ref(), x, y + lh * i as f32, style);
        }
        lh * lines.len() as f32
    }

    /// Wrap `text` to `max_width` virtual pixels (see [`wrap_text`]).
    pub fn wrap(&self, text: &str, font: FontId, size: u8, max_width: f32) -> Vec<String> {
        wrap_text(text, max_width, |c| self.char_width(c, font, size))
    }

    /// `text` cut to `max_width` virtual pixels with a trailing `…` when it is wider (see
    /// [`elide_text`]).
    pub fn elide(&self, text: &str, font: FontId, size: u8, max_width: f32) -> String {
        elide_text(text, max_width, |s| self.text_width(s, font, size))
    }

    /// `text` cut to `max_width` virtual pixels with a `…` in the middle when it is wider, so the
    /// two ends that tell similar names apart stay readable (see [`elide_middle_text`]).
    pub fn elide_middle(&self, text: &str, font: FontId, size: u8, max_width: f32) -> String {
        elide_middle_text(text, max_width, |s| self.text_width(s, font, size))
    }
}

impl Default for Gfx {
    fn default() -> Self {
        Gfx::new()
    }
}

/// Characters that may not start a line.
fn no_break_before(c: char) -> bool {
    matches!(
        c,
        '.' | ','
            | '!'
            | '?'
            | ':'
            | ';'
            | ')'
            | ']'
            | '}'
            | '…'
            | '」'
            | '』'
            | '》'
            | '〉'
            | '。'
            | '、'
            | '！'
            | '？'
            | '）'
            | '~'
            | '\''
            | '"'
            | '’'
            | '”'
    )
}

/// Characters that may not end a line.
fn no_break_after(c: char) -> bool {
    matches!(
        c,
        '(' | '[' | '{' | '「' | '『' | '《' | '〈' | '（' | '‘' | '“'
    )
}

/// Hangul, kana and CJK ideographs: scripts where a line may break between two characters.
pub fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x1100..=0x11FF       // Hangul Jamo
        | 0x3040..=0x30FF     // Hiragana, Katakana
        | 0x3130..=0x318F     // Hangul compatibility Jamo
        | 0x3400..=0x4DBF     // CJK extension A
        | 0x4E00..=0x9FFF     // CJK unified ideographs
        | 0xAC00..=0xD7A3     // Hangul syllables
        | 0xF900..=0xFAFF     // CJK compatibility ideographs
        | 0xFF00..=0xFFEF     // full-width forms
    )
}

fn can_break_between(prev: char, next: char) -> bool {
    !no_break_before(next) && !no_break_after(prev) && (is_cjk(prev) || is_cjk(next))
}

/// Characters that extend the character before them instead of standing on their own: combining
/// accent marks and the medial/final jamo of decomposed (NFD) Hangul. A cut between a base
/// character and its marks would leave a stray mark at the start of the next part.
pub fn is_combining(c: char) -> bool {
    matches!(c as u32,
        0x0300..=0x036F       // combining diacritical marks
        | 0x0483..=0x0489
        | 0x0591..=0x05BD
        | 0x05BF | 0x05C1..=0x05C2 | 0x05C4..=0x05C5 | 0x05C7
        | 0x0610..=0x061A
        | 0x064B..=0x065F | 0x0670
        | 0x06D6..=0x06DC | 0x06DF..=0x06E4 | 0x06E7..=0x06E8 | 0x06EA..=0x06ED
        | 0x0711 | 0x0730..=0x074A
        | 0x07A6..=0x07B0
        | 0x0900..=0x0903 | 0x093A..=0x094F | 0x0951..=0x0957 | 0x0962..=0x0963
        | 0x0981..=0x0983 | 0x09BC | 0x09BE..=0x09CD
        | 0x0A01..=0x0A03 | 0x0A3C | 0x0A3E..=0x0A4D
        | 0x0A81..=0x0A83 | 0x0ABC | 0x0ABE..=0x0ACD
        | 0x0B01..=0x0B03 | 0x0B3C | 0x0B3E..=0x0B4D
        | 0x0C00..=0x0C04 | 0x0C3E..=0x0C56
        | 0x0D00..=0x0D03 | 0x0D3B..=0x0D4D
        | 0x0E31 | 0x0E34..=0x0E3A | 0x0E47..=0x0E4E
        | 0x0EB1 | 0x0EB4..=0x0EBC | 0x0EC8..=0x0ECD
        | 0x0F18..=0x0F19 | 0x0F35 | 0x0F37 | 0x0F39 | 0x0F71..=0x0F84
        | 0x102B..=0x103E
        | 0x1160..=0x11FF     // Hangul Jungseong/Jongseong
        | 0x135D..=0x135F
        | 0x1712..=0x1714 | 0x1732..=0x1734
        | 0x1A17..=0x1A1B | 0x1AB0..=0x1AFF
        | 0x1B00..=0x1B04 | 0x1B34..=0x1B44
        | 0x1DC0..=0x1DFF     // combining diacritical marks supplement
        | 0x20D0..=0x20F0     // combining diacritical marks for symbols
        | 0x2CEF..=0x2CF1
        | 0x302A..=0x302F | 0x3099..=0x309A
        | 0xA66F..=0xA672 | 0xA674..=0xA67D | 0xA69E..=0xA69F
        | 0xA6F0..=0xA6F1
        | 0xFE00..=0xFE0F     // variation selectors
        | 0xFE20..=0xFE2F     // combining half marks
        | 0x101FD | 0x10A01..=0x10A0F | 0x10A38..=0x10A3F
        | 0x11000..=0x11002 | 0x11038..=0x11046
        | 0x1D165..=0x1D169 | 0x1D16D..=0x1D182
        | 0x1D1AA..=0x1D1AD
    )
}

/// The character [`elide_text`] and [`elide_middle_text`] put where they cut.
pub const ELLIPSIS: char = '…';

/// Byte offsets in `text` where a cut leaves whole characters: at the start and the end of the
/// text, and before every character that is not a combining mark.
fn cut_points(text: &str) -> Vec<usize> {
    let mut cuts: Vec<usize> = text
        .char_indices()
        .filter(|&(i, c)| i == 0 || !is_combining(c))
        .map(|(i, _)| i)
        .collect();
    cuts.push(text.len());
    cuts
}

/// `text` cut to `max_width` pixels with a trailing `…` when it is wider, trimmed at the end so a
/// cut that lands after a space does not leave a gap before the `…`.
///
/// `width` measures a whole slice, so the kerning between neighbouring characters counts as it
/// does when the text is drawn, and the cut never lands inside a character with combining marks
/// (the front part keeps them, the rest is dropped).
pub fn elide_text(text: &str, max_width: f32, mut width: impl FnMut(&str) -> f32) -> String {
    if width(text) <= max_width {
        return text.to_string();
    }
    let ellipsis = ELLIPSIS.to_string();
    let budget = max_width - width(&ellipsis);
    let mut end = 0;
    for &cut in cut_points(text).iter().skip(1) {
        if width(&text[..cut]) > budget {
            break;
        }
        end = cut;
    }
    let mut out = text[..end].trim_end().to_string();
    out.push(ELLIPSIS);
    out
}

/// `text` cut to `max_width` pixels with a `…` in the middle when it is wider: the front and the
/// back are kept, so two long names that differ only at their ends still read apart. Half the
/// room goes to the front, the rest to as much of the back as fits.
pub fn elide_middle_text(text: &str, max_width: f32, mut width: impl FnMut(&str) -> f32) -> String {
    if width(text) <= max_width {
        return text.to_string();
    }
    let ellipsis = ELLIPSIS.to_string();
    let budget = max_width - width(&ellipsis);
    if budget <= 0.0 {
        return ellipsis;
    }
    let cuts = cut_points(text);
    let mut front = 0;
    for &cut in cuts.iter().skip(1) {
        if cut == text.len() || width(&text[..cut]) > budget / 2.0 {
            break;
        }
        front = cut;
    }
    let room = budget - width(&text[..front]);
    // The widest suffix that still fits: a shorter one only gets wider (kerning aside).
    let mut back = text.len();
    for &cut in cuts.iter().rev() {
        if cut < front || width(&text[cut..]) > room {
            break;
        }
        back = cut;
    }
    let mut out = text[..front].trim_end().to_string();
    out.push(ELLIPSIS);
    out.push_str(text[back..].trim_start());
    out
}

/// Word wrap for Korean (and mixed Latin/Hanja) text.
///
/// * explicit `\n` always breaks (empty lines are kept);
/// * lines break at spaces whenever possible (the spaces are dropped);
/// * a word longer than the line breaks between syllables (Hangul/CJK) without splitting before
///   closing punctuation, and as a last resort between any two characters;
/// * `advance` returns the width of one character.
pub fn wrap_text(text: &str, max_width: f32, mut advance: impl FnMut(char) -> f32) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let chars: Vec<char> = paragraph
            .chars()
            .filter(|c| *c != '\r')
            .map(|c| if c == '\t' { ' ' } else { c })
            .collect();
        let mut start = 0;
        loop {
            // Skip spaces at the start of a continuation line.
            if start > 0 {
                while start < chars.len() && chars[start] == ' ' {
                    start += 1;
                }
            }
            let mut width = 0.0;
            let mut last_space: Option<usize> = None;
            let mut last_break: Option<usize> = None;
            let mut i = start;
            let mut cut: Option<(usize, usize)> = None; // (line end, next start)
            while i < chars.len() {
                let c = chars[i];
                let w = advance(c);
                if width + w > max_width && i > start {
                    // A space only counts once a word precedes it (not the indentation of a
                    // paragraph), otherwise the line would come out empty.
                    let usable_space =
                        last_space.filter(|&sp| chars[start..sp].iter().any(|&ch| ch != ' '));
                    cut = Some(if c == ' ' {
                        (i, i + 1)
                    } else if let Some(sp) = usable_space {
                        (sp, sp + 1)
                    } else if can_break_between(chars[i - 1], c) {
                        // The overflowing syllable itself starts the next line.
                        (i, i)
                    } else if let Some(b) = last_break {
                        (b, b)
                    } else {
                        (i, i)
                    });
                    break;
                }
                if c == ' ' {
                    last_space = Some(i);
                } else if i > start && can_break_between(chars[i - 1], c) {
                    last_break = Some(i);
                }
                width += w;
                i += 1;
            }
            match cut {
                Some((end, next)) => {
                    let line: String = chars[start..end].iter().collect();
                    lines.push(line.trim_end().to_string());
                    start = next;
                }
                None => {
                    let line: String = chars[start..].iter().collect();
                    lines.push(line.trim_end().to_string());
                    break;
                }
            }
        }
    }
    lines
}

// ----- drawing helpers -----------------------------------------------------------------------

/// Solid rectangle.
pub fn fill_rect(r: Rect, color: Color) {
    draw_rectangle(r.x, r.y, r.w, r.h, color);
}

/// 1-pixel outline drawn inside `r` (four filled strips, so it stays on the pixel grid).
pub fn stroke_rect(r: Rect, color: Color) {
    if r.w <= 0.0 || r.h <= 0.0 {
        return;
    }
    draw_rectangle(r.x, r.y, r.w, 1.0, color);
    draw_rectangle(r.x, r.y + r.h - 1.0, r.w, 1.0, color);
    draw_rectangle(r.x, r.y + 1.0, 1.0, r.h - 2.0, color);
    draw_rectangle(r.x + r.w - 1.0, r.y + 1.0, 1.0, r.h - 2.0, color);
}

/// Rectangle with a vertical colour gradient.
pub fn fill_gradient_v(r: Rect, top: Color, bottom: Color) {
    let vertices = [
        Vertex::new(r.x, r.y, 0.0, 0.0, 0.0, top),
        Vertex::new(r.x + r.w, r.y, 0.0, 1.0, 0.0, top),
        Vertex::new(r.x + r.w, r.y + r.h, 0.0, 1.0, 1.0, bottom),
        Vertex::new(r.x, r.y + r.h, 0.0, 0.0, 1.0, bottom),
    ];
    let indices: [u16; 6] = [0, 1, 2, 0, 2, 3];
    // SAFETY: `get_internal_gl` is only unsafe because it hands out the renderer while other
    // macroquad calls could also use it; we issue one self-contained geometry batch on the main
    // thread, exactly what `draw_rectangle` does internally.
    let gl = unsafe { get_internal_gl() };
    gl.quad_gl.texture(None);
    gl.quad_gl.draw_mode(DrawMode::Triangles);
    gl.quad_gl.geometry(&vertices, &indices);
}

/// Rectangle with a horizontal colour gradient.
pub fn fill_gradient_h(r: Rect, left: Color, right: Color) {
    let vertices = [
        Vertex::new(r.x, r.y, 0.0, 0.0, 0.0, left),
        Vertex::new(r.x + r.w, r.y, 0.0, 1.0, 0.0, right),
        Vertex::new(r.x + r.w, r.y + r.h, 0.0, 1.0, 1.0, right),
        Vertex::new(r.x, r.y + r.h, 0.0, 0.0, 1.0, left),
    ];
    let indices: [u16; 6] = [0, 1, 2, 0, 2, 3];
    // SAFETY: see `fill_gradient_v`.
    let gl = unsafe { get_internal_gl() };
    gl.quad_gl.texture(None);
    gl.quad_gl.draw_mode(DrawMode::Triangles);
    gl.quad_gl.geometry(&vertices, &indices);
}

/// Draw one frame of a sprite sheet laid out as a grid of `frame`-sized cells, with the frame's
/// top-left at `pos` (virtual pixels), at 1 virtual pixel per texel.
pub fn draw_sprite_frame(
    texture: &Texture2D,
    frame: Vec2,
    cell: (u32, u32),
    pos: Vec2,
    flip_x: bool,
    tint: Color,
) {
    let source = Rect::new(
        cell.0 as f32 * frame.x,
        cell.1 as f32 * frame.y,
        frame.x,
        frame.y,
    );
    draw_texture_ex(
        texture,
        pos.x.round(),
        pos.y.round(),
        tint,
        DrawTextureParams {
            dest_size: Some(frame),
            source: Some(source),
            flip_x,
            ..Default::default()
        },
    );
}

/// How [`draw_texture_fit`] maps an image into a box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fit {
    /// Whole image visible, centred, aspect kept (may leave empty bands).
    Contain,
    /// Box completely covered, aspect kept, overflow cropped evenly.
    Cover,
    /// Stretched to the box.
    Stretch,
}

/// Draw a texture into `dest` (virtual pixels). Used for high resolution art such as portraits
/// and backgrounds, which keep their detail on large canvases.
pub fn draw_texture_fit(texture: &Texture2D, dest: Rect, fit: Fit, tint: Color) {
    let (tw, th) = (texture.width(), texture.height());
    if tw <= 0.0 || th <= 0.0 || dest.w <= 0.0 || dest.h <= 0.0 {
        return;
    }
    let (source, target) = fit_rects(vec2(tw, th), dest, fit);
    draw_texture_ex(
        texture,
        target.x,
        target.y,
        tint,
        DrawTextureParams {
            dest_size: Some(vec2(target.w, target.h)),
            source: Some(source),
            ..Default::default()
        },
    );
}

/// Source rectangle (texels) and destination rectangle for [`draw_texture_fit`].
pub fn fit_rects(size: Vec2, dest: Rect, fit: Fit) -> (Rect, Rect) {
    let full = Rect::new(0.0, 0.0, size.x, size.y);
    match fit {
        Fit::Stretch => (full, dest),
        Fit::Contain => {
            let k = (dest.w / size.x).min(dest.h / size.y);
            let (w, h) = (size.x * k, size.y * k);
            (
                full,
                Rect::new(
                    dest.x + (dest.w - w) / 2.0,
                    dest.y + (dest.h - h) / 2.0,
                    w,
                    h,
                ),
            )
        }
        Fit::Cover => {
            let k = (dest.w / size.x).max(dest.h / size.y);
            let (sw, sh) = (dest.w / k, dest.h / k);
            (
                Rect::new((size.x - sw) / 2.0, (size.y - sh) / 2.0, sw, sh),
                dest,
            )
        }
    }
}

/// Stable colour derived from a key (for placeholders of missing art).
pub fn key_color(key: &str) -> Color {
    // FNV-1a, then pick a hue; saturation/value fixed so placeholders look alike.
    let mut h: u32 = 0x811c_9dc5;
    for b in key.bytes() {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    let hue = (h % 360) as f32 / 360.0;
    hsl_to_rgb(hue, 0.55, 0.45)
}

/// Coloured box with a cross, drawn in place of a missing sprite or image.
pub fn draw_placeholder(r: Rect, key: &str) {
    let c = key_color(key);
    fill_rect(r, Color::new(c.r, c.g, c.b, 0.85));
    stroke_rect(r, Color::new(0.0, 0.0, 0.0, 0.8));
    let inner = Rect::new(r.x + 1.0, r.y + 1.0, r.w - 2.0, r.h - 2.0);
    let light = Color::new(1.0, 1.0, 1.0, 0.45);
    draw_line(inner.x, inner.y, inner.right(), inner.bottom(), 1.0, light);
    draw_line(inner.right(), inner.y, inner.x, inner.bottom(), 1.0, light);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wrap_mono(text: &str, width: f32) -> Vec<String> {
        // Monospace: every character is 1 wide except Hangul/Hanja which are 2 wide.
        wrap_text(text, width, |c| if is_cjk(c) { 2.0 } else { 1.0 })
    }

    #[test]
    fn scale_and_presentation() {
        let c = DEFAULT_CANVAS;
        assert_eq!(c, vec2(480.0, 270.0));
        assert_eq!(integer_scale(1440.0, 810.0, c), 3);
        assert_eq!(integer_scale(1920.0, 1080.0, c), 4);
        assert_eq!(integer_scale(1366.0, 768.0, c), 2);
        assert_eq!(integer_scale(300.0, 200.0, c), 1);
        assert_eq!(integer_scale(0.0, 0.0, c), 1);
        // The default canvas goes up to 3840x2160.
        assert_eq!(integer_scale(100_000.0, 100_000.0, c), 8);

        let r = present_rect(1366.0, 768.0, c, 2);
        assert_eq!((r.x, r.y, r.w, r.h), (203.0, 114.0, 960.0, 540.0));
        // A window smaller than the canvas shrinks it to fit, keeping the aspect ratio.
        let r = present_rect(240.0, 270.0, c, 1);
        assert_eq!((r.w, r.h), (240.0, 135.0));
    }

    #[test]
    fn other_canvas_sizes_scale_the_same_way() {
        let vga = vec2(640.0, 480.0);
        assert_eq!(integer_scale(1440.0, 810.0, vga), 1);
        assert_eq!(integer_scale(1920.0, 1080.0, vga), 2);
        assert_eq!(integer_scale(3840.0, 2160.0, vga), 4);
        // The render target never exceeds MAX_TARGET_PX on either side.
        let big = vec2(1280.0, 800.0);
        assert_eq!(integer_scale(100_000.0, 100_000.0, big), 3);
        assert_eq!(integer_scale(100_000.0, 100_000.0, vga), 6);
        let r = present_rect(1920.0, 1080.0, vga, 2);
        assert_eq!((r.x, r.y, r.w, r.h), (320.0, 60.0, 1280.0, 960.0));
        // A window smaller than 640x480 shrinks the canvas (aspect kept).
        let r = present_rect(320.0, 480.0, vga, 1);
        assert_eq!((r.w, r.h), (320.0, 240.0));
    }

    #[test]
    fn canvas_sizes_come_from_the_presentation_profile() {
        assert_eq!(canvas_size(&Presentation::default()), DEFAULT_CANVAS);
        let p = |w, h| Presentation {
            canvas: [w, h],
            battle_frame: None,
            camp_frame: None,
            status_frame: None,
        };
        assert_eq!(canvas_size(&p(640, 480)), vec2(640.0, 480.0));
        assert_eq!(canvas_size(&p(1280, 800)), vec2(1280.0, 800.0));
    }

    #[test]
    fn mouse_mapping() {
        let present = Rect::new(203.0, 114.0, 960.0, 540.0);
        let c = DEFAULT_CANVAS;
        assert_eq!(
            screen_to_virtual(present, c, vec2(203.0, 114.0)),
            vec2(0.0, 0.0)
        );
        assert_eq!(
            screen_to_virtual(present, c, vec2(1163.0, 654.0)),
            vec2(480.0, 270.0)
        );
        assert_eq!(
            screen_to_virtual(present, c, vec2(683.0, 384.0)),
            vec2(240.0, 135.0)
        );
        assert_eq!(
            screen_to_virtual(Rect::new(0.0, 0.0, 0.0, 0.0), c, vec2(5.0, 5.0)),
            Vec2::ZERO
        );
        // 640x480 shown at 2x in a 1920x1080 window.
        let present = Rect::new(320.0, 60.0, 1280.0, 960.0);
        assert_eq!(
            screen_to_virtual(present, vec2(640.0, 480.0), vec2(960.0, 540.0)),
            vec2(320.0, 240.0)
        );
    }

    #[test]
    fn wraps_at_spaces() {
        assert_eq!(wrap_mono("aaa bbb ccc", 7.0), vec!["aaa bbb", "ccc"]);
        assert_eq!(wrap_mono("aaa bbb ccc", 3.0), vec!["aaa", "bbb", "ccc"]);
        // Korean words (2 units per syllable) break at spaces, not inside words.
        assert_eq!(
            wrap_mono("어지러운 세상이로구나 백성을", 20.0),
            vec!["어지러운", "세상이로구나 백성을"]
        );
    }

    #[test]
    fn elides_text_to_a_width() {
        let w = |s: &str| s.chars().count() as f32;
        assert_eq!(elide_text("abc", 3.0, w), "abc");
        assert_eq!(elide_text("abcdef", 4.0, w), "abc…");
        // A cut that lands after a space does not leave a gap before the ellipsis.
        assert_eq!(elide_text("ab cdef", 4.0, w), "ab…");
        assert_eq!(elide_text("", 0.0, w), "");
        // The ellipsis itself takes room: a budget that leaves none cuts everything.
        assert_eq!(elide_text("abc", 0.5, w), "…");
    }

    /// Kerning: the widths are measured on whole slices, so characters that draw narrower
    /// together are not cut early. This font charges 0.5 less for the "fi" pair.
    #[test]
    fn eliding_measures_whole_slices() {
        let w = |s: &str| s.chars().count() as f32 - 0.5 * s.matches("fi").count() as f32;
        // "fiX" draws 2.5 pixels wide, so it fits where one-by-one sums say 3.
        assert_eq!(elide_text("fiX", 2.6, w), "fiX");
        assert_eq!(elide_text("fiXYZ", 4.2, w), "fiX…");
    }

    /// A cut never lands between a base character and its combining marks (NFD Hangul here), so
    /// no stray jamo is left at the start of the next part.
    #[test]
    fn eliding_keeps_combining_marks_with_their_base() {
        let w = |s: &str| s.chars().count() as f32;
        // U+1100 U+1161 (decomposed 가) then U+1102 U+1161 (나).
        let text = "\u{1100}\u{1161}\u{1102}\u{1161}";
        // Room for the first syllable plus the ellipsis, none for the second's base.
        assert_eq!(elide_text(text, 3.0, w), "\u{1100}\u{1161}…");
        assert!(is_combining('\u{1161}') && !is_combining('\u{1100}'));
    }

    #[test]
    fn the_middle_elide_keeps_both_ends() {
        let w = |s: &str| s.chars().count() as f32;
        assert_eq!(elide_middle_text("abc", 3.0, w), "abc");
        // The front gets half the room, the back as much as fits; the whole stays within the
        // width it was given.
        assert_eq!(
            elide_middle_text("guan_yu_the_elder", 10.0, w),
            "guan…elder"
        );
        assert_eq!(elide_middle_text("abcdefg", 6.0, w), "ab…efg");
        // Names that differ at the front and the back stay apart, unlike a tail-only cut.
        assert_ne!(
            elide_middle_text("older_zhang_fei", 8.0, w),
            elide_middle_text("younger_zhang_yun", 8.0, w)
        );
        assert_eq!(elide_middle_text("abc defgh ijk", 8.0, w), "abc…ijk");
        assert_eq!(elide_middle_text("abc", 0.5, w), "…");
    }

    #[test]
    fn long_words_break_between_syllables() {
        assert_eq!(wrap_mono("가나다라마바", 6.0), vec!["가나다", "라마바"]);
        // Closing punctuation stays with the previous syllable.
        assert_eq!(wrap_mono("가나다.라", 6.0), vec!["가나", "다.라"]);
        // Latin without spaces breaks anywhere as a last resort.
        assert_eq!(wrap_mono("abcdefgh", 3.0), vec!["abc", "def", "gh"]);
    }

    #[test]
    fn explicit_newlines_and_edge_cases() {
        assert_eq!(wrap_mono("a\n\nb", 10.0), vec!["a", "", "b"]);
        assert_eq!(wrap_mono("", 10.0), vec![""]);
        assert_eq!(wrap_mono("ab  \r\ncd", 10.0), vec!["ab", "cd"]);
        // A character wider than the line still makes progress.
        assert_eq!(wrap_mono("가나", 1.0), vec!["가", "나"]);
        // Leading spaces of a paragraph are kept, those after a wrap are dropped.
        assert_eq!(wrap_mono("  ab cd", 5.0), vec!["  ab", "cd"]);
        // Indentation is not a break opportunity (no empty first line).
        assert_eq!(wrap_mono("  abcdef", 4.0), vec!["  ab", "cdef"]);
        // Spaces win over syllable breaks.
        assert_eq!(wrap_mono("가 나다라", 6.0), vec!["가", "나다라"]);
    }

    #[test]
    fn fit_rectangles() {
        let dest = Rect::new(0.0, 0.0, 64.0, 80.0);
        let (src, dst) = fit_rects(vec2(192.0, 240.0), dest, Fit::Contain);
        assert_eq!(src, Rect::new(0.0, 0.0, 192.0, 240.0));
        assert_eq!(dst, dest);
        let (src, dst) = fit_rects(
            vec2(200.0, 100.0),
            Rect::new(0.0, 0.0, 100.0, 100.0),
            Fit::Cover,
        );
        assert_eq!(src, Rect::new(50.0, 0.0, 100.0, 100.0));
        assert_eq!(dst, Rect::new(0.0, 0.0, 100.0, 100.0));
        let (_, dst) = fit_rects(
            vec2(200.0, 100.0),
            Rect::new(0.0, 0.0, 100.0, 100.0),
            Fit::Contain,
        );
        assert_eq!(dst, Rect::new(0.0, 25.0, 100.0, 50.0));
    }

    #[test]
    fn key_colors_are_stable() {
        assert_eq!(key_color("units/archer"), key_color("units/archer"));
        assert_ne!(key_color("units/archer"), key_color("units/bandit"));
    }
}
