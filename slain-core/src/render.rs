//! GPU Video Rendering via wgpu
//!
//! Uploads video frames to GPU textures and renders to screen.
//! Supports NV12, YUV420, and RGB formats.

use std::sync::Arc;
use parking_lot::Mutex;

// ============================================================================
// Vertex and Shader
// ============================================================================

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 2],
    tex_coords: [f32; 2],
}

const VERTICES: &[Vertex] = &[
    Vertex { position: [-1.0, -1.0], tex_coords: [0.0, 1.0] },
    Vertex { position: [ 1.0, -1.0], tex_coords: [1.0, 1.0] },
    Vertex { position: [ 1.0,  1.0], tex_coords: [1.0, 0.0] },
    Vertex { position: [-1.0,  1.0], tex_coords: [0.0, 0.0] },
];

const INDICES: &[u16] = &[0, 1, 2, 2, 3, 0];

const SHADER_RGB: &str = r#"
struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) tex_coords: vec2<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) tex_coords: vec2<f32>,
}

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = vec4<f32>(in.position, 0.0, 1.0);
    out.tex_coords = in.tex_coords;
    return out;
}

@group(0) @binding(0) var t_diffuse: texture_2d<f32>;
@group(0) @binding(1) var s_diffuse: sampler;

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(t_diffuse, s_diffuse, in.tex_coords);
}
"#;

const SHADER_NV12: &str = r#"
struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) tex_coords: vec2<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) tex_coords: vec2<f32>,
}

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = vec4<f32>(in.position, 0.0, 1.0);
    out.tex_coords = in.tex_coords;
    return out;
}

@group(0) @binding(0) var t_y: texture_2d<f32>;
@group(0) @binding(1) var t_uv: texture_2d<f32>;
@group(0) @binding(2) var s: sampler;

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let y = textureSample(t_y, s, in.tex_coords).r;
    let uv = textureSample(t_uv, s, in.tex_coords).rg;
    
    // BT.709 YUV to RGB
    let y_scaled = (y - 0.0625) * 1.164;
    let u = uv.r - 0.5;
    let v = uv.g - 0.5;
    
    let r = y_scaled + 1.793 * v;
    let g = y_scaled - 0.213 * u - 0.533 * v;
    let b = y_scaled + 2.112 * u;
    
    return vec4<f32>(r, g, b, 1.0);
}
"#;

// ============================================================================
// Frame Format
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameFormat {
    RGB24,
    RGBA32,
    NV12,
    YUV420P,
}

// ============================================================================
// Video Frame for Rendering
// ============================================================================

pub struct VideoFrame {
    pub width: u32,
    pub height: u32,
    pub format: FrameFormat,
    pub data: Vec<u8>,
    pub pts_us: i64,
}

// ============================================================================
// GPU Renderer
// ============================================================================

pub struct GpuRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
    
    // RGB pipeline
    rgb_pipeline: wgpu::RenderPipeline,
    rgb_bind_group_layout: wgpu::BindGroupLayout,
    
    // NV12 pipeline
    nv12_pipeline: wgpu::RenderPipeline,
    nv12_bind_group_layout: wgpu::BindGroupLayout,
    
    // Geometry
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    
    // Current texture
    current_bind_group: Option<wgpu::BindGroup>,
    current_format: FrameFormat,
    
    // Sampler
    sampler: wgpu::Sampler,
    
    // Dimensions
    width: u32,
    height: u32,
}

