//! Minimal winit 0.30.13 + wgpu 30.0.1 web probe (WebGPU with WebGL2 fallback).
#![cfg(target_arch = "wasm32")]
use std::sync::Arc;

use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wgpu::util::DeviceExt;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
use winit::platform::web::{EventLoopExtWebSys, WindowAttributesExtWebSys};
use winit::window::{Window, WindowId};

const MSAA: u32 = 4;
const SHADER: &str = r#"
struct Obj { shift: vec2<f32>, tint: f32, t: f32 };
@group(0) @binding(0) var<uniform> obj: Obj;            // dynamic offset
@group(0) @binding(1) var data_tex: texture_2d<f32>;     // R32Float, textureLoad only
@group(0) @binding(2) var lut: texture_2d<f32>;          // Rgba8Unorm colormap
@group(0) @binding(3) var lin_samp: sampler;

struct VOut { @builtin(position) pos: vec4<f32>, @location(0) color: vec4<f32> };

@vertex
fn vs(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32,
      @location(0) center: vec2<f32>, @location(1) half: f32, @location(2) kind: f32) -> VOut {
    var corners = array<vec2<f32>, 6>(vec2(-1.0, -1.0), vec2(1.0, -1.0), vec2(1.0, 1.0),
                                       vec2(-1.0, -1.0), vec2(1.0, 1.0), vec2(-1.0, 1.0));
    var o: VOut;
    o.pos = vec4<f32>(center + obj.shift + corners[vi] * half, 0.0, 1.0);
    if (kind > 0.5) {
        o.color = vec4<f32>(0.0, 0.0, 0.0, 0.5);              // premultiplied 50% black
    } else {
        let v = textureLoad(data_tex, vec2<i32>(i32(ii % 16u), i32(ii / 16u)), 0).r;
        let c = textureSampleLevel(lut, lin_samp, vec2<f32>(v, 0.5), 0.0);  // VS texture sample
        o.color = vec4<f32>(c.rgb * obj.tint, 1.0);
    }
    return o;
}
@fragment fn fs(i: VOut) -> @location(0) vec4<f32> { return i.color; }
"#;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Obj { shift: [f32; 2], tint: f32, t: f32 }

struct Gpu {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    format: wgpu::TextureFormat,
    backend: wgpu::Backend,
    pipeline: wgpu::RenderPipeline,
    bind: wgpu::BindGroup,
    ubuf: wgpu::Buffer,
    inst: wgpu::Buffer,
    msaa: Option<wgpu::TextureView>,
    config: Option<wgpu::SurfaceConfiguration>,
}

enum UserEvent { Ready(Box<Gpu>), Failed(String) }

struct App {
    proxy: EventLoopProxy<UserEvent>,
    window: Option<Arc<Window>>,
    gpu: Option<Box<Gpu>>,
    start: web_time::Instant,
    frames: u64,
}

fn document() -> web_sys::Document { web_sys::window().unwrap().document().unwrap() }

