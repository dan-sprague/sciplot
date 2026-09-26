//! Glyph outlines for vector output (SVG), extracted with ab_glyph.
//!
//! Outlines are in font units relative to the glyph origin with y flipped to point down, so a
//! glyph drawn at baseline origin `p` and font size `s` is the outline scaled by `s / upem` and
//! translated to `p` (this matches `text::layout`, where advance = `h_advance_unscaled / upem * s`).

use super::{Font, faces};
use ab_glyph::{Font as _, GlyphId, OutlineCurve, Point};

/// One path command in font units, y down.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum PathSeg {
    Move([f32; 2]),
    Line([f32; 2]),
    Quad([f32; 2], [f32; 2]),
    Cubic([f32; 2], [f32; 2], [f32; 2]),
    Close,
}

/// Units per em of `font` (1000 for TeX Gyre Heros).
pub(crate) fn units_per_em(font: Font) -> f64 {
    faces().get(font).units_per_em().unwrap_or(1000.0) as f64
}

/// The outline of glyph `id` as closed subpaths in font units (y down), or `None` for glyphs
/// without ink (space) or unknown ids.
pub(crate) fn glyph_path(font: Font, id: u16) -> Option<Vec<PathSeg>> {
    let outline = faces().get(font).outline(GlyphId(id))?;
    if outline.curves.is_empty() {
        return None;
    }
    let f = |p: Point| [p.x, -p.y];
    let mut segs = Vec::with_capacity(outline.curves.len() + 8);
    // ab_glyph returns independent curves; a new subpath starts wherever a curve does not begin
    // at the previous end point.
    let mut end: Option<Point> = None;
    let mut first = Point::default();
    let close = |segs: &mut Vec<PathSeg>, first: Point| {
        // `Z` draws the closing line itself (outlines may also end with a zero-length line).
        while matches!(segs.last(), Some(PathSeg::Line(p)) if near_pt(Point { x: p[0], y: -p[1] }, first)) {
            segs.pop();
        }
        segs.push(PathSeg::Close);
    };
    for c in &outline.curves {
        let (start, last) = match *c {
            OutlineCurve::Line(a, b) => (a, b),
            OutlineCurve::Quad(a, _, b) => (a, b),
            OutlineCurve::Cubic(a, _, _, b) => (a, b),
        };
        if end != Some(start) {
            if end.is_some() {
                close(&mut segs, first);
            }
            segs.push(PathSeg::Move(f(start)));
            first = start;
        }
        segs.push(match *c {
            OutlineCurve::Line(_, b) => PathSeg::Line(f(b)),
            // CFF straight edges come out as cubics with control points on the ends.
            OutlineCurve::Cubic(a, c1, c2, b) if near_pt(c1, a) && near_pt(c2, b) => PathSeg::Line(f(b)),
            OutlineCurve::Quad(_, c1, b) => PathSeg::Quad(f(c1), f(b)),
            OutlineCurve::Cubic(_, c1, c2, b) => PathSeg::Cubic(f(c1), f(c2), f(b)),
        });
        end = Some(last);
    }
    close(&mut segs, first);
    Some(segs)
}

/// Equal within f32 accumulation noise (font units).
fn near_pt(a: Point, b: Point) -> bool {
    (a.x - b.x).abs() < 0.01 && (a.y - b.y).abs() < 0.01
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outlines() {
        let face = faces().get(Font::Regular);
        let id = face.glyph_id('o').0;
        let p = glyph_path(Font::Regular, id).unwrap();
        // 'o' has two contours (outer and counter).
        assert_eq!(p.iter().filter(|s| matches!(s, PathSeg::Move(_))).count(), 2);
        assert_eq!(p.iter().filter(|s| matches!(s, PathSeg::Close)).count(), 2);
        // y is flipped: the glyph sits above the baseline, i.e. at negative y.
        let ys: Vec<f32> = p
            .iter()
            .filter_map(|s| match s {
                PathSeg::Move(q) | PathSeg::Line(q) | PathSeg::Quad(_, q) | PathSeg::Cubic(_, _, q) => Some(q[1]),
                PathSeg::Close => None,
            })
            .collect();
        assert!(ys.iter().all(|y| *y <= 20.0) && ys.iter().any(|y| *y < -400.0), "{ys:?}");
        assert!(glyph_path(Font::Regular, face.glyph_id(' ').0).is_none());
        assert_eq!(units_per_em(Font::Bold), 1000.0);
    }
}
