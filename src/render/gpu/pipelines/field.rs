//! `field`: heatmaps as one quad per field (or per tile), with the cell lookup and colormap in
//! the fragment shader. Values live in `R32Float` textures read with `textureLoad` (no filtering;
//! bilinear interpolation is done by hand), split into tiles when the grid exceeds the device's
//! texture size. Irregular cell edges live in `R32Float` textures too, wrapped into rows of at
//! most the texture size.

use super::super::frame::{CMapU, DrawCmd, Frame};
use super::Layouts;
use crate::scene::drawlist::{Buf, FieldPrim, GridAxis};
use bytemuck::{Pod, Zeroable};
use std::sync::atomic::{AtomicU32, Ordering};

pub(crate) const SHADER: &str = include_str!("field.wgsl");

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct FieldU {
    rect_px: [f32; 4],
    imap: [f32; 4],
    lmap: [f32; 4],
    dims: [u32; 2],
    xdir: f32,
    ydir: f32,
    /// First column and row held in the values texture, and its width and height.
    tile: [u32; 4],
    interpolate: u32,
    irregular: u32,
    /// Row widths of the x / y edge textures.
    edge_w: [u32; 2],
    cm: CMapU,
}

pub(crate) fn layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("field"),
        entries: &[
            super::uniform_entry(0, true),
            super::texture_entry(1, false),
            super::texture_entry(2, false),
            super::texture_entry(3, false),
            super::texture_entry(4, true),
        ],
    })
}

pub(crate) fn pipeline(
    device: &wgpu::Device,
    l: &Layouts,
    shader: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    super::pipeline(
        device,
        "field",
        shader,
        "vs_field",
        "fs_field",
        l,
        &l.field,
        &[],
        wgpu::PrimitiveTopology::TriangleStrip,
        format,
    )
}

/// Test hook: overrides the tile edge in texels (0 = the device's `max_texture_dimension_2d`).
pub(crate) static MAX_TILE_OVERRIDE: AtomicU32 = AtomicU32::new(0);

/// Most tiles one field may be split into.
const MAX_TILES: usize = 1024;

/// Tiles along one dimension of `n` cells for textures of at most `max` texels: `(j0, j1, r0,
/// r1)` draws cells `j0..j1` from texels `r0..r1`, which add one halo cell on each side
/// (bilinear neighbours, and pixel centres on a tile's boundary). Empty if a tile can't hold a
/// cell.
pub(crate) fn tiles(n: usize, max: usize) -> Vec<(usize, usize, usize, usize)> {
    let halo = 2;
    if n <= max {
        return vec![(0, n, 0, n)];
    }
    if max <= halo {
        return vec![];
    }
    let per = max - halo;
    let h = halo / 2;
    (0..n)
        .step_by(per)
        .map(|j0| {
            let j1 = (j0 + per).min(n);
            (j0, j1, j0.saturating_sub(h), (j1 + h).min(n))
        })
        .collect()
}

/// One axis of the lookup: first and last edge in local coordinates, whether the shader searches
/// the edges buffer, and the edges' direction.
struct AxisMap {
    lo: f64,
    hi: f64,
    irregular: bool,
    dir: f32,
}

fn axis_map(g: &GridAxis) -> AxisMap {
    match g {
        GridAxis::Regular { e0, e1 } => AxisMap { lo: *e0, hi: *e1, irregular: false, dir: 1.0 },
        GridAxis::Edges(b) => {
            let first = b.data.first().copied().unwrap_or(0.0) as f64;
            let last = b.data.last().copied().unwrap_or(0.0) as f64;
            AxisMap { lo: first, hi: last, irregular: true, dir: if last < first { -1.0 } else { 1.0 } }
        }
    }
}

/// Local coordinate of edge `j` (of `n`).
fn edge(g: &GridAxis, j: usize, n: usize) -> f64 {
    match g {
        GridAxis::Regular { e0, e1 } => e0 + (e1 - e0) * (j as f64 / n as f64),
        GridAxis::Edges(b) => b.data.get(j).copied().unwrap_or(0.0) as f64,
    }
}

