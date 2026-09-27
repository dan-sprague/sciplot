//! Text: rich strings, the bundled TeX Gyre Heros Makie fonts, and Makie-exact layout
//! (advance-only, no kerning; line box = ascender − descender = 1.165 em).
//!
//! Provenance: algorithm adapted from Makie 0.24.14 `src/basic_recipes/text.jl`
//! (`process_rt_node!`, `max_y_ascender`/`min_y_descender`, `apply_alignment_and_justification!`),
//! `src/layouting/text_layouting.jl` (`glyph_collection` line height) and
//! `src/layouting/text_boundingbox.jl` (advance × ascender/descender boxes); MIT licensed. The
//! bundled fonts are data from Makie's assets artifact (TeX Gyre Heros "Makie" variant, GUST Font
//! License); see assets/fonts/README.md and THIRD_PARTY_NOTICES.md.

pub(crate) mod atlas;
pub(crate) mod outline;
mod rich;
mod tex;

pub use rich::{RichText, TextSpan, subscript, superscript};
pub use tex::{IntoSpans, colored, tex};

use crate::color::Color;
use crate::scene::drawlist::GlyphInst;
use ab_glyph::{Font as _, FontRef, GlyphId};
use std::sync::OnceLock;

/// Font style for text attributes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Font {
    #[default]
    Regular,
    Bold,
    Italic,
    BoldItalic,
}

crate::attrs::conv_identity!(Font);

static REGULAR: &[u8] = include_bytes!("../../assets/fonts/TeXGyreHerosMakie-Regular.otf");
static BOLD: &[u8] = include_bytes!("../../assets/fonts/TeXGyreHerosMakie-Bold.otf");
static ITALIC: &[u8] = include_bytes!("../../assets/fonts/TeXGyreHerosMakie-Italic.otf");
static BOLD_ITALIC: &[u8] = include_bytes!("../../assets/fonts/TeXGyreHerosMakie-BoldItalic.otf");

pub(crate) struct Faces {
    faces: [FontRef<'static>; 4],
}

impl Faces {
    pub fn get(&self, f: Font) -> &FontRef<'static> {
        &self.faces[f as usize]
    }
}

/// The bundled fonts, parsed once.
pub(crate) fn faces() -> &'static Faces {
    static F: OnceLock<Faces> = OnceLock::new();
    F.get_or_init(|| {
        let load = |b: &'static [u8]| FontRef::try_from_slice(b).expect("bundled font is valid");
        Faces { faces: [load(REGULAR), load(BOLD), load(ITALIC), load(BOLD_ITALIC)] }
    })
}

/// Ascender and descender as fractions of the em (TeX Gyre Heros: 0.947, 0.218).
pub(crate) fn metrics(f: Font) -> (f64, f64) {
    let face = faces().get(f);
    let upem = face.units_per_em().unwrap_or(1000.0) as f64;
    (face.ascent_unscaled() as f64 / upem, -face.descent_unscaled() as f64 / upem)
}

/// Height of one line at `size` (ascender − descender).
pub(crate) fn line_height(size: f64) -> f64 {
    let (a, d) = metrics(Font::Regular);
    (a + d) * size
}

/// Laid-out text, relative to the baseline origin of the first line (y down).
#[derive(Clone, Debug, Default)]
pub(crate) struct TextLayout {
    pub glyphs: Vec<GlyphInst>,
    pub width: f64,
    /// Above the first baseline (positive).
    pub ascent: f64,
    /// Below the last baseline (positive).
    pub descent: f64,
    /// Number of lines.
    pub lines: usize,
    pub size: f64,
}

impl TextLayout {
    /// Total height of the text box.
    pub fn height(&self) -> f64 {
        self.ascent + self.descent + (self.lines.max(1) - 1) as f64 * line_height(self.size)
    }
}

/// Width and vertical extent of `rt` at `size`.
pub(crate) struct Extent {
    pub width: f64,
    #[allow(dead_code)]
    pub height: f64,
}