async fn init_gpu(window: Arc<Window>) -> Result<Gpu, String> {
    let force_gl = web_sys::window()
        .and_then(|w| w.location().search().ok())
        .is_some_and(|q| q.contains("backend=gl"));
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
    desc.backends = if force_gl { wgpu::Backends::GL } else { wgpu::Backends::BROWSER_WEBGPU | wgpu::Backends::GL };
    // Instance::new alone would pick WebGPU whenever navigator.gpu exists, even if requestAdapter() -> null.
    let instance = wgpu::util::new_instance_with_webgpu_detection(desc).await;
    let surface = instance.create_surface(window.clone()).map_err(|e| e.to_string())?;
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface), // mandatory for WebGL2
            ..Default::default()
        })
        .await
        .map_err(|e| e.to_string())?;
    let info = adapter.get_info();
    log::info!("adapter: {:?} {:?} driver={:?}", info.backend, info.name, info.driver_info);
    let l = adapter.limits();
    log::info!(
        "limits: tex2d={} ubo_align={} ubo_size={} vb_stride={} vbufs={} storage_bufs_vs={} inter_stage={} dyn_ubo={}",
        l.max_texture_dimension_2d, l.min_uniform_buffer_offset_alignment, l.max_uniform_buffer_binding_size,
        l.max_vertex_buffer_array_stride, l.max_vertex_buffers, l.max_storage_buffers_per_shader_stage,
        l.max_inter_stage_shader_variables, l.max_dynamic_uniform_buffers_per_pipeline_layout
    );
    log::info!("downlevel: {:?}", adapter.get_downlevel_capabilities().flags);
    for f in [wgpu::TextureFormat::R32Float, wgpu::TextureFormat::Rgba8Unorm, wgpu::TextureFormat::Bgra8Unorm] {
        log::info!("{f:?}: {:?}", adapter.get_texture_format_features(f).flags);
    }
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("probe"),
            required_limits: wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits()),
            ..Default::default()
        })
        .await
        .map_err(|e| e.to_string())?;
    let caps = surface.get_capabilities(&adapter);
    log::info!("surface formats={:?} alpha={:?} present={:?}", caps.formats, caps.alpha_modes, caps.present_modes);
    let format = caps
        .formats
        .iter()
        .copied()
        .find(|f| matches!(f, wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Bgra8Unorm))
        .ok_or("no non-sRGB 8-bit surface format")?;

    // Data texture (R32Float, unfilterable) and LUT (Rgba8Unorm).
    let vals: Vec<f32> = (0..256).map(|k| ((k % 16) + (k / 16)) as f32 / 30.0).collect();
    let data_tex = device.create_texture_with_data(
        &queue,
        &wgpu::TextureDescriptor {
            label: Some("data"),
            size: wgpu::Extent3d { width: 16, height: 16, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        bytemuck::cast_slice(&vals),
    );
    let lut_px: Vec<u8> = (0..256u32).flat_map(|i| [i as u8, 64, (255 - i) as u8, 255]).collect();
    let lut = device.create_texture_with_data(
        &queue,
        &wgpu::TextureDescriptor {
            label: Some("lut"),
            size: wgpu::Extent3d { width: 256, height: 1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &lut_px,
    );
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    // Two 256-byte-aligned uniform blocks addressed by dynamic offsets 0 and 256.
    let ubuf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("obj"),
        size: 512,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let vis = wgpu::ShaderStages::VERTEX_FRAGMENT;
    let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("probe"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: vis,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new(16),
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: vis,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: vis,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 3,
                visibility: vis,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });
    let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("probe"),
        layout: &bgl,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &ubuf,
                    offset: 0,
                    size: wgpu::BufferSize::new(16),
                }),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&data_tex.create_view(&Default::default())),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(&lut.create_view(&Default::default())),
            },
            wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&sampler) },
        ],
    });
    // Instance records [center.x, center.y, half, kind]; record k+1 holds the *center* of cell k,
    // so slot 1 (same buffer, +16 B offset) supplies centers: one buffer bound at two offsets.
    let mut cells: Vec<[f32; 4]> = (0..256)
        .map(|k| {
            let (i, j) = ((k % 16) as f32, (k / 16) as f32);
            [-0.95 + 0.9 * (i + 0.5) / 16.0, -0.9 + 1.8 * (j + 0.5) / 16.0, 0.025, 0.0]
        })
        .collect();
    cells.push([0.5, 0.0, 0.3, 1.0]); // blend-test quad
    let mut recs = vec![[0.0f32; 4]; cells.len() + 1];
    for (k, c) in cells.iter().enumerate() {
        recs[k][2] = c[2];
        recs[k][3] = c[3];
        recs[k + 1][0] = c[0];
        recs[k + 1][1] = c[1];
    }
    let inst = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("instances"),
        contents: bytemuck::cast_slice(&recs),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("probe"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("probe"),
        bind_group_layouts: &[Some(&bgl)],
        immediate_size: 0,
    });
    let attrs0 = [
        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32, offset: 8, shader_location: 1 },
        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32, offset: 12, shader_location: 2 },
    ];
    let attrs1 = [wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 0, shader_location: 0 }];
    let buffers = [
        Some(wgpu::VertexBufferLayout { array_stride: 16, step_mode: wgpu::VertexStepMode::Instance, attributes: &attrs0 }),
        Some(wgpu::VertexBufferLayout { array_stride: 16, step_mode: wgpu::VertexStepMode::Instance, attributes: &attrs1 }),
    ];
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("probe"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs"),
            compilation_options: Default::default(),
            buffers: &buffers,
        },
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fs"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState { count: MSAA, mask: !0, alpha_to_coverage_enabled: false },
        multiview_mask: None,
        cache: None,
    });
    Ok(Gpu { surface, device, queue, format, backend: info.backend, pipeline, bind, ubuf, inst, msaa: None, config: None })
}

impl Gpu {
    fn resize(&mut self, size: PhysicalSize<u32>) {
        if size.width == 0 || size.height == 0 {
            return; // canvas not laid out yet (winit reports 0x0 until the ResizeObserver fires)
        }
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: self.format,
            color_space: wgpu::SurfaceColorSpace::Srgb,
            width: size.width,
            height: size.height,
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: wgpu::CompositeAlphaMode::Opaque,
            view_formats: vec![],
        };
        self.surface.configure(&self.device, &config); // also sets canvas.width/height
        let msaa = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("msaa"),
            size: wgpu::Extent3d { width: size.width, height: size.height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: MSAA,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        self.msaa = Some(msaa.create_view(&Default::default()));
        self.config = Some(config);
    }

