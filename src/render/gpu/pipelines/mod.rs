//! Render pipelines. Each pipeline lives in its own module with its WGSL file and exposes:
//! - `layout(device) -> BindGroupLayout` (group 1) and `SHADER` (its WGSL),
//! - `pipeline(device, &Layouts, &ShaderModule, format) -> RenderPipeline` for one target format,
//! - `prepare(frame, prim, xform) -> Option<DrawCmd>` turning one draw-list primitive into a draw.
//!
//! Portability (WebGPU and WebGL2): no storage buffers. Per-element data comes in through
//! instance-step vertex buffers (at most 8 buffers and 16 attributes per pipeline), large arrays
//! through `R32Float` data textures read with `textureLoad`; uniform blocks stay under 256 bytes.
//!
//! Adding a pipeline = a new module, one field in [`Layouts`] and [`Pipelines`] (plus their
//! constructors and [`sources`]), and one match arm in `Renderer::render`.

pub(crate) mod field;
pub(crate) mod glyph;
pub(crate) mod line;
pub(crate) mod lines3d;
pub(crate) mod markers3d;
pub(crate) mod mesh;
pub(crate) mod mesh3d;
pub(crate) mod sprite;
pub(crate) mod view3d;

use super::MSAA;

/// Device-wide objects shared by every target format: bind-group layouts, samplers, shaders.
pub(crate) struct Layouts {
    pub globals: wgpu::BindGroupLayout,
    pub sampler: wgpu::Sampler,
    pub nearest: wgpu::Sampler,
    pub line: wgpu::BindGroupLayout,
    pub mesh: wgpu::BindGroupLayout,
    pub field: wgpu::BindGroupLayout,
    pub sprite: wgpu::BindGroupLayout,
    pub glyph: wgpu::BindGroupLayout,
    pub lines3d: wgpu::BindGroupLayout,
    pub markers3d: wgpu::BindGroupLayout,
    pub mesh3d: wgpu::BindGroupLayout,
    shaders: [wgpu::ShaderModule; 8],
}

/// The WGSL of every pipeline (with `common.wgsl` prepended), by name.
pub(crate) fn sources() -> [(&'static str, String); 8] {
    [
        ("line", with_common(line::SHADER)),
        ("mesh", with_common(mesh::SHADER)),
        ("field", with_common(field::SHADER)),
        ("sprite", with_common(sprite::SHADER)),
        ("glyph", with_common(glyph::SHADER)),
        ("lines3d", with_common(&format!("{}\n{}", view3d::SHADER, lines3d::SHADER))),
        ("markers3d", with_common(&format!("{}\n{}", view3d::SHADER, markers3d::SHADER))),
        ("mesh3d", with_common(&format!("{}\n{}", view3d::SHADER, mesh3d::SHADER))),
    ]
}

impl Layouts {
    pub fn new(device: &wgpu::Device) -> Layouts {
        let globals = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals"),
            entries: &[
                uniform_entry(0, false),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("linear"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let nearest = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("nearest"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let shaders = sources().map(|(name, code)| {
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(name),
                source: wgpu::ShaderSource::Wgsl(code.into()),
            })
        });
        Layouts {
            globals,
            sampler,
            nearest,
            line: line::layout(device),
            mesh: mesh::layout(device),
            field: field::layout(device),
            sprite: sprite::layout(device),
            glyph: glyph::layout(device),
            lines3d: lines3d::layout(device),
            markers3d: markers3d::layout(device),
            mesh3d: mesh3d::layout(device),
            shaders,
        }
    }
}

/// Every pipeline for one target format.
pub(crate) struct Pipelines {
    pub line: wgpu::RenderPipeline,
    pub mesh: wgpu::RenderPipeline,
    pub field: wgpu::RenderPipeline,
    pub sprite: wgpu::RenderPipeline,
    pub glyph: wgpu::RenderPipeline,
    pub lines3d: wgpu::RenderPipeline,
    pub markers3d: wgpu::RenderPipeline,
    pub mesh3d: wgpu::RenderPipeline,
}

impl Pipelines {
    pub fn new(device: &wgpu::Device, l: &Layouts, format: wgpu::TextureFormat) -> Pipelines {
        let [s_line, s_mesh, s_field, s_sprite, s_glyph, s_lines3d, s_markers3d, s_mesh3d] = &l.shaders;
        Pipelines {
            line: line::pipeline(device, l, s_line, format),
            mesh: mesh::pipeline(device, l, s_mesh, format),
            field: field::pipeline(device, l, s_field, format),
            sprite: sprite::pipeline(device, l, s_sprite, format),
            glyph: glyph::pipeline(device, l, s_glyph, format),
            lines3d: lines3d::pipeline(device, l, s_lines3d, format),
            markers3d: markers3d::pipeline(device, l, s_markers3d, format),
            mesh3d: mesh3d::pipeline(device, l, s_mesh3d, format),
        }
    }
}

const COMMON: &str = include_str!("../common.wgsl");

fn with_common(src: &str) -> String {
    format!("{COMMON}\n{src}")
}

pub(crate) fn uniform_entry(binding: u32, dynamic: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: dynamic,
            min_binding_size: None,
        },
        count: None,
    }
}

pub(crate) fn texture_entry(binding: u32, filterable: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

/// An instance-step vertex buffer holding one attribute at `location`.
pub(crate) fn instance_attr(
    stride: u64,
    attrs: &'static [wgpu::VertexAttribute],
) -> Option<wgpu::VertexBufferLayout<'static>> {
    Some(wgpu::VertexBufferLayout {
        array_stride: stride,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: attrs,
    })
}

/// The single color target every pipeline draws into (premultiplied "over").
pub(crate) fn target(format: wgpu::TextureFormat) -> [Option<wgpu::ColorTargetState>; 1] {
    [Some(wgpu::ColorTargetState {
        format,
        blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
        write_mask: wgpu::ColorWrites::ALL,
    })]
}

pub(crate) fn multisample() -> wgpu::MultisampleState {
    wgpu::MultisampleState { count: MSAA, mask: !0, alpha_to_coverage_enabled: false }
}

/// A standard pipeline: group 0 = globals, group 1 = `layout`, given vertex buffers and topology.
#[allow(clippy::too_many_arguments)]
pub(crate) fn pipeline(
    device: &wgpu::Device,
    label: &str,
    shader: &wgpu::ShaderModule,
    vs: &str,
    fs: &str,
    l: &Layouts,
    layout: &wgpu::BindGroupLayout,
    buffers: &[Option<wgpu::VertexBufferLayout<'_>>],
    topology: wgpu::PrimitiveTopology,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: &[Some(&l.globals), Some(layout)],
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(&pl),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some(vs),
            compilation_options: Default::default(),
            buffers,
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(fs),
            compilation_options: Default::default(),
            targets: &target(format),
        }),
        primitive: wgpu::PrimitiveState { topology, ..Default::default() },
        depth_stencil: None,
        multisample: multisample(),
        multiview_mask: None,
        cache: None,
    })
}
