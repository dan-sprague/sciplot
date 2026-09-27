//! `glyph`: text as instanced quads over a per-renderer R8 coverage atlas.
//!
//! [`begin_frame`] makes every glyph of the draw list resident in the atlas before any draw is
//! prepared (growing it, or evicting glyphs unused this frame), so slots never move while a frame
//! is being built. [`prepare`] then turns one `Prim::Glyphs` into a draw.

use super::super::Gpu;
use super::super::frame::{DrawCmd, Frame};
use super::Layouts;
use crate::scene::drawlist::{DrawList, GlyphInst, GlyphsPrim, Item, Prim, Space};
use crate::text::atlas::{Atlas, Change, GlyphKey, SUBPIXEL_BINS};
use bytemuck::{Pod, Zeroable};
use std::f64::consts::FRAC_PI_2;

/// Initial atlas edge in texels.
const ATLAS_START: u32 = 2048;
/// Largest atlas edge before eviction kicks in.
const ATLAS_MAX: u32 = 4096;
/// Glyphs larger than this (device px em size) are not drawn.
const MAX_GLYPH_PX: f32 = 1024.0;

pub(crate) const SHADER: &str = include_str!("glyph.wgsl");

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GlyphU {
    atlas_px: [f32; 2],
    _pad: [f32; 2],
}

/// WGSL `GlyphI`: one instance-step vertex (48 bytes).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GlyphI {
    origin: [f32; 2],
    off: [f32; 2],
    size: [f32; 2],
    uv: [f32; 2],
    cs: [f32; 2],
    color: u32,
    flags: u32,
}

const FLAG_LINEAR: u32 = 1;

pub(crate) fn layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("glyph"),
        entries: &[super::uniform_entry(0, true), super::texture_entry(1, true)],
    })
}

pub(crate) fn pipeline(
    device: &wgpu::Device,
    l: &Layouts,
    shader: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    const ATTRS: [wgpu::VertexAttribute; 7] = wgpu::vertex_attr_array![
        0 => Float32x2, 1 => Float32x2, 2 => Float32x2, 3 => Float32x2, 4 => Float32x2, 5 => Uint32, 6 => Uint32
    ];
    super::pipeline(
        device,
        "glyph",
        shader,
        "vs_glyph",
        "fs_glyph",
        l,
        &l.glyph,
        &[super::instance_attr(std::mem::size_of::<GlyphI>() as u64, &ATTRS)],
        wgpu::PrimitiveTopology::TriangleStrip,
        format,
    )
}

/// The glyph atlas of one render context: CPU packer plus its GPU texture.
pub(crate) struct GlyphCache {
    atlas: Atlas,
    tex: wgpu::Texture,
    view: wgpu::TextureView,
}

fn atlas_texture(gpu: &Gpu, size: u32) -> (wgpu::Texture, wgpu::TextureView) {
    let tex = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("glyph atlas"),
        size: wgpu::Extent3d { width: size, height: size, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
    (tex, view)
}

impl GlyphCache {
    fn new(gpu: &Gpu) -> GlyphCache {
        let max = ATLAS_MAX.min(gpu.max_texture_size());
        let atlas = Atlas::new(ATLAS_START.min(max), max);
        let (tex, view) = atlas_texture(gpu, atlas.size());
        GlyphCache { atlas, tex, view }
    }

    /// Resizes the texture after the atlas grew, keeping the texels already uploaded.
    fn sync_size(&mut self, gpu: &Gpu, change: Change) {
        let size = self.atlas.size();
        if self.tex.width() == size {
            return;
        }
        let (tex, view) = atlas_texture(gpu, size);
        if let Change::Grew(old) = change {
            let mut enc =
                gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("atlas grow") });
            enc.copy_texture_to_texture(
                self.tex.as_image_copy(),
                tex.as_image_copy(),
                wgpu::Extent3d { width: old, height: old, depth_or_array_layers: 1 },
            );
            gpu.queue.submit([enc.finish()]);
        }
        self.tex = tex;
        self.view = view;
    }

    /// Writes pending glyph bitmaps into the texture; returns the bytes uploaded.
    fn upload(&mut self, gpu: &Gpu) -> u64 {
        let mut bytes = 0;
        for u in self.atlas.take_uploads() {
            gpu.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.tex,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x: u.x, y: u.y, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                &u.data,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(u.w), rows_per_image: Some(u.h) },
                wgpu::Extent3d { width: u.w, height: u.h, depth_or_array_layers: 1 },
            );
            bytes += u.data.len() as u64;
        }
        bytes
    }
}

