//! Pipeline and bind-group-layout creation.

use super::{MSAA, TARGET_FORMAT};

pub(crate) struct Pipelines {
    pub globals_layout: wgpu::BindGroupLayout,
    pub mesh_layout: wgpu::BindGroupLayout,
    pub sprite_layout: wgpu::BindGroupLayout,
    pub mesh: wgpu::RenderPipeline,
    pub sprite: wgpu::RenderPipeline,
    pub sampler: wgpu::Sampler,
}

const COMMON: &str = include_str!("shaders/common.wgsl");

fn module(device: &wgpu::Device, label: &str, src: &str) -> wgpu::ShaderModule {
    let code = format!("{COMMON}\n{src}");
    device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(code.into()),
    })
}

fn uniform_entry(binding: u32, dynamic: bool) -> wgpu::BindGroupLayoutEntry {
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

fn storage_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
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

fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn target() -> [Option<wgpu::ColorTargetState>; 1] {
    [Some(wgpu::ColorTargetState {
        format: TARGET_FORMAT,
        blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
        write_mask: wgpu::ColorWrites::ALL,
    })]
}

fn multisample() -> wgpu::MultisampleState {
    wgpu::MultisampleState {
        count: MSAA,
        mask: !0,
        alpha_to_coverage_enabled: false,
    }
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
        let mesh_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mesh"),
            entries: &[uniform_entry(0, true)],
        });
        let sprite_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sprite"),
            entries: &[
                uniform_entry(0, true),
                storage_entry(1),
                storage_entry(2),
                storage_entry(3),
                storage_entry(4),
                texture_entry(5),
            ],
        });

        let mesh_mod = module(device, "mesh", include_str!("shaders/mesh.wgsl"));
        let mesh_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mesh"),
            bind_group_layouts: &[Some(&globals_layout), Some(&mesh_layout)],
            immediate_size: 0,
        });
        let mesh_vb = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<crate::scene::drawlist::MeshVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Unorm8x4],
        };
        let mesh = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mesh"),
            layout: Some(&mesh_pl),
            vertex: wgpu::VertexState {
                module: &mesh_mod,
                entry_point: Some("vs_mesh"),
                compilation_options: Default::default(),
                buffers: &[Some(mesh_vb)],
            },
            fragment: Some(wgpu::FragmentState {
                module: &mesh_mod,
                entry_point: Some("fs_mesh"),
                compilation_options: Default::default(),
                targets: &target(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: multisample(),
            multiview_mask: None,
            cache: None,
        });

        let sprite_mod = module(device, "sprite", include_str!("shaders/sprite.wgsl"));
        let sprite_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sprite"),
            bind_group_layouts: &[Some(&globals_layout), Some(&sprite_layout)],
            immediate_size: 0,
        });
        let sprite = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sprite"),
            layout: Some(&sprite_pl),
            vertex: wgpu::VertexState {
                module: &sprite_mod,
                entry_point: Some("vs_marker"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &sprite_mod,
                entry_point: Some("fs_marker"),
                compilation_options: Default::default(),
                targets: &target(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: multisample(),
            multiview_mask: None,
            cache: None,
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("linear"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        Pipelines {
            globals_layout,
            mesh_layout,
            sprite_layout,
            mesh,
            sprite,
            sampler,
        }
    }
}
