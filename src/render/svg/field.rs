//! CPU colormapping (the GPU `cmap_lookup` rules) and heatmap images for vector output.

use crate::color::Color;
use crate::scene::drawlist::{ColorMapping, FieldPrim, GridAxis};

/// Maps values to colors exactly like `cmap_lookup` in `common.wgsl`: a linearly sampled RGBA8
/// LUT, Makie's clip colors, `alpha` applied to everything but the NaN color.
pub(crate) struct Mapper {
    lut: Vec<[f32; 4]>,
    range: [f32; 2],
    low: Color,
    high: Color,
    nan: Color,
    alpha: f32,
}

impl Mapper {
    pub fn new(m: &ColorMapping) -> Mapper {
        // The GPU samples an RGBA8 texture: quantize the same way.
        let lut: Vec<[f32; 4]> = m.lut.iter().map(|c| c.to_rgba8().map(|v| v as f32 / 255.0)).collect::<Vec<_>>();
        let first = m.lut.first().copied().unwrap_or(Color::TRANSPARENT);
        let last = m.lut.last().copied().unwrap_or(Color::TRANSPARENT);
        let fade = |c: Color| c.with_alpha(c.a * m.alpha);
        Mapper {
            lut,
            range: m.range,
            low: fade(m.lowclip.unwrap_or(first)),
            high: fade(m.highclip.unwrap_or(last)),
            nan: m.nan_color,
            alpha: m.alpha,
        }
    }

    /// Straight-alpha color of `v`.
    pub fn color(&self, v: f32) -> Color {
        if v.is_nan() {
            return self.nan;
        }
        let [lo, hi] = self.range;
        if v < lo {
            return self.low;
        }
        if v > hi {
            return self.high;
        }
        if self.lut.is_empty() {
            return Color::TRANSPARENT;
        }
        let w = hi - lo;
        let t = if w > 0.0 { ((v - lo) / w).clamp(0.0, 1.0) } else { 0.5 };
        let p = t * (self.lut.len() - 1) as f32;
        let i = (p.floor() as usize).min(self.lut.len() - 1);
        let j = (i + 1).min(self.lut.len() - 1);
        let f = p - i as f32;
        let (a, b) = (self.lut[i], self.lut[j]);
        let mix = |k: usize| a[k] + (b[k] - a[k]) * f;
        Color::rgba(mix(0), mix(1), mix(2), mix(3) * self.alpha)
    }
}

/// Cell colors of a field, x fastest (`None` if the value buffer is too short).
pub(crate) fn cell_colors(f: &FieldPrim) -> Option<Vec<Color>> {
    let n = f.nx as usize * f.ny as usize;
    let vals = f.values.data.get(..n)?;
    let m = Mapper::new(&f.map);
    Some(vals.iter().map(|v| m.color(*v)).collect())
}

/// Cell edges along one axis in local coordinates (`n + 1` values).
pub(crate) fn edges(axis: &GridAxis, n: u32) -> Option<Vec<f64>> {
    match axis {
        GridAxis::Regular { e0, e1 } => {
            let n = n as f64;
            Some((0..=n as usize).map(|i| e0 + (e1 - e0) * i as f64 / n).collect())
        }
        GridAxis::Edges(b) => {
            let e = b.data.get(..n as usize + 1)?;
            Some(e.iter().map(|v| *v as f64).collect())
        }
    }
}

/// A regular field as an image placed in figure units.
pub(crate) struct FieldImage {
    /// `[x, y, w, h]` in figure units.
    pub rect: [f64; 4],
    pub width: u32,
    pub height: u32,
    /// Straight-alpha RGBA8, top row first, one pixel per cell.
    pub rgba: Vec<u8>,
}