/// Device-pixel placement of one glyph and the atlas bitmap it needs.
struct Placed {
    key: GlyphKey,
    origin: [f32; 2],
    cs: [f32; 2],
    linear: bool,
}

/// Places `g` for drawing with the item transform `xf` (`px = pos * xf.xy + xf.zw`).
///
/// Axis-aligned and quarter-turn glyphs snap the baseline to whole device pixels and the origin
/// along the advance to a pixel boundary, choosing the atlas bitmap rasterized at the remaining
/// subpixel phase, so texels land exactly on pixels. Other angles keep the exact origin.
fn place(g: &GlyphInst, xf: [f32; 4], ppu: f64) -> Option<Placed> {
    let size_px = g.size * ppu as f32;
    if !(0.25..=MAX_GLYPH_PX).contains(&size_px) || g.color.a <= 0.0 {
        return None;
    }
    let x = (g.pos[0] * xf[0] + xf[2]) as f64;
    let y = (g.pos[1] * xf[1] + xf[3]) as f64;
    let a = g.angle as f64;
    if !(x.is_finite() && y.is_finite() && a.is_finite()) {
        return None;
    }
    let k = (a / FRAC_PI_2).round();
    if (a - k * FRAC_PI_2).abs() > 1e-4 {
        let (s, c) = a.sin_cos();
        let key = GlyphKey::new(g.font, g.glyph, size_px, 0);
        return Some(Placed { key, origin: [x as f32, y as f32], cs: [c as f32, s as f32], linear: true });
    }
    let (c, s) = match (k as i64).rem_euclid(4) {
        0 => (1.0, 0.0),
        1 => (0.0, 1.0),
        2 => (-1.0, 0.0),
        _ => (0.0, -1.0),
    };
    // u: along the advance, v: along the glyph's "down" (perpendicular to the baseline).
    let u = x * c - y * s;
    let v = (x * s + y * c).round();
    let bins = SUBPIXEL_BINS as f64;
    let mut ui = u.floor();
    let mut bin = ((u - ui) * bins).round();
    if bin >= bins {
        ui += 1.0;
        bin = 0.0;
    }
    let key = GlyphKey::new(g.font, g.glyph, size_px, bin as u8);
    let origin = [(ui * c + v * s) as f32, (-ui * s + v * c) as f32];
    Some(Placed { key, origin, cs: [c as f32, s as f32], linear: false })
}

/// The item's local -> device pixel transform, as `Renderer::render` computes it.
fn item_xform(dl: &DrawList, item: &Item, ppu: f64) -> [f32; 4] {
    match item.space {
        Space::Figure => [ppu as f32, ppu as f32, 0.0, 0.0],
        Space::Data(i) => {
            dl.axes.get(i as usize).map_or([ppu as f32, ppu as f32, 0.0, 0.0], |a| a.affine(ppu).map(|v| v as f32))
        }
    }
}

/// Makes every glyph drawn by `dl` resident in the atlas and uploads new bitmaps. Call once per
/// frame before preparing any `Prim::Glyphs`.
pub(crate) fn begin_frame(f: &mut Frame, dl: &DrawList) {
    let ppu = f.ppu;
    let res = &mut *f.res;
    let cache = res.glyphs.get_or_insert_with(|| GlyphCache::new(&res.gpu));
    cache.atlas.begin_frame();
    let keys = dl.items.iter().flat_map(|item| {
        let glyphs: &[GlyphInst] = match &item.prim {
            Prim::Glyphs(g) => &g.glyphs,
            _ => &[],
        };
        let xf = item_xform(dl, item, ppu);
        glyphs.iter().filter_map(move |g| place(g, xf, ppu).map(|p| p.key))
    });
    let change = cache.atlas.ensure(keys);
    cache.sync_size(&res.gpu, change);
    res.stats.transient_bytes += cache.upload(&res.gpu);
}

