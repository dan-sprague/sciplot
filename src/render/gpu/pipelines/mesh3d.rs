//! `mesh3d`: Axis3 triangle meshes (surfaces). Vertices (position + data-space normal, stride 24)
//! and per-vertex colors or values are vertex-step buffers; the pipeline writes depth so lines and
//! markers drawn afterwards are hidden behind the mesh.

use super::super::frame::{CMapU, DrawCmd, Frame, premul};
use super::Layouts;
use super::view3d::{View3U, view_uniform};
use crate::scene::drawlist::{Mesh3dPrim, PrimColor};
use bytemuck::{Pod, Zeroable};

pub(crate) const SHADER: &str = include_str!("mesh3d.wgsl");

/// WGSL `Mesh3dU`.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Mesh3dU {
    v: View3U,
    s: [f32; 4],
    eye: [f32; 4],
    light: [f32; 4],
    ns: [f32; 4],
    color: [f32; 4],
    color_mode: u32,
    diffuse: f32,
    specular: f32,
    _p: u32,
    cm: CMapU,
}

pub(crate) fn layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("mesh3d"),
        entries: &[super::uniform_entry(0, true), super::texture_entry(1, true)],
    })
}

pub(crate) fn pipeline(
    device: &wgpu::Device,
    l: &Layouts,
    shader: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    super::view3d::pipeline(
        device,
        "mesh3d",
        shader,
        "vs_mesh3d",
        "fs_mesh3d",
        l,
        &l.mesh3d,
        &[
            Some(wgpu::VertexBufferLayout {
                array_stride: 24,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3],
            }),
            Some(wgpu::VertexBufferLayout {
                array_stride: 4,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![2 => Uint32],
            }),
        ],
        wgpu::PrimitiveTopology::TriangleList,
        format,
        true,
    )
}

pub(crate) fn prepare(f: &mut Frame, m: &Mesh3dPrim) -> Option<DrawCmd> {
    let n = m.verts.len() / 3 * 3;
    if n == 0 || n > u32::MAX as usize {
        return None;
    }
    let verts = f.vertex(&m.verts);
    let (color_mode, color, col, cm, lut) = match &m.color {
        PrimColor::Uniform(c) => (0, premul(*c), None, CMapU::default(), f.dummy_lut()),
        PrimColor::PerElement(b) if b.len() >= n => (1, [0.0; 4], Some(f.vertex(b)), CMapU::default(), f.dummy_lut()),
        PrimColor::Values(b, map) if b.len() >= n => {
            (2, [0.0; 4], Some(f.vertex(b)), CMapU::from(map), f.lut(&map.lut))
        }
        _ => {
            crate::warn_once("3D mesh colors have fewer entries than vertices; drawing nothing");
            return None;
        }
    };
    let v = &m.view;
    let eye_local: [f64; 3] = std::array::from_fn(|i| (v.eye[i] - v.world_offset[i]) / v.world_scale[i]);
    let (shading, mat) = match m.shading {
        Some(mat) => (1.0, mat),
        None => (0.0, crate::scene::drawlist::Material { diffuse: 1.0, specular: 0.0, shininess: 1.0 }),
    };
    let f3 = |a: [f64; 3], w: f32| [a[0] as f32, a[1] as f32, a[2] as f32, w];
    let offset = f.push_uniform(&Mesh3dU {
        v: view_uniform(v, f.ppu, f.size),
        s: f3(v.world_scale, v.ambient),
        eye: f3(eye_local, v.light_color),
        light: f3(v.light_dir, shading),
        ns: f3(v.normal_scale, mat.shininess),
        color,
        color_mode,
        diffuse: mat.diffuse,
        specular: mat.specular,
        _p: 0,
        cm,
    });
    let bind = f.device().create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("mesh3d"),
        layout: &f.layouts().mesh3d,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: f.uniform_binding::<Mesh3dU>() },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&lut) },
        ],
    });
    let col = col.unwrap_or_else(|| verts.clone());
    Some(DrawCmd {
        pipeline: f.pipes.mesh3d.clone(),
        bind,
        offset,
        vbs: vec![(verts, 0), (col, 0)],
        vertices: 0..n as u32,
        instances: 0..1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uniform_layout_matches_wgsl() {
        assert_eq!(std::mem::size_of::<Mesh3dU>(), 256);
    }
}