/// The image of a field on a regular grid under the local -> units affine `xf`
/// (`None` for irregular grids, empty fields or degenerate geometry).
pub(crate) fn regular_image(f: &FieldPrim, xf: [f64; 4]) -> Option<FieldImage> {
    let (GridAxis::Regular { e0: x0, e1: x1 }, GridAxis::Regular { e0: y0, e1: y1 }) = (&f.x, &f.y) else {
        return None;
    };
    if f.nx == 0 || f.ny == 0 {
        return None;
    }
    let [sx, sy, tx, ty] = xf;
    let (fx0, fx1) = (x0 * sx + tx, x1 * sx + tx);
    let (fy0, fy1) = (y0 * sy + ty, y1 * sy + ty);
    if ![fx0, fx1, fy0, fy1].iter().all(|v| v.is_finite()) || fx0 == fx1 || fy0 == fy1 {
        return None;
    }
    let colors = cell_colors(f)?;
    let (nx, ny) = (f.nx as usize, f.ny as usize);
    let flip_x = fx1 < fx0;
    // Image rows run down the page; cell rows run from y0 to y1.
    let flip_y = fy1 < fy0;
    let mut rgba = Vec::with_capacity(nx * ny * 4);
    for r in 0..ny {
        let j = if flip_y { ny - 1 - r } else { r };
        for c in 0..nx {
            let i = if flip_x { nx - 1 - c } else { c };
            rgba.extend_from_slice(&colors[j * nx + i].to_rgba8());
        }
    }
    Some(FieldImage {
        rect: [fx0.min(fx1), fy0.min(fy1), (fx1 - fx0).abs(), (fy1 - fy0).abs()],
        width: f.nx,
        height: f.ny,
        rgba,
    })
}

/// Integer upsampling so each cell spans at least 4 image pixels (viewers that smooth images
/// then only blur cell borders), keeping the image at most 4096 px per side.
pub(crate) fn upsample_factor(img: &FieldImage, interpolate: bool) -> u32 {
    if interpolate {
        return 1;
    }
    (4096 / img.width.max(img.height).max(1)).clamp(1, 4)
}

/// PNG bytes of `img` upsampled by `k` (nearest neighbour).
pub(crate) fn encode_png(img: &FieldImage, k: u32) -> Option<Vec<u8>> {
    let (w, h, k) = (img.width as usize, img.height as usize, k.max(1) as usize);
    let mut data = Vec::with_capacity(w * h * k * k * 4);
    for row in img.rgba.chunks_exact(w * 4) {
        let mut line = Vec::with_capacity(w * k * 4);
        for px in row.as_chunks::<4>().0 {
            for _ in 0..k {
                line.extend_from_slice(px);
            }
        }
        for _ in 0..k {
            data.extend_from_slice(&line);
        }
    }
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, (w * k) as u32, (h * k) as u32);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut wr = enc.write_header().ok()?;
        wr.write_image_data(&data).ok()?;
        wr.finish().ok()?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn mapping() -> ColorMapping {
        ColorMapping {
            lut: Arc::new(vec![Color::rgb(0.0, 0.0, 0.0), Color::rgb(1.0, 1.0, 1.0)]),
            range: [0.0, 1.0],
            lowclip: Some(Color::rgb(0.0, 0.0, 1.0)),
            highclip: None,
            nan_color: Color::TRANSPARENT,
            alpha: 0.5,
        }
    }

    #[test]
    fn colormap_rules() {
        let m = Mapper::new(&mapping());
        let c = m.color(0.25);
        assert!((c.r - 0.25).abs() < 1e-6 && (c.a - 0.5).abs() < 1e-6);
        assert_eq!(m.color(-1.0), Color::rgba(0.0, 0.0, 1.0, 0.5));
        assert_eq!(m.color(2.0), Color::rgba(1.0, 1.0, 1.0, 0.5));
        assert_eq!(m.color(f32::NAN), Color::TRANSPARENT);
        assert_eq!(m.color(f32::INFINITY), Color::rgba(1.0, 1.0, 1.0, 0.5));
    }
}
