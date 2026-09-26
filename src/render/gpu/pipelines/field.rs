//! `field`: heatmaps as one quad per field (or per row band), with the cell lookup and colormap
//! in the fragment shader.

use super::super::frame::{CMapU, DrawCmd, Frame};
use crate::scene::drawlist::{FieldPrim, GridAxis};
use bytemuck::{Pod, Zeroable};
use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) struct FieldPipeline {
    pub layout: wgpu::BindGroupLayout,
    pub pipeline: wgpu::RenderPipeline,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct FieldU {
    rect_px: [f32; 4],
    imap: [f32; 4],
    lmap: [f32; 4],
    dims: [u32; 2],
    row0: u32,
    rows: u32,
    interpolate: u32,
    irregular: u32,
    xdir: f32,
    ydir: f32,
    cm: CMapU,
}

pub(crate) fn create(device: &wgpu::Device, globals: &wgpu::BindGroupLayout) -> FieldPipeline {
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("field"),
        entries: &[
            super::uniform_entry(0, true),
            super::storage_entry(1),
            super::storage_entry(2),
            super::storage_entry(3),
            super::texture_entry(4, true),
        ],
    });
    let shader = super::module(device, "field", include_str!("field.wgsl"));
    let pipeline = super::pipeline(
        device,
        "field",
        &shader,
        "vs_field",
        "fs_field",
        globals,
        &layout,
        &[],
        wgpu::PrimitiveTopology::TriangleStrip,
    );
    FieldPipeline { layout, pipeline }
}

/// Test hook: overrides the per-binding byte limit (0 = use the device limit).
pub(crate) static MAX_BINDING_OVERRIDE: AtomicU64 = AtomicU64::new(0);

