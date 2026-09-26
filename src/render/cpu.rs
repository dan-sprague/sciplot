//! CPU PNG fallback: rasterizes the SVG backend's output with resvg when no GPU adapter exists
//! (or when forced with `Save::cpu(true)` / `EZVIZ_FORCE_CPU=1`).
//!
//! resvg is built without raster-image decoding, so regular heatmaps (embedded PNGs in the SVG)
//! are composited directly with tiny-skia in painter's order; everything else goes through the
//! SVG. Blending happens on sRGB-encoded values, like the GPU backend.

use super::svg::field::{FieldImage, regular_image};
use super::svg::{self, SvgOptions};
use crate::color::Color;
use crate::error::{Error, Result};
use crate::scene::drawlist::{DrawList, Item, Prim, Space};
use resvg::{tiny_skia, usvg};

/// Largest image side, as for GPU exports.
const MAX_SIDE: u32 = 16384;

/// Whether `EZVIZ_FORCE_CPU=1` asks for the CPU rasterizer.
pub(crate) fn forced_by_env() -> bool {
    std::env::var_os("EZVIZ_FORCE_CPU").is_some_and(|v| v == "1")
}

/// Renders `dl` at `ppu` device pixels per unit to straight-alpha RGBA8 (top row first).
pub(crate) fn render_rgba(dl: &DrawList, ppu: f64) -> Result<(u32, u32, Vec<u8>)> {
    let w = (dl.size[0] * ppu).round().max(1.0) as u32;
    let h = (dl.size[1] * ppu).round().max(1.0) as u32;
    if w > MAX_SIDE || h > MAX_SIDE || !ppu.is_finite() || ppu <= 0.0 {
        return Err(Error::TooLarge(format!(
            "a {w}x{h} px image exceeds the limit of {MAX_SIDE} px; lower px_per_unit/dpi or the figure size"
        )));
    }
    let mut pm = tiny_skia::Pixmap::new(w, h).ok_or_else(|| Error::TooLarge(format!("cannot allocate {w}x{h} px")))?;
    pm.fill(skia_color(dl.background));

    // Split the list at regular fields; each run of other items is one SVG document.
    let mut start = 0;
    for (i, item) in dl.items.iter().enumerate() {
        if let Some(img) = field_image(dl, item) {
            draw_svg(dl, &dl.items[start..i], ppu, &mut pm)?;
            draw_image(&img, item, ppu, &mut pm);
            start = i + 1;
        }
    }
    draw_svg(dl, &dl.items[start..], ppu, &mut pm)?;

    let rgba = pm
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect();
    Ok((w, h, rgba))
}

fn skia_color(c: Color) -> tiny_skia::Color {
    let q = |v: f32| v.clamp(0.0, 1.0);
    tiny_skia::Color::from_rgba(q(c.r), q(c.g), q(c.b), q(c.a)).unwrap_or(tiny_skia::Color::TRANSPARENT)
}

fn field_image(dl: &DrawList, item: &Item) -> Option<FieldImage> {
    let Prim::Field(f) = &item.prim else { return None };
    let xf = match item.space {
        Space::Figure => [1.0, 1.0, 0.0, 0.0],
        Space::Data(i) => dl.axes.get(i as usize)?.affine(1.0),
    };
    regular_image(f, xf)
}

fn draw_svg(dl: &DrawList, items: &[Item], ppu: f64, pm: &mut tiny_skia::Pixmap) -> Result<()> {
    if items.is_empty() {
        return Ok(());
    }
    let part = DrawList { size: dl.size, background: Color::TRANSPARENT, axes: dl.axes.clone(), items: items.to_vec() };
    let doc = svg::document(&part, &SvgOptions { pt_per_unit: 0.75, snap_ppu: Some(ppu) });
    let tree = usvg::Tree::from_str(&doc, &usvg::Options::default()).map_err(|e| Error::Encode(e.to_string()))?;
    // The tree's size is in CSS px; map it onto the pixmap.
    let s = tree.size();
    let t = tiny_skia::Transform::from_scale(pm.width() as f32 / s.width(), pm.height() as f32 / s.height());
    resvg::render(&tree, t, &mut pm.as_mut());
    Ok(())
}

/// Composites a heatmap image (one pixel per cell) with nearest or bilinear sampling, clipped
/// to the item's clip rectangle snapped to device pixels (the GPU scissor).
fn draw_image(img: &FieldImage, item: &Item, ppu: f64, pm: &mut tiny_skia::Pixmap) {
    let Prim::Field(f) = &item.prim else { return };
    let Some(size) = tiny_skia::IntSize::from_wh(img.width, img.height) else { return };
    let premul: Vec<u8> = img
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| {
            let a = p[3] as u32;
            let m = |v: u8| ((v as u32 * a + 127) / 255) as u8;
            [m(p[0]), m(p[1]), m(p[2]), p[3]]
        })
        .collect();
    let Some(src) = tiny_skia::Pixmap::from_vec(premul, size) else { return };
    let [x, y, w, h] = img.rect.map(|v| v * ppu);
    let quality = if f.interpolate { tiny_skia::FilterQuality::Bilinear } else { tiny_skia::FilterQuality::Nearest };
    let shader = tiny_skia::Pattern::new(
        src.as_ref(),
        tiny_skia::SpreadMode::Pad,
        quality,
        1.0,
        tiny_skia::Transform::from_row(
            (w / img.width as f64) as f32,
            0.0,
            0.0,
            (h / img.height as f64) as f32,
            x as f32,
            y as f32,
        ),
    );
    let paint = tiny_skia::Paint { shader, anti_alias: true, ..Default::default() };
    let Some(rect) = tiny_skia::Rect::from_xywh(x as f32, y as f32, w as f32, h as f32) else { return };
    let mask = item.clip.and_then(|c| {
        let [x0, y0, x1, y1] = [c.x, c.y, c.right(), c.bottom()].map(|v| (v * ppu).round() as f32);
        let r = tiny_skia::Rect::from_ltrb(x0, y0, x1, y1)?;
        let mut m = tiny_skia::Mask::new(pm.width(), pm.height())?;
        m.fill_path(&tiny_skia::PathBuilder::from_rect(r), tiny_skia::FillRule::Winding, false, Default::default());
        Some(m)
    });
    if item.clip.is_some() && mask.is_none() {
        return; // empty clip
    }
    pm.fill_rect(rect, &paint, tiny_skia::Transform::identity(), mask.as_ref());
}