pub(crate) fn prepare(f: &mut Frame, g: &GlyphsPrim, xform: [f32; 4]) -> Option<DrawCmd> {
    let ppu = f.ppu;
    let cache = f.res.glyphs.as_ref()?;
    let inst: Vec<GlyphI> = g
        .glyphs
        .iter()
        .filter_map(|gi| {
            let p = place(gi, xform, ppu)?;
            let slot = cache.atlas.get(&p.key)?;
            Some(GlyphI {
                origin: p.origin,
                off: [slot.off[0] as f32, slot.off[1] as f32],
                size: [slot.w as f32, slot.h as f32],
                uv: [slot.x as f32, slot.y as f32],
                cs: p.cs,
                color: gi.color.to_premul_u32(),
                flags: if p.linear { FLAG_LINEAR } else { 0 },
            })
        })
        .collect();
    if inst.is_empty() {
        return None;
    }
    let atlas_px = cache.atlas.size() as f32;
    let view = cache.view.clone();
    let buf = f.transient(bytemuck::cast_slice(&inst), wgpu::BufferUsages::VERTEX);
    let offset = f.push_uniform(&GlyphU { atlas_px: [atlas_px; 2], _pad: [0.0; 2] });
    let bind = f.device().create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("glyph"),
        layout: &f.layouts().glyph,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: f.uniform_binding::<GlyphU>() },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&view) },
        ],
    });
    Some(DrawCmd {
        pipeline: f.pipes.glyph.clone(),
        bind,
        offset,
        vbs: vec![(buf, 0)],
        vertices: 0..4,
        instances: 0..inst.len() as u32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Color;
    use crate::text::Font;

    fn glyph(pos: [f32; 2], angle: f32) -> GlyphInst {
        GlyphInst { font: Font::Regular, glyph: 20, pos, size: 14.0, color: Color::rgb(0.0, 0.0, 0.0), angle }
    }

    #[test]
    fn axis_aligned_snaps_baseline_and_bins_x() {
        let p = place(&glyph([10.3, 20.4], 0.0), [2.0, 2.0, 0.0, 0.0], 2.0).unwrap();
        // x = 20.6 px -> pixel 20, phase 0.6 -> bin 2; y = 40.8 -> 41.
        assert_eq!(p.origin, [20.0, 41.0]);
        assert_eq!(p.key.bin, 2);
        assert_eq!(p.key.size_q, 112);
        assert!(!p.linear);
        // Phase 0.9 rounds up to the next pixel with bin 0.
        let p = place(&glyph([10.45, 0.0], 0.0), [2.0, 2.0, 0.0, 0.0], 2.0).unwrap();
        assert_eq!((p.origin[0], p.key.bin), (21.0, 0));
    }

    #[test]
    fn quarter_turn_snaps_along_rotated_axes() {
        // 90° CCW: the advance points up (-y), the baseline is vertical at a whole x.
        let p = place(&glyph([10.3, 20.4], std::f32::consts::FRAC_PI_2), [2.0, 2.0, 0.0, 0.0], 2.0).unwrap();
        assert_eq!(p.cs, [0.0, 1.0]);
        assert_eq!(p.origin[0], 21.0);
        // u = -y = -40.8 -> floor -41, phase 0.2 -> bin 1; origin y = 41.
        assert_eq!((p.origin[1], p.key.bin), (41.0, 1));
        assert!(!p.linear);
    }

    /// Renders glyph runs offscreen at ppu 2; `None` without a GPU adapter.
    fn render(glyphs: Vec<GlyphInst>, size: [f64; 2]) -> Option<crate::figure::RgbaImage> {
        let gpu = match crate::render::gpu::gpu() {
            Ok(g) => g,
            Err(crate::Error::NoGpuAdapter(_)) => return None,
            Err(e) => panic!("{e}"),
        };
        let mut r = crate::render::gpu::Renderer::new(gpu);
        let item = Item { z: 0.0, seq: 0, clip: None, space: Space::Figure, prim: Prim::Glyphs(GlyphsPrim { glyphs }) };
        let dl = DrawList { size, background: Color::TRANSPARENT, axes: Vec::new(), items: vec![item] };
        let (width, height, data) = r.render_rgba(&dl, 2.0).unwrap();
        let img = crate::figure::RgbaImage { width, height, data };
        if let Ok(dir) = std::env::var("SCIPLOT_TEST_OUT") {
            let _ = crate::figure::write_png(&std::path::Path::new(&dir).join("glyph_angles.png"), &img, 2.0);
        }
        Some(img)
    }

    /// Alpha of `img` inside `[x0, x1) × [y0, y1)`, cropped to the ink.
    fn ink(img: &crate::figure::RgbaImage, x0: u32, x1: u32, y0: u32, y1: u32) -> Vec<Vec<u8>> {
        let a = |x: u32, y: u32| img.data[((y * img.width + x) * 4 + 3) as usize];
        let (mut bx0, mut bx1, mut by0, mut by1) = (x1, x0, y1, y0);
        for y in y0..y1 {
            for x in x0..x1 {
                if a(x, y) > 0 {
                    (bx0, bx1, by0, by1) = (bx0.min(x), bx1.max(x + 1), by0.min(y), by1.max(y + 1));
                }
            }
        }
        (by0..by1).map(|y| (bx0..bx1).map(|x| a(x, y)).collect()).collect()
    }

    #[test]
    fn quarter_turn_text_is_the_rotated_bitmap() {
        let black = Color::rgb(0.0, 0.0, 0.0);
        let l = crate::text::layout(&"Hg(".into(), 14.0, Font::Regular, black);
        // Anchors chosen so every run starts at the same subpixel phase (0.6 px) along its advance,
        // so all three use the same atlas bitmaps.
        let mut glyphs = crate::text::place(&l, [20.3, 40.2], (0.0, 0.0), 0.0);
        glyphs.extend(crate::text::place(&l, [80.3, 60.2], (0.0, 0.0), std::f64::consts::FRAC_PI_2));
        glyphs.extend(crate::text::place(&l, [160.3, 30.3], (0.0, 0.0), -std::f64::consts::FRAC_PI_2));
        glyphs.extend(crate::text::place(&l, [260.0, 40.0], (0.0, 0.0), 0.5));
        let Some(img) = render(glyphs, [300.0, 80.0]) else { return };
        let up = ink(&img, 0, 120, 0, 160);
        let ccw = ink(&img, 120, 240, 0, 160);
        let cw = ink(&img, 240, 400, 0, 160);
        let (h, w) = (up.len(), up[0].len());
        assert!(h > 10 && w > 20, "{w}x{h}");
        assert_eq!((ccw.len(), ccw[0].len()), (w, h));
        for j in 0..h {
            for i in 0..w {
                // 90° CCW: local (i, j) -> device (j, -i); 90° CW: (-j, i).
                assert_eq!(ccw[w - 1 - i][j], up[j][i], "ccw at ({i}, {j})");
                assert_eq!(cw[i][h - 1 - j], up[j][i], "cw at ({i}, {j})");
            }
        }
        // Axis-aligned text reaches full coverage (not blurred by resampling).
        assert!(up.iter().flatten().any(|&a| a == 255));
        // The arbitrary angle draws about as much ink.
        let slanted = ink(&img, 480, 600, 0, 160);
        let sum = |v: &Vec<Vec<u8>>| v.iter().flatten().map(|&a| a as f64).sum::<f64>();
        let ratio = sum(&slanted) / sum(&up);
        assert!((ratio - 1.0).abs() < 0.03, "{ratio}");
    }

    #[test]
    fn arbitrary_angles_sample_linearly() {
        let p = place(&glyph([10.3, 20.4], 0.5), [2.0, 2.0, 0.0, 0.0], 2.0).unwrap();
        assert!(p.linear);
        assert_eq!(p.origin, [20.6, 40.8]);
        assert!(place(&glyph([f32::NAN, 0.0], 0.0), [2.0, 2.0, 0.0, 0.0], 2.0).is_none());
    }
}
