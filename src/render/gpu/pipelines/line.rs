//! `line`: polylines and line segments (a port of GLMakie's line shaders) as instanced 4-vertex
//! triangle strips, one instance per segment.
//!
//! Instance `k` draws the segment from point `k` to `k + 1` (in segments mode only even `k`). The
//! point buffer from `Frame::points` is bound at four offsets (instance step, stride 8) so each
//! instance reads `p[k - 1] .. p[k + 2]`; per-point colors (or values) are bound at two offsets
//! for both ends, and dash arc lengths at one. When consecutive points repeat exactly, the
//! neighbours past the duplicates (up to `MAX_SKIP`) are resolved on the CPU into per-segment
//! `prev`/`next` buffers that take the place of `p[k - 1]` and `p[k + 2]`.
//!
//! Provenance: `arc_lengths` algorithm adapted from GLMakie 0.13.14 `src/glshaders/lines.jl`
//! (`sumlengths`); `gl_miter_limit` ported from Makie 0.24.14 `src/backend-functionality.jl`. The
//! shader is a port of GLMakie's line shaders (see `line.wgsl`). MIT licensed; see
//! THIRD_PARTY_NOTICES.md.

use super::super::frame::{CMapU, DrawCmd, Frame, POINTS_OFFSET, premul, tag};
use super::Layouts;
use crate::scene::drawlist::{Buf, LinesPrim, PrimColor};
use crate::style::{JoinStyle, LineCap};
use bytemuck::{Pod, Zeroable};
use std::hash::{Hash, Hasher};

pub(crate) const SHADER: &str = include_str!("line.wgsl");

/// Most dash boundaries a pattern may have (`LineU::breaks`).
const MAX_BREAKS: usize = 16;
/// Exact duplicate points looked past when searching for a joint's neighbour (as `MAX_SKIP` in
/// line.wgsl did when it searched on the GPU).
const MAX_SKIP: usize = 8;

/// WGSL `LineU` (see line.wgsl).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct LineU {
    xform: [f32; 4],
    color: [f32; 4],
    width: f32,
    miter_limit: f32,
    joinstyle: u32,
    linecap: u32,
    color_mode: u32,
    segments: u32,
    n: u32,
    n_breaks: u32,
    pattern_len: f32,
    closed: u32,
    _p: [u32; 2],
    breaks: [f32; MAX_BREAKS],
    cm: CMapU,
}

pub(crate) fn layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("line"),
        entries: &[super::uniform_entry(0, true), super::texture_entry(1, true)],
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
        "line",
        shader,
        "vs_line",
        "fs_line",
        l,
        &l.line,
        &[
            super::instance_attr(8, &wgpu::vertex_attr_array![0 => Float32x2]),
            super::instance_attr(8, &wgpu::vertex_attr_array![1 => Float32x2]),
            super::instance_attr(8, &wgpu::vertex_attr_array![2 => Float32x2]),
            super::instance_attr(8, &wgpu::vertex_attr_array![3 => Float32x2]),
            super::instance_attr(4, &wgpu::vertex_attr_array![4 => Uint32]),
            super::instance_attr(4, &wgpu::vertex_attr_array![5 => Uint32]),
            super::instance_attr(4, &wgpu::vertex_attr_array![6 => Float32]),
        ],
        wgpu::PrimitiveTopology::TriangleStrip,
        format,
    )
}

