//! Text: rich strings, the bundled TeX Gyre Heros Makie fonts, and Makie-exact layout
//! (advance-only, no kerning; line box = ascender − descender = 1.165 em).

pub(crate) mod atlas;
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
pub(crate) fn layout(rt: &RichText, size: f64, font: Font, color: Color) -> TextLayout {
    let fs = faces();
    let (asc, desc) = metrics(font);
    let lh = line_height(size);
    let mut glyphs = Vec::new();
    let mut x = 0.0f64;
    let mut y = 0.0f64;
    let mut width = 0.0f64;
    let mut lines = 1;
    for span in &rt.spans {
        let f = span.font.unwrap_or(font);
        let face = fs.get(f);
        let upem = face.units_per_em().unwrap_or(1000.0) as f64;
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
                continue;
            }
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
    TextLayout { glyphs, width, ascent: asc * size, descent: desc * size, lines, size }
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

    #[test]
    fn digit_advance() {
        // Digits advance 0.556 em in TeX Gyre Heros.
        let l = layout(&"10".into(), 14.0, Font::Regular, Color::rgb(0.0, 0.0, 0.0));
        assert!((l.width - 2.0 * 0.556 * 14.0).abs() < 1e-9, "{}", l.width);
    }
}