    fn render(&mut self, t: f32) -> bool {
        let Some(msaa) = &self.msaa else { return false };
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            other => {
                log::warn!("get_current_texture: {}", match other {
                    wgpu::CurrentSurfaceTexture::Lost => "lost",
                    wgpu::CurrentSurfaceTexture::Outdated => "outdated",
                    _ => "other",
                });
                if let Some(c) = &self.config {
                    self.surface.configure(&self.device, c);
                }
                return false;
            }
        };
        let view = frame.texture.create_view(&Default::default());
        let a = Obj { shift: [0.0, 0.0], tint: 1.0, t };
        let b = Obj { shift: [0.0, 0.0], tint: 0.5, t };
        self.queue.write_buffer(&self.ubuf, 0, bytemuck::bytes_of(&a));
        self.queue.write_buffer(&self.ubuf, 256, bytemuck::bytes_of(&b));
        let mut enc = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("probe"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: msaa,
                    depth_slice: None,
                    resolve_target: Some(&view),
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::WHITE), store: wgpu::StoreOp::Discard },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_vertex_buffer(0, self.inst.slice(..));
            pass.set_vertex_buffer(1, self.inst.slice(16..)); // same buffer, second offset
            pass.set_bind_group(0, &self.bind, &[0]);
            pass.draw(0..6, 0..128);
            pass.set_bind_group(0, &self.bind, &[256]); // dynamic offset -> tint 0.5
            pass.draw(0..6, 128..256); // first_instance != 0 (emulated on WebGL2)
            pass.set_bind_group(0, &self.bind, &[0]);
            pass.draw(0..6, 256..257); // 50% black over white: 128 if blending is gamma-space
        }
        self.queue.submit([enc.finish()]);
        self.queue.present(frame); // wgpu 30: Queue::present, not SurfaceTexture::present
        true
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let canvas = document()
            .get_element_by_id("sciplot")
            .and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok());
        let attrs = Window::default_attributes()
            .with_canvas(canvas) // None -> winit creates one ...
            .with_append(true) // ... and appends it to <body> (no-op if already in the DOM)
            .with_prevent_default(true);
        let window = Arc::new(el.create_window(attrs).expect("create_window"));
        log::info!("created: inner_size={:?} scale_factor={}", window.inner_size(), window.scale_factor());
        self.window = Some(window.clone());
        let proxy = self.proxy.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let ev = match init_gpu(window).await {
                Ok(g) => UserEvent::Ready(Box::new(g)),
                Err(e) => UserEvent::Failed(e),
            };
            let _ = proxy.send_event(ev);
        });
    }

    fn user_event(&mut self, _el: &ActiveEventLoop, ev: UserEvent) {
        match ev {
            UserEvent::Ready(mut gpu) => {
                let w = self.window.as_ref().unwrap();
                gpu.resize(w.inner_size());
                log::info!("gpu ready: backend={:?} format={:?} size={:?}", gpu.backend, gpu.format, w.inner_size());
                self.gpu = Some(gpu);
                w.request_redraw();
            }
            UserEvent::Failed(e) => {
                log::error!("gpu init failed: {e}");
                document().set_title("done:failed");
            }
        }
    }

    fn window_event(&mut self, _el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::Resized(size) => {
                log::info!("Resized({size:?})");
                if let Some(g) = &mut self.gpu {
                    g.resize(size);
                }
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => log::info!("ScaleFactorChanged({scale_factor})"),
            WindowEvent::RedrawRequested => {
                let Some(g) = &mut self.gpu else { return };
                if g.render(self.start.elapsed().as_secs_f32()) {
                    self.frames += 1;
                    if self.frames == 120 {
                        let s = self.start.elapsed().as_secs_f32();
                        log::info!("120 frames in {s:.2}s");
                        document().set_title(&format!("done:{:?}", g.backend));
                    }
                }
                self.window.as_ref().unwrap().request_redraw(); // next requestAnimationFrame
            }
            WindowEvent::RedrawRequested | WindowEvent::Resized(_) => {}
            other => {
                if self.gpu.is_some() {
                    log::info!("event {other:?}");
                }
            }
        }
    }
}

#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
    console_log::init_with_level(log::Level::Info).ok();
    let event_loop = EventLoop::<UserEvent>::with_user_event().build().expect("event loop");
    let app = App { proxy: event_loop.create_proxy(), window: None, gpu: None, start: web_time::Instant::now(), frames: 0 };
    event_loop.spawn_app(app); // returns immediately; the browser drives the loop
}