pub(crate) fn prepare(f: &mut Frame, l: &LinesPrim, xform: [f32; 4]) -> Option<DrawCmd> {
    let n = l.pts.len();
    let width_px = l.width as f64 * f.ppu;
    let nseg = if l.segments { n / 2 } else { n.saturating_sub(1) };
    if nseg == 0 || width_px.is_nan() || width_px <= 0.0 || n > u32::MAX as usize {
        return None;
    }
    let closed = l.closed && !l.segments;
    let pts = f.points(&l.pts, l.append, closed);
    let (color_mode, color, col, cm, lut) = match &l.color {
        PrimColor::Uniform(c) => (0, premul(*c), None, CMapU::default(), f.dummy_lut()),
        PrimColor::PerElement(b) if b.len() >= n => (1, [0.0; 4], Some(f.vertex(b)), CMapU::default(), f.dummy_lut()),
        PrimColor::Values(b, map) if b.len() >= n => {
            (2, [0.0; 4], Some(f.vertex(b)), CMapU::from(map), f.lut(&map.lut))
        }
        _ => {
            crate::warn_once("line colors have fewer entries than points; drawing nothing");
            return None;
        }
    };

    // Dashes: Makie's cumulative pattern in linewidths, phase continuous along each polyline run
    // through the screen-space arc length at every point.
    let mut breaks = [0.0f32; MAX_BREAKS];
    let (n_breaks, pattern_len, cum) = match l.pattern.as_deref().and_then(valid_pattern) {
        Some(p) => {
            breaks[..p.len()].copy_from_slice(p);
            let len = p[p.len() - 1] - p[0];
            let cum = if l.segments {
                None
            } else {
                let period = len as f64 * width_px.max(0.8);
                Some(dash_arc_lengths(f, &l.pts, xform, period))
            };
            (p.len() as u32, len, cum)
        }
        None => (0, 1.0, None),
    };

    // Joint neighbours: the points before and after each segment, or (past exact duplicates)
    // resolved per segment on the CPU.
    let (prev, next) = if !l.segments && f.has_duplicates(&l.pts) {
        let nb = std::cell::OnceCell::new();
        let calc = || nb.get_or_init(|| neighbours(&l.pts.data, closed));
        let usage = wgpu::BufferUsages::VERTEX;
        let (prev, next) = match l.pts.key {
            Some(k) => {
                let rev = hash((k.rev, closed));
                let bytes = |v: &Vec<[f32; 2]>| bytemuck::cast_slice::<_, u8>(v).to_vec();
                (
                    f.cached_with((k.uid, k.part, tag::PREV), rev, usage, || bytes(&calc().0)),
                    f.cached_with((k.uid, k.part, tag::NEXT), rev, usage, || bytes(&calc().1)),
                )
            }
            None => {
                let (p, q) = calc();
                (f.transient(bytemuck::cast_slice(p), usage), f.transient(bytemuck::cast_slice(q), usage))
            }
        };
        ((prev, 0), (next, 0))
    } else {
        ((pts.clone(), 0), (pts.clone(), 3 * 8))
    };

    let offset = f.push_uniform(&LineU {
        xform,
        color,
        width: width_px as f32,
        miter_limit: gl_miter_limit(l.miter_limit),
        joinstyle: match l.join {
            JoinStyle::Miter => 0,
            JoinStyle::Round => 2,
            JoinStyle::Bevel => 3,
        },
        linecap: match l.cap {
            LineCap::Butt => 0,
            LineCap::Square => 1,
            LineCap::Round => 2,
        },
        color_mode,
        segments: l.segments as u32,
        n: n as u32,
        n_breaks,
        pattern_len,
        closed: closed as u32,
        _p: [0; 2],
        breaks,
        cm,
    });
    let bind = f.device().create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("line"),
        layout: &f.layouts().line,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: f.uniform_binding::<LineU>() },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&lut) },
        ],
    });
    // Streams a line doesn't have read the (large enough) point buffer; the shader ignores them.
    let (c1, c2) = match col {
        Some(b) => ((b.clone(), 0), (b, 4)),
        None => ((pts.clone(), 0), (pts.clone(), 0)),
    };
    let cum = cum.map_or((pts.clone(), 0), |b| (b, 0));
    Some(DrawCmd {
        pipeline: f.pipes.line.clone(),
        bind,
        offset,
        vbs: vec![prev, (pts.clone(), POINTS_OFFSET), (pts, 2 * POINTS_OFFSET), next, c1, c2, cum],
        vertices: 0..4,
        instances: 0..(n - 1) as u32,
    })
}