pub(crate) fn measure(rt: &RichText, size: f64, font: Font) -> Extent {
    let l = layout(rt, size, font, Color::rgb(0.0, 0.0, 0.0));
    Extent { width: l.width, height: l.height() }
}

/// Lays out rich text like Makie: glyph advances only, spans may scale and shift the baseline.
///
/// The vertical extent is Makie's (`max_y_ascender` / `min_y_descender`): the union over the
/// glyphs of the first line of `baseline shift + ascender × glyph size`, and over the last line of
/// `baseline shift − descender × glyph size`, so superscripts make a label taller. Text without
/// glyphs has no extent.
pub(crate) fn layout(rt: &RichText, size: f64, font: Font, color: Color) -> TextLayout {
    let fs = faces();
    let lh = line_height(size);
    let mut glyphs = Vec::new();
    let mut x = 0.0f64;
    let mut y = 0.0f64;
    let mut width = 0.0f64;
    let mut lines = 1;
    // Extent above the first baseline and below the last one (None: no glyph on that line yet).
    let mut first_up: Option<f64> = None;
    let mut last_down: Option<f64> = None;
    for span in &rt.spans {
        let f = span.font.unwrap_or(font);
        let face = fs.get(f);
        let upem = face.units_per_em().unwrap_or(1000.0) as f64;
        let (asc, desc) = metrics(f);
        let ssize = size * span.size_scale as f64;
        let shift = -(span.baseline_shift as f64) * size;
        let c = span.color.unwrap_or(color);
        x += span.x_offset as f64 * ssize;
        for ch in span.text.chars() {
            if ch == '\n' {
                width = width.max(x);
                x = 0.0;
                y += lh;
                lines += 1;
                last_down = None;
                continue;
            }
            if lines == 1 {
                first_up = Some(first_up.unwrap_or(f64::NEG_INFINITY).max(asc * ssize - shift));
            }
            last_down = Some(last_down.unwrap_or(f64::NEG_INFINITY).max(desc * ssize + shift));
            let id: GlyphId = face.glyph_id(ch);
            if id.0 == 0 {
                atlas::warn_missing_glyph(ch);
            }
            glyphs.push(GlyphInst {
                font: f,
                glyph: id.0,
                pos: [x as f32, (y + shift) as f32],
                size: ssize as f32,
                color: c,
                angle: 0.0,
            });
            x += face.h_advance_unscaled(id) as f64 / upem * ssize;
        }
    }
    width = width.max(x);
    TextLayout { glyphs, width, ascent: first_up.unwrap_or(0.0), descent: last_down.unwrap_or(0.0), lines, size }
}

/// The box `[x, y, w, h]` (figure units, y down) that `place(l, anchor, align, angle)` covers:
/// Makie's string bounding box (advance widths × ascender/descender extents), rotated.
pub(crate) fn placed_bbox(l: &TextLayout, anchor: [f64; 2], align: (f64, f64), angle: f64) -> [f64; 4] {
    let top = -l.ascent;
    let bottom = l.descent + (l.lines.max(1) - 1) as f64 * line_height(l.size);
    let ox = -align.0 * l.width;
    let oy = -(bottom - align.1 * (bottom - top));
    let (s, c) = angle.sin_cos();
    let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for (px, py) in [(ox, oy + top), (ox + l.width, oy + top), (ox, oy + bottom), (ox + l.width, oy + bottom)] {
        let (rx, ry) = (anchor[0] + px * c + py * s, anchor[1] - px * s + py * c);
        b = [b[0].min(rx), b[1].min(ry), b[2].max(rx), b[3].max(ry)];
    }
    [b[0], b[1], b[2] - b[0], b[3] - b[1]]
}