/// The edges texture of an irregular axis (`n + 1` values in rows of `max` texels) and its row
/// width; the 1×1 dummy for regular axes.
fn edges_texture(f: &mut Frame, g: &GridAxis, max: usize) -> (wgpu::TextureView, u32) {
    let GridAxis::Edges(b) = g else { return (f.dummy_tex(), 1) };
    let n = b.data.len();
    let w = n.clamp(1, max);
    let h = n.div_ceil(w);
    let key = b.key.map(|k| (k.uid, k.part, 0));
    let rev = b.key.map_or(0, |k| k.rev);
    if n == w * h {
        return (f.data_texture(key, rev, &b.data, w, 0, 0, w, h), w as u32);
    }
    // Pad the last row.
    let mut v = b.data.as_slice().to_vec();
    v.resize(w * h, 0.0);
    (f.data_texture(key, rev, &v, w, 0, 0, w, h), w as u32)
}

/// Draws for one field. `aff = (sx, sy, tx, ty)` maps local coordinates to device pixels (f64).
pub(crate) fn prepare(f: &mut Frame, p: &FieldPrim, aff: [f64; 4]) -> Vec<DrawCmd> {
    let (nx, ny) = (p.nx as usize, p.ny as usize);
    if nx == 0 || ny == 0 || p.values.len() < nx * ny {
        return vec![];
    }
    let [sx, sy, tx, ty] = aff;
    let (mx, my) = (axis_map(&p.x), axis_map(&p.y));
    // Regular grids: fractional cell index = px * a + b, derived in f64.
    let imap = |lo: f64, hi: f64, n: usize, s: f64, t: f64| {
        let d = (hi - lo) / n as f64;
        let a = 1.0 / (s * d);
        [a, -(t / s + lo) / d]
    };
    let [ix_a, ix_b] = if mx.irregular { [0.0, 0.0] } else { imap(mx.lo, mx.hi, nx, sx, tx) };
    let [iy_a, iy_b] = if my.irregular { [0.0, 0.0] } else { imap(my.lo, my.hi, ny, sy, ty) };
    let imap = [ix_a as f32, iy_a as f32, ix_b as f32, iy_b as f32];
    let lmap = [(1.0 / sx) as f32, (1.0 / sy) as f32, (-tx / sx) as f32, (-ty / sy) as f32];
    if !imap.iter().chain(&lmap).all(|v| v.is_finite()) {
        return vec![];
    }
    let short = |g: &GridAxis, n: usize| matches!(g, GridAxis::Edges(e) if e.len() < n + 1);
    if short(&p.x, nx) || short(&p.y, ny) {
        return vec![];
    }

    let max = match MAX_TILE_OVERRIDE.load(Ordering::Relaxed) {
        0 => f.device().limits().max_texture_dimension_2d,
        o => o,
    } as usize;
    let (xt, yt) = (tiles(nx, max), tiles(ny, max));
    if xt.is_empty() || yt.is_empty() || xt.len() * yt.len() > MAX_TILES {
        crate::warn_once("heatmap too large for the GPU's texture limits; not drawn");
        return vec![];
    }

    let (xe, xw) = edges_texture(f, &p.x, max);
    let (ye, yw) = edges_texture(f, &p.y, max);
    let lut = f.lut(&p.map.lut);
    let px = |v: f64, s: f64, t: f64, lim: u32| (v * s + t).clamp(-1.0, lim as f64 + 1.0);
    let (w, h) = (f.size[0], f.size[1]);
    // Boundaries between tiles snap to whole pixels, so every pixel (all its MSAA samples) is
    // drawn by one tile and looks up the same cell as an unsplit draw would.
    let tile_edge = |g: &GridAxis, j: usize, n: usize, s: f64, t: f64, lim: u32| {
        let v = px(edge(g, j, n), s, t, lim);
        if j == 0 || j == n { v } else { v.round() }
    };
    let mut cmds = Vec::with_capacity(xt.len() * yt.len());
    for (ty_i, &(j0, j1, r0, r1)) in yt.iter().enumerate() {
        let (y0, y1) = (tile_edge(&p.y, j0, ny, sy, ty, h), tile_edge(&p.y, j1, ny, sy, ty, h));
        for (tx_i, &(i0, i1, c0, c1)) in xt.iter().enumerate() {
            let (x0, x1) = (tile_edge(&p.x, i0, nx, sx, tx, w), tile_edge(&p.x, i1, nx, sx, tx, w));
            let index = (ty_i * xt.len() + tx_i) as u32;
            let values = values_texture(f, &p.values, nx, index, c0, r0, c1 - c0, r1 - r0);
            let offset = f.push_uniform(&FieldU {
                rect_px: [x0.min(x1) as f32, y0.min(y1) as f32, x0.max(x1) as f32, y0.max(y1) as f32],
                imap,
                lmap,
                dims: [p.nx, p.ny],
                xdir: mx.dir,
                ydir: my.dir,
                tile: [c0 as u32, r0 as u32, (c1 - c0) as u32, (r1 - r0) as u32],
                interpolate: p.interpolate as u32,
                irregular: mx.irregular as u32 | (my.irregular as u32) << 1,
                edge_w: [xw, yw],
                cm: CMapU::from(&p.map),
            });
            let bind = f.device().create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("field"),
                layout: &f.layouts().field,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: f.uniform_binding::<FieldU>() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&values) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&xe) },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&ye) },
                    wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&lut) },
                ],
            });
            cmds.push(DrawCmd {
                pipeline: f.pipes.field.clone(),
                bind,
                offset,
                vbs: Vec::new(),
                vertices: 0..4,
                instances: 0..1,
            });
        }
    }
    cmds
}