/// Per segment `k` (points `k`, `k + 1`): the point before `k` and the point after `k + 1`,
/// looking past up to [`MAX_SKIP`] exact duplicates and across the seam of `closed` loops, as the
/// neighbouring drawn segments see them. NaN where the line starts or ends (or breaks).
pub(crate) fn neighbours(p: &[[f32; 2]], closed: bool) -> (Vec<[f32; 2]>, Vec<[f32; 2]>) {
    let n = p.len();
    let finite = |q: [f32; 2]| q[0].is_finite() && q[1].is_finite();
    let none = [f32::NAN; 2];
    let prev = (0..n)
        .map(|i1| {
            let mut j = i1;
            for _ in 0..MAX_SKIP {
                if j == 0 {
                    if !closed {
                        break;
                    }
                    j = n - 1; // continue with the last segment (its end repeats point 0)
                }
                let q = p[j - 1];
                if !finite(q) {
                    break;
                }
                if q != p[j] {
                    return q;
                }
                j -= 1;
            }
            none
        })
        .collect();
    let next = (0..n)
        .map(|i1| {
            let mut j = i1 + 1;
            for _ in 0..MAX_SKIP {
                if j + 1 >= n {
                    if !closed {
                        break;
                    }
                    j = 0;
                }
                let q = p[j + 1];
                if !finite(q) {
                    break;
                }
                if q != p[j] {
                    return q;
                }
                j += 1;
            }
            none
        })
        .collect();
    (prev, next)
}

/// Makie's `gl_miter_limit = cos(pi - miter_limit)`: joints whose direction cosine is below it are
/// truncated. Clamped so near-zero limits can't produce unbounded spikes.
fn gl_miter_limit(angle: f32) -> f32 {
    (std::f32::consts::PI - angle).cos().max(-0.9999)
}

/// A usable pattern: 2..=16 non-decreasing boundaries spanning a positive length.
fn valid_pattern(p: &[f32]) -> Option<&[f32]> {
    let ok =
        p.len() >= 2 && p.iter().all(|v| v.is_finite()) && p.windows(2).all(|w| w[0] <= w[1]) && p[p.len() - 1] > p[0];
    if !ok {
        crate::warn_once("invalid linestyle pattern (needs increasing boundaries); drawing a solid line");
        return None;
    }
    if p.len() > MAX_BREAKS {
        crate::warn_once("linestyle patterns are limited to 16 boundaries; extra ones are ignored");
    }
    Some(&p[..p.len().min(MAX_BREAKS)])
}

/// Screen-space arc length (device px) at each point modulo the dash period, restarting at NaN
/// breaks (GLMakie's `sumlengths`). Computed in f64 whenever the points or the view change.
fn dash_arc_lengths(f: &mut Frame, pts: &Buf<[f32; 2]>, xform: [f32; 4], period: f64) -> wgpu::Buffer {
    let cum = arc_lengths(&pts.data, xform, period);
    let bytes: &[u8] = bytemuck::cast_slice(&cum);
    match pts.key {
        Some(k) => {
            let rev = hash((k.rev, xform.map(f32::to_bits), period.to_bits()));
            f.cached((k.uid, k.part, tag::DASH), rev, bytes, wgpu::BufferUsages::VERTEX)
        }
        None => f.transient(bytes, wgpu::BufferUsages::VERTEX),
    }
}

fn hash(v: impl Hash) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

