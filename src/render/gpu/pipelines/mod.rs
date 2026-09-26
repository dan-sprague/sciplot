//! Render pipelines. Each pipeline lives in its own module with its WGSL file and exposes:
//! - `create(device, globals_layout) -> P` building the pipeline and its bind-group layout,
//! - `prepare(frame, prim, xform) -> Option<DrawCmd>` turning one draw-list primitive into a draw.
//!
//! Adding a pipeline = a new module, one field in [`Pipelines`], one line in `Pipelines::new`,
//! and one match arm in `Renderer::render`.

pub(crate) mod glyph;
pub(crate) mod mesh;
pub(crate) mod sprite;

use super::{MSAA, TARGET_FORMAT};

/// Every pipeline, created once per device.
pub(crate) struct Pipelines {
    pub globals_layout: wgpu::BindGroupLayout,
    pub sampler: wgpu::Sampler,
    pub nearest: wgpu::Sampler,
    pub mesh: mesh::MeshPipeline,
    pub sprite: sprite::SpritePipeline,
    pub glyph: glyph::GlyphPipeline,
}

impl Pipelines {
    pub fn new(device: &wgpu::Device) -> Pipelines {
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
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
        Pipelines {
            mesh: mesh::create(device, &globals_layout),
            sprite: sprite::create(device, &globals_layout),
            glyph: glyph::create(device, &globals_layout),
            globals_layout,
            sampler,
            nearest,
        }
    }
}

const COMMON: &str = include_str!("../common.wgsl");

/// Compiles `src` with the shared WGSL definitions prepended.
pub(crate) fn module(device: &wgpu::Device, label: &str, src: &str) -> wgpu::ShaderModule {
    let code = format!("{COMMON}\n{src}");
    device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(code.into()),
    })
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

pub(crate) fn storage_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
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

pub(crate) fn sampler_entry(binding: u32, filtering: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
        ty: wgpu::BindingType::Sampler(if filtering {
            wgpu::SamplerBindingType::Filtering
        } else {
            wgpu::SamplerBindingType::NonFiltering
        }),
        count: None,
    }
}

/// The single color target every pipeline draws into (premultiplied "over").
pub(crate) fn target() -> [Option<wgpu::ColorTargetState>; 1] {
    [Some(wgpu::ColorTargetState {
        format: TARGET_FORMAT,
        blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
        write_mask: wgpu::ColorWrites::ALL,
    })]
}

pub(crate) fn multisample() -> wgpu::MultisampleState {
    wgpu::MultisampleState { count: MSAA, mask: !0, alpha_to_coverage_enabled: false }
}

/// A standard pipeline: group 0 = globals, group 1 = `layout`, given vertex buffers and topology.
pub(crate) fn pipeline(
    device: &wgpu::Device,
    label: &str,
    shader: &wgpu::ShaderModule,
    vs: &str,
    fs: &str,
    globals_layout: &wgpu::BindGroupLayout,
    layout: &wgpu::BindGroupLayout,
    buffers: &[Option<wgpu::VertexBufferLayout<'_>>],
    topology: wgpu::PrimitiveTopology,
) -> wgpu::RenderPipeline {
    let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: &[Some(globals_layout), Some(layout)],
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
            targets: &target(),
        }),
        primitive: wgpu::PrimitiveState { topology, ..Default::default() },
        depth_stencil: None,
        multisample: multisample(),
        multiview_mask: None,
        cache: None,
    })
}