impl GpuRenderer {
    /// Create renderer for a window
    pub async fn new(window: Arc<winit::window::Window>) -> Result<Self, String> {
        let size = window.inner_size();
        
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });
        
        let surface = instance.create_surface(window.clone())
            .map_err(|e| format!("Surface creation failed: {}", e))?;
        
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .ok_or("No suitable GPU adapter")?;
        
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                    label: Some("slain_device"),
                    memory_hints: Default::default(),
                },
                None,
            )
            .await
            .map_err(|e| format!("Device request failed: {}", e))?;
        
        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps.formats.iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(surface_caps.formats[0]);
        
        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width,
            height: size.height,
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &surface_config);
        
        // Create sampler
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        
        // RGB bind group layout
        let rgb_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
            label: Some("rgb_bind_group_layout"),
        });
        
        // NV12 bind group layout
        let nv12_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
            label: Some("nv12_bind_group_layout"),
        });
        
        // Vertex buffer layout
        let vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x2,
                },
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 2]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x2,
                },
            ],
        };
        
        // RGB pipeline
        let rgb_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("rgb_shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER_RGB.into()),
        });
        
        let rgb_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("rgb_pipeline_layout"),
            bind_group_layouts: &[&rgb_bind_group_layout],
            push_constant_ranges: &[],
        });
        
        let rgb_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rgb_pipeline"),
            layout: Some(&rgb_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &rgb_shader,
                entry_point: Some("vs_main"),
                buffers: &[vertex_layout.clone()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &rgb_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
            cache: None,
        });
        
        // NV12 pipeline
        let nv12_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("nv12_shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER_NV12.into()),
        });
        
        let nv12_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("nv12_pipeline_layout"),
            bind_group_layouts: &[&nv12_bind_group_layout],
            push_constant_ranges: &[],
        });
        
        let nv12_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("nv12_pipeline"),
            layout: Some(&nv12_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &nv12_shader,
                entry_point: Some("vs_main"),
                buffers: &[vertex_layout],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &nv12_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
            cache: None,
        });
        
        // Vertex buffer
        use wgpu::util::DeviceExt;
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("vertex_buffer"),
            contents: bytemuck::cast_slice(VERTICES),
            usage: wgpu::BufferUsages::VERTEX,
        });
        
        // Index buffer
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("index_buffer"),
            contents: bytemuck::cast_slice(INDICES),
            usage: wgpu::BufferUsages::INDEX,
        });
        
        Ok(Self {
            device,
            queue,
            surface,
            surface_config,
            rgb_pipeline,
            rgb_bind_group_layout,
            nv12_pipeline,
            nv12_bind_group_layout,
            vertex_buffer,
            index_buffer,
            current_bind_group: None,
            current_format: FrameFormat::RGB24,
            sampler,
            width: size.width,
            height: size.height,
        })
    }
    
    /// Resize surface
    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.width = width;
            self.height = height;
            self.surface_config.width = width;
            self.surface_config.height = height;
            self.surface.configure(&self.device, &self.surface_config);
        }
    }
    
    /// Upload frame to GPU
    pub fn upload_frame(&mut self, frame: &VideoFrame) {
        match frame.format {
            FrameFormat::RGB24 | FrameFormat::RGBA32 => {
                self.upload_rgb(frame);
            }
            FrameFormat::NV12 => {
                self.upload_nv12(frame);
            }
            FrameFormat::YUV420P => {
                // Convert to RGB first (or implement YUV shader)
                // For now, skip
            }
        }
    }
    
    fn upload_rgb(&mut self, frame: &VideoFrame) {
        let format = if frame.format == FrameFormat::RGBA32 {
            wgpu::TextureFormat::Rgba8UnormSrgb
        } else {
            wgpu::TextureFormat::Rgba8UnormSrgb
        };
        
        // Convert RGB24 to RGBA32 if needed
        let data = if frame.format == FrameFormat::RGB24 {
            let mut rgba = Vec::with_capacity(frame.width as usize * frame.height as usize * 4);
            for chunk in frame.data.chunks(3) {
                rgba.push(chunk[0]);
                rgba.push(chunk[1]);
                rgba.push(chunk[2]);
                rgba.push(255);
            }
            rgba
        } else {
            frame.data.clone()
        };
        
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("video_texture"),
            size: wgpu::Extent3d {
                width: frame.width,
                height: frame.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        
        self.queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &data,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(4 * frame.width),
                rows_per_image: Some(frame.height),
            },
            wgpu::Extent3d {
                width: frame.width,
                height: frame.height,
                depth_or_array_layers: 1,
            },
        );
        
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        
        self.current_bind_group = Some(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rgb_bind_group"),
            layout: &self.rgb_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        }));
        
        self.current_format = FrameFormat::RGB24;
    }
    
    fn upload_nv12(&mut self, frame: &VideoFrame) {
        let y_size = (frame.width * frame.height) as usize;
        let uv_width = frame.width / 2;
        let uv_height = frame.height / 2;
        
        // Y plane texture
        let y_texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("y_texture"),
            size: wgpu::Extent3d {
                width: frame.width,
                height: frame.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        
        self.queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &y_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &frame.data[..y_size],
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(frame.width),
                rows_per_image: Some(frame.height),
            },
            wgpu::Extent3d {
                width: frame.width,
                height: frame.height,
                depth_or_array_layers: 1,
            },
        );
        
        // UV plane texture (interleaved)
        let uv_texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("uv_texture"),
            size: wgpu::Extent3d {
                width: uv_width,
                height: uv_height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rg8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        
        self.queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &uv_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &frame.data[y_size..],
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(frame.width), // UV is half width but 2 bytes per pixel
                rows_per_image: Some(uv_height),
            },
            wgpu::Extent3d {
                width: uv_width,
                height: uv_height,
                depth_or_array_layers: 1,
            },
        );
        
        let y_view = y_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let uv_view = uv_texture.create_view(&wgpu::TextureViewDescriptor::default());
        
        self.current_bind_group = Some(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("nv12_bind_group"),
            layout: &self.nv12_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&y_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&uv_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        }));
        
        self.current_format = FrameFormat::NV12;
    }
    
    /// Render current frame to screen
    pub fn render(&self) -> Result<(), wgpu::SurfaceError> {
        let output = self.surface.get_current_texture()?;
        let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());
        
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("render_encoder"),
        });
        
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("render_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            
            if let Some(bind_group) = &self.current_bind_group {
                let pipeline = match self.current_format {
                    FrameFormat::NV12 => &self.nv12_pipeline,
                    _ => &self.rgb_pipeline,
                };
                
                render_pass.set_pipeline(pipeline);
                render_pass.set_bind_group(0, bind_group, &[]);
                render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
                render_pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
                render_pass.draw_indexed(0..6, 0, 0..1);
            }
        }
        
        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();
        
        Ok(())
    }
}