pub(crate) fn arc_lengths(pts: &[[f32; 2]], xform: [f32; 4], period: f64) -> Vec<f32> {
    let [sx, sy, tx, ty] = xform.map(f64::from);
    let mut acc = 0.0f64;
    let mut prev: Option<[f64; 2]> = None;
    pts.iter()
        .map(|p| {
            let q = [p[0] as f64 * sx + tx, p[1] as f64 * sy + ty];
            let cur = (q[0].is_finite() && q[1].is_finite()).then_some(q);
            acc = match (prev, cur) {
                (Some(a), Some(b)) => (acc + (b[0] - a[0]).hypot(b[1] - a[1])).rem_euclid(period),
                _ => 0.0,
            };
            prev = cur;
            acc as f32
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uniform_layout_matches_wgsl() {
        // 2 vec4 + 12 scalars + 4 vec4 breaks + CMap (4 vec4)
        assert_eq!(std::mem::size_of::<LineU>(), 16 + 16 + 48 + 64 + 64);
    }

    #[test]
    fn miter_limit_default_is_minus_half() {
        assert!((gl_miter_limit(std::f32::consts::FRAC_PI_3) + 0.5).abs() < 1e-6);
        assert!(gl_miter_limit(0.0) > -1.0);
    }

    #[test]
    fn arc_lengths_restart_at_nan_and_wrap() {
        let nan = f32::NAN;
        let p = [[0.0, 0.0], [3.0, 4.0], [3.0, 4.0], [6.0, 8.0], [nan, nan], [0.0, 0.0], [0.0, 1.0]];
        let c = arc_lengths(&p, [1.0, 1.0, 0.0, 0.0], 1e9);
        assert_eq!(c, [0.0, 5.0, 5.0, 10.0, 0.0, 0.0, 1.0]);
        // Scaled by the affine (px), modulo the period.
        let c = arc_lengths(&p[..4], [2.0, 2.0, 7.0, 7.0], 6.0);
        assert_eq!(c, [0.0, 4.0, 4.0, 2.0]);
    }

    /// Segments mode draws independent pairs; a dash pattern restarts on each segment.
    #[test]
    fn segments_and_dashes_render() {
        use crate::color::Color;
        use crate::scene::drawlist::{DrawList, Emitter, Prim, Space};
        let Ok(gpu) = crate::render::gpu::gpu() else { return };
        let mut r = crate::render::gpu::Renderer::new(gpu);
        let line = |pts: Vec<[f32; 2]>, pattern: Option<Vec<f32>>, segments: bool| {
            Prim::Lines(LinesPrim {
                pts: Buf::transient(pts),
                color: PrimColor::Uniform(Color::rgb(0.0, 0.0, 0.0)),
                width: 4.0,
                pattern,
                cap: LineCap::Butt,
                join: JoinStyle::Miter,
                miter_limit: std::f32::consts::FRAC_PI_3,
                segments,
                closed: false,
                append: false,
            })
        };
        let mut em = Emitter::new();
        em.push(
            0.0,
            None,
            Space::Figure,
            line(vec![[10.0, 10.0], [90.0, 10.0], [10.0, 30.0], [90.0, 30.0]], None, true),
        );
        em.push(0.0, None, Space::Figure, line(vec![[10.0, 50.0], [90.0, 50.0]], Some(vec![0.0, 3.0, 6.0]), false));
        let dl = DrawList { size: [100.0, 60.0], background: Color::rgb(1.0, 1.0, 1.0), axes: vec![], items: em.items };
        let (w, _, px) = r.render_rgba(&dl, 1.0).unwrap();
        let at = |x: usize, y: usize| px[(y * w as usize + x) * 4];
        assert!(at(50, 10) < 20 && at(50, 30) < 20, "both segments drawn");
        assert!(at(50, 20) > 240, "segments are not connected");
        assert!(at(5, 10) > 240 && at(95, 10) > 240, "butt caps end at the points");
        // Dash of 3 linewidths (12 px) on, 12 px off, starting at x = 10.
        assert!(at(16, 50) < 20 && at(28, 50) > 240 && at(40, 50) < 20, "dash pattern");
    }

    #[test]
    fn patterns() {
        assert!(valid_pattern(&[0.0, 3.0, 6.0]).is_some());
        assert!(valid_pattern(&[0.0]).is_none());
        assert!(valid_pattern(&[0.0, 3.0, 2.0]).is_none());
        assert!(valid_pattern(&[1.0, 1.0]).is_none());
    }
}
