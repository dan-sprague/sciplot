//! `sprite`: scatter markers as instanced quads with analytic SDFs.

use super::super::frame::{CMapU, DrawCmd, Frame, premul};
use crate::scene::drawlist::{MarkersPrim, PrimColor};
use bytemuck::{Pod, Zeroable};

pub(crate) struct SpritePipeline {
    pub layout: wgpu::BindGroupLayout,
    pub pipeline: wgpu::RenderPipeline,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct SpriteU {
    xform: [f32; 4],
    color: [f32; 4],
    stroke_color: [f32; 4],
    size: f32,
    stroke: f32,
    shape: u32,
    col_mode: u32,
    size_stride: u32,
    rotation: f32,
    _p0: u32,
    _p1: u32,
    cm: CMapU,
}

pub(crate) fn create(device: &wgpu::Device, globals: &wgpu::BindGroupLayout) -> SpritePipeline {
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("sprite"),
        entries: &[
            super::uniform_entry(0, true),
            super::storage_entry(1),
            super::storage_entry(2),
            super::storage_entry(3),
            super::storage_entry(4),
            super::texture_entry(5, true),
        ],
    });
    let shader = super::module(device, "sprite", include_str!("sprite.wgsl"));
    let pipeline = super::pipeline(
        device,
        "sprite",
        &shader,
        "vs_marker",
        "fs_marker",
        globals,
        &layout,
        &[],
        wgpu::PrimitiveTopology::TriangleStrip,
    );
    SpritePipeline { layout, pipeline }
}

pub(crate) fn prepare(f: &mut Frame, m: &MarkersPrim, xform: [f32; 4]) -> Option<DrawCmd> {
    if m.pos.data.is_empty() {
        return None;
    }
    let pos = f.storage(&m.pos);
    let (col_mode, color, col, val, cm, lut) = match &m.color {
        PrimColor::Uniform(c) => (0, premul(*c), f.dummy(), f.dummy(), CMapU::default(), f.dummy_lut()),
        PrimColor::PerElement(b) => (1, [0.0; 4], f.storage(b), f.dummy(), CMapU::default(), f.dummy_lut()),
        PrimColor::Values(b, map) => (2, [0.0; 4], f.dummy(), f.storage(b), CMapU::from(map), f.lut(&map.lut)),
    };
    let (size_stride, sz) = match &m.sizes {
        Some(s) => (1, f.storage(s)),
        None => (0, f.dummy()),
    };
    let offset = f.push_uniform(&SpriteU {
        xform,
        color,
        stroke_color: premul(m.stroke_color),
        size: m.size,
        stroke: m.stroke_width,
        shape: m.marker.shader_id(),
        col_mode,
        size_stride,
        rotation: m.rotation,
        _p0: 0,
        _p1: 0,
        cm,
    });
    let bind = f.device().create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("sprite"),
        layout: &f.pipes.sprite.layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: f.uniform_binding::<SpriteU>() },
            wgpu::BindGroupEntry { binding: 1, resource: pos.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 2, resource: col.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 3, resource: val.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 4, resource: sz.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(&lut) },
        ],
    });
    Some(DrawCmd {
        pipeline: f.pipes.sprite.pipeline.clone(),
        bind,
        offset,
        vb: None,
        vertices: 0..4,
        instances: 0..m.pos.data.len() as u32,
    })
}