/// Row bands `(j0, j1, r0, r1)`: cells `j0..j1` are drawn from buffer rows `r0..r1`, which add one
/// halo row on each side (bilinear neighbours, and pixel centres on a band's boundary). Empty if
/// a band can't hold a row.
pub(crate) fn bands(nx: usize, ny: usize, max_bytes: u64) -> Vec<(usize, usize, usize, usize)> {
    let row_bytes = (nx as u64 * 4).max(1);
    let halo = 2;
    if row_bytes * ny as u64 <= max_bytes {
        return vec![(0, ny, 0, ny)];
    }
    let fit = (max_bytes / row_bytes) as usize;
    if fit <= halo {
        return vec![];
    }
    let per = fit - halo;
    let h = halo / 2;
    (0..ny)
        .step_by(per)
        .map(|j0| {
            let j1 = (j0 + per).min(ny);
            (j0, j1, j0.saturating_sub(h), (j1 + h).min(ny))
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

    let limits = f.device().limits();
    let max = match MAX_BINDING_OVERRIDE.load(Ordering::Relaxed) {
        0 => limits.max_storage_buffer_binding_size.min(limits.max_buffer_size / 3 * 2),
        o => o,
    } / 4
        * 4;
    let bands = bands(nx, ny, max);
    if bands.is_empty() || bands.len() > 200 {
        crate::warn_once("heatmap too large for the GPU's storage buffer limits; not drawn");
        return vec![];
    }

    let xe = match &p.x {
        GridAxis::Edges(b) => f.storage(b),
        GridAxis::Regular { .. } => f.dummy(),
    };
    let ye = match &p.y {
        GridAxis::Edges(b) => f.storage(b),
        GridAxis::Regular { .. } => f.dummy(),
    };
    let lut = f.lut(&p.map.lut);
    let px = |v: f64, s: f64, t: f64, lim: u32| (v * s + t).clamp(-1.0, lim as f64 + 1.0);
    let (w, h) = (f.size[0], f.size[1]);
    let (x0, x1) = (px(mx.lo, sx, tx, w), px(mx.hi, sx, tx, w));
    let single = bands.len() == 1;
    let mut cmds = Vec::with_capacity(bands.len());
    for (b, (j0, j1, r0, r1)) in bands.into_iter().enumerate() {
        // Boundaries between bands snap to whole pixels, so every pixel (all its MSAA samples) is
        // drawn by one band and looks up the same cell as an unsplit draw would.
        let band_edge = |j: usize| {
            let y = px(edge(&p.y, j, ny), sy, ty, h);
            if j == 0 || j == ny { y } else { y.round() }
        };
        let (y0, y1) = (band_edge(j0), band_edge(j1));
        let values = if single {
            f.storage(&p.values)
        } else {
            let bytes: &[u8] = bytemuck::cast_slice(&p.values.data[r0 * nx..r1 * nx]);
            match p.values.key {
                Some(k) => f.cached((k.uid, 16 + b as u8), k.rev, bytes, wgpu::BufferUsages::STORAGE),
                None => f.transient(bytes, wgpu::BufferUsages::STORAGE),
            }
        };
        let offset = f.push_uniform(&FieldU {
            rect_px: [x0.min(x1) as f32, y0.min(y1) as f32, x0.max(x1) as f32, y0.max(y1) as f32],
            imap,
            lmap,
            dims: [p.nx, p.ny],
            row0: r0 as u32,
            rows: (r1 - r0) as u32,
            interpolate: p.interpolate as u32,
            irregular: mx.irregular as u32 | (my.irregular as u32) << 1,
            xdir: mx.dir,
            ydir: my.dir,
            cm: CMapU::from(&p.map),
        });
        let bind = f.device().create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("field"),
            layout: &f.pipes.field.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: f.uniform_binding::<FieldU>() },
                wgpu::BindGroupEntry { binding: 1, resource: values.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: xe.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: ye.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&lut) },
            ],
        });
        cmds.push(DrawCmd {
            pipeline: f.pipes.field.pipeline.clone(),
            bind,
            offset,
            vb: None,
            vertices: 0..4,
            instances: 0..1,
        });
    }
    cmds
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prelude::*;

    #[test]
    fn row_bands() {
        assert_eq!(bands(4, 3, 1 << 20), [(0, 3, 0, 3)]);
        // 4 floats per row, room for 4 rows: 2 cell rows plus a halo row on each side.
        assert_eq!(bands(4, 5, 64), [(0, 2, 0, 3), (2, 4, 1, 5), (4, 5, 3, 5)]);
        assert!(bands(4, 5, 32).is_empty());
    }

    fn render(fig: &Figure) -> Option<Vec<u8>> {
        match fig.render_rgba(&Save::new().px_per_unit(1)) {
            Ok(img) => Some(img.data),
            Err(crate::Error::NoGpuAdapter(_)) => None,
            Err(e) => panic!("{e}"),
        }
    }

    /// Fields split into row bands (above the storage binding limit) render exactly like one draw.
    #[test]
    fn row_bands_render_identically() {
        let (nx, ny) = (37, 23);
        let v: Vec<f64> = (0..nx * ny).map(|k| ((k % nx) as f64 * 0.3).sin() + (k / nx) as f64 * 0.1).collect();
        for interpolate in [false, true] {
            let fig = Figure!(size = (300, 200));
            let ax = Axis::new(fig.at(1, 1));
            ax.heatmap_xy(linspace(0.0, 1.0, nx + 1), Edges(0.0, 1.0), Field::new(&v, nx, ny)).interpolate(interpolate);
            MAX_BINDING_OVERRIDE.store(0, Ordering::Relaxed);
            let Some(whole) = render(&fig) else { return };
            MAX_BINDING_OVERRIDE.store((nx * 4 * 5) as u64, Ordering::Relaxed);
            let banded = render(&fig).unwrap();
            MAX_BINDING_OVERRIDE.store(0, Ordering::Relaxed);
            assert!(whole == banded, "banded render differs (interpolate = {interpolate})");
        }
    }
}