/// Places a layout: the point of its box at fractions `align = (h, v)` (h: 0 left..1 right,
/// v: 0 bottom..1 top) goes to `anchor` (figure units, y down), then the text is rotated by
/// `angle` radians counter-clockwise about the anchor.
pub(crate) fn place(l: &TextLayout, anchor: [f64; 2], align: (f64, f64), angle: f64) -> Vec<GlyphInst> {
    let top = -l.ascent;
    let bottom = l.descent + (l.lines.max(1) - 1) as f64 * line_height(l.size);
    let ox = -align.0 * l.width;
    let oy = -(bottom - align.1 * (bottom - top));
    let (s, c) = angle.sin_cos();
    l.glyphs
        .iter()
        .map(|g| {
            let px = g.pos[0] as f64 + ox;
            let py = g.pos[1] as f64 + oy;
            let rx = px * c + py * s;
            let ry = -px * s + py * c;
            GlyphInst { pos: [(anchor[0] + rx) as f32, (anchor[1] + ry) as f32], angle: angle as f32, ..*g }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn makie_metrics() {
        let (a, d) = metrics(Font::Regular);
        assert!((a - 0.947).abs() < 1e-9 && (d - 0.218).abs() < 1e-9);
        assert!((line_height(14.0) - 16.31).abs() < 1e-9);
    }

    /// Makie's string bounding box of `rich("10", superscript("3", offset = (0.1, 0)))` at 14:
    /// 17.402 high (ascent 0.4·14 + 0.947·9.24), width 2·7.784 + 0.924 + 5.137 (CairoMakie dump).
    #[test]
    fn superscript_extends_the_box() {
        let rt = RichText::from_spans([TextSpan::plain("10"), superscript("3").offset(0.1, 0.0)]);
        let l = layout(&rt, 14.0, Font::Regular, Color::rgb(0.0, 0.0, 0.0));
        assert!((l.ascent - (0.4 * 14.0 + 0.947 * 0.66 * 14.0)).abs() < 1e-5, "{}", l.ascent);
        assert!((l.descent - 0.218 * 14.0).abs() < 1e-9);
        assert!((l.height() - 17.402).abs() < 1e-3, "{}", l.height());
        assert!((l.width - 21.629).abs() < 2e-3, "{}", l.width);
        let sub = layout(
            &RichText::from_spans([TextSpan::plain("x"), subscript("i")]),
            14.0,
            Font::Regular,
            l.glyphs[0].color,
        );
        assert!((sub.descent - (0.25 * 14.0 + 0.218 * 0.66 * 14.0)).abs() < 1e-5);
        assert!((sub.ascent - 0.947 * 14.0).abs() < 1e-9);
        assert_eq!(superscript("2").x_offset, 0.0);
        assert_eq!(layout(&"".into(), 14.0, Font::Regular, l.glyphs[0].color).height(), 0.0);
    }

    #[test]
    fn placed_bbox_matches_alignment() {
        let l = layout(&"10".into(), 14.0, Font::Regular, Color::rgb(0.0, 0.0, 0.0));
        let b = placed_bbox(&l, [100.0, 50.0], (0.5, 1.0), 0.0);
        assert!((b[0] - (100.0 - 7.784)).abs() < 1e-9 && (b[1] - 50.0).abs() < 1e-9);
        assert!((b[3] - 16.31).abs() < 1e-9);
        let r = placed_bbox(&l, [100.0, 50.0], (0.5, 0.0), std::f64::consts::FRAC_PI_2);
        assert!((r[2] - 16.31).abs() < 1e-9 && (r[3] - 15.568).abs() < 1e-9);
        assert!((r[0] + r[2] - 100.0).abs() < 1e-9, "bottom of rotated text faces +x: {r:?}");
    }

    #[test]
    fn digit_advance() {
        // Digits advance 0.556 em in TeX Gyre Heros.
        let l = layout(&"10".into(), 14.0, Font::Regular, Color::rgb(0.0, 0.0, 0.0));
        assert!((l.width - 2.0 * 0.556 * 14.0).abs() < 1e-9, "{}", l.width);
    }
}