/// The values texture of tile `index`: `w × h` cells from column `x0`, row `y0`.
#[allow(clippy::too_many_arguments)]
fn values_texture(
    f: &mut Frame,
    v: &Buf<f32>,
    nx: usize,
    index: u32,
    x0: usize,
    y0: usize,
    w: usize,
    h: usize,
) -> wgpu::TextureView {
    let key = v.key.map(|k| (k.uid, k.part, index));
    let rev = v.key.map_or(0, |k| k.rev);
    f.data_texture(key, rev, &v.data, nx, x0, y0, w, h)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prelude::*;

    #[test]
    fn tiling() {
        assert_eq!(tiles(3, 1 << 20), [(0, 3, 0, 3)]);
        // Room for 4 texels: 2 cells plus a halo cell on each side.
        assert_eq!(tiles(5, 4), [(0, 2, 0, 3), (2, 4, 1, 5), (4, 5, 3, 5)]);
        assert!(tiles(5, 2).is_empty());
    }

    fn render(fig: &Figure) -> Option<Vec<u8>> {
        match fig.render_rgba(&Save::new().px_per_unit(1)) {
            Ok(img) => Some(img.data),
            Err(crate::Error::NoGpuAdapter(_)) => None,
            Err(e) => panic!("{e}"),
        }
    }

    /// Fields split into tiles (above the texture size limit) render exactly like one draw.
    #[test]
    fn tiles_render_identically() {
        let (nx, ny) = (37, 23);
        let v: Vec<f64> = (0..nx * ny).map(|k| ((k % nx) as f64 * 0.3).sin() + (k / nx) as f64 * 0.1).collect();
        for interpolate in [false, true] {
            for irregular in [false, true] {
                let fig = Figure!(size = (300, 200));
                let ax = Axis::new(fig.at(1, 1));
                let xs: Vec<f64> = (0..=nx)
                    .map(|i| if irregular { (i as f64 / nx as f64).powf(1.3) } else { i as f64 / nx as f64 })
                    .collect();
                ax.heatmap_xy(xs, Edges(0.0, 1.0), Field::new(&v, nx, ny)).interpolate(interpolate);
                MAX_TILE_OVERRIDE.store(0, Ordering::Relaxed);
                let Some(whole) = render(&fig) else { return };
                MAX_TILE_OVERRIDE.store(7, Ordering::Relaxed);
                let tiled = render(&fig).unwrap();
                MAX_TILE_OVERRIDE.store(0, Ordering::Relaxed);
                assert!(whole == tiled, "tiled render differs (interpolate = {interpolate}, irregular = {irregular})");
            }
        }
    }
}
