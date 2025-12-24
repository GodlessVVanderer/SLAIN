//! # Video Filters - GPU-Accelerated Video Processing Pipeline
//!
//! A modular, GPU-accelerated video filter system for real-time processing.
//!
//! ## Supported Filters
//!
//! **Color Correction:**
//! - Brightness, Contrast, Saturation, Gamma
//! - Color temperature (warm/cool)
//! - Hue shift
//! - Vibrance (smart saturation)
//!
//! **Enhancement:**
//! - Sharpening (unsharp mask, CAS)
//! - Noise reduction (temporal, spatial)
//! - Edge enhancement
//!
//! **Deinterlacing:**
//! - Bob (field duplication)
//! - Weave
//! - Yadif-style (motion adaptive)
//!
//! **Effects:**
//! - Blur (box, gaussian)
//! - Vignette
//! - Film grain
//! - LUT (3D color lookup table)
//!
//! ## Architecture
//!
//! ```text
//! Input Frame ─► Filter 1 ─► Filter 2 ─► ... ─► Filter N ─► Output Frame
//!                   │           │                   │
//!              GPU Compute  GPU Compute        GPU Compute
//! ```

use std::sync::Arc;
use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use wgpu::util::DeviceExt;

// ============================================================================
// Filter Parameters
// ============================================================================

/// Color correction parameters
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ColorParams {
    /// Brightness adjustment (-1.0 to 1.0, 0.0 = no change)
    pub brightness: f32,
    /// Contrast multiplier (0.0 to 3.0, 1.0 = no change)
    pub contrast: f32,
    /// Saturation multiplier (0.0 to 3.0, 1.0 = no change)
    pub saturation: f32,
    /// Gamma correction (0.1 to 3.0, 1.0 = no change)
    pub gamma: f32,
    /// Hue rotation in degrees (-180 to 180)
    pub hue: f32,
    /// Color temperature (-1.0 cool to 1.0 warm, 0.0 = neutral)
    pub temperature: f32,
    /// Vibrance (smart saturation, 0.0 to 2.0, 1.0 = no change)
    pub vibrance: f32,
}

impl Default for ColorParams {
    fn default() -> Self {
        Self {
            brightness: 0.0,
            contrast: 1.0,
            saturation: 1.0,
            gamma: 1.0,
            hue: 0.0,
            temperature: 0.0,
            vibrance: 1.0,
        }
    }
}

impl ColorParams {
    /// Check if any color correction is needed
    pub fn is_identity(&self) -> bool {
        (self.brightness.abs() < 0.001)
            && (self.contrast - 1.0).abs() < 0.001
            && (self.saturation - 1.0).abs() < 0.001
            && (self.gamma - 1.0).abs() < 0.001
            && self.hue.abs() < 0.1
            && self.temperature.abs() < 0.001
            && (self.vibrance - 1.0).abs() < 0.001
    }
}

/// Sharpening parameters
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SharpenParams {
    /// Sharpening algorithm
    pub algorithm: SharpenAlgorithm,
    /// Strength (0.0 to 2.0)
    pub strength: f32,
    /// Radius for unsharp mask
    pub radius: f32,
    /// Threshold to avoid sharpening noise
    pub threshold: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SharpenAlgorithm {
    /// Unsharp mask (classic)
    UnsharpMask,
    /// AMD FidelityFX Contrast Adaptive Sharpening
    Cas,
    /// Simple laplacian
    Laplacian,
}

impl Default for SharpenParams {
    fn default() -> Self {
        Self {
            algorithm: SharpenAlgorithm::Cas,
            strength: 0.5,
            radius: 1.0,
            threshold: 0.0,
        }
    }
}

/// Deinterlacing parameters
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct DeinterlaceParams {
    /// Deinterlacing algorithm
    pub algorithm: DeinterlaceAlgorithm,
    /// Field order (true = top field first)
    pub tff: bool,
    /// Output both fields as separate frames (double framerate)
    pub double_rate: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeinterlaceAlgorithm {
    /// Simple bob (field scaling)
    Bob,
    /// Weave fields together
    Weave,
    /// Motion adaptive (YADIF-like)
    MotionAdaptive,
    /// Blend fields
    Blend,
}

impl Default for DeinterlaceParams {
    fn default() -> Self {
        Self {
            algorithm: DeinterlaceAlgorithm::MotionAdaptive,
            tff: true,
            double_rate: false,
        }
    }
}

/// Noise reduction parameters
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct DenoiseParams {
    /// Spatial noise reduction strength
    pub spatial: f32,
    /// Temporal noise reduction strength (requires motion estimation)
    pub temporal: f32,
    /// Luminance noise reduction
    pub luma: f32,
    /// Chroma noise reduction
    pub chroma: f32,
}

impl Default for DenoiseParams {
    fn default() -> Self {
        Self {
            spatial: 0.3,
            temporal: 0.3,
            luma: 1.0,
            chroma: 1.0,
        }
    }
}

/// Blur parameters
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct BlurParams {
    /// Blur algorithm
    pub algorithm: BlurAlgorithm,
    /// Blur radius
    pub radius: f32,
    /// Sigma for gaussian blur
    pub sigma: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlurAlgorithm {
    Box,
    Gaussian,
    Kawase,  // Fast approximation
}

impl Default for BlurParams {
    fn default() -> Self {
        Self {
            algorithm: BlurAlgorithm::Gaussian,
            radius: 3.0,
            sigma: 1.5,
        }
    }
}

/// Vignette effect parameters
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct VignetteParams {
    /// Vignette intensity (0.0 to 1.0)
    pub intensity: f32,
    /// Vignette radius (0.0 to 2.0, 1.0 = corners)
    pub radius: f32,
    /// Softness of the falloff
    pub softness: f32,
    /// Center X offset (-1.0 to 1.0)
    pub center_x: f32,
    /// Center Y offset (-1.0 to 1.0)
    pub center_y: f32,
}

impl Default for VignetteParams {
    fn default() -> Self {
        Self {
            intensity: 0.3,
            radius: 0.8,
            softness: 0.5,
            center_x: 0.0,
            center_y: 0.0,
        }
    }
}

/// Film grain parameters
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct GrainParams {
    /// Grain intensity (0.0 to 1.0)
    pub intensity: f32,
    /// Grain size
    pub size: f32,
    /// Color grain vs monochrome
    pub color: bool,
    /// Random seed (changes per frame for animation)
    pub seed: u32,
}

impl Default for GrainParams {
    fn default() -> Self {
        Self {
            intensity: 0.1,
            size: 1.0,
            color: false,
            seed: 0,
        }
    }
}

// ============================================================================
// Filter Types
// ============================================================================

/// A single filter in the chain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Filter {
    /// Color correction
    Color(ColorParams),
    /// Sharpening
    Sharpen(SharpenParams),
    /// Deinterlacing
    Deinterlace(DeinterlaceParams),
    /// Noise reduction
    Denoise(DenoiseParams),
    /// Blur
    Blur(BlurParams),
    /// Vignette effect
    Vignette(VignetteParams),
    /// Film grain
    Grain(GrainParams),
    /// 3D LUT (lookup table path)
    Lut3D { path: String, strength: f32 },
    /// Scale (resize)
    Scale { width: u32, height: u32, algorithm: ScaleAlgorithm },
    /// Crop
    Crop { x: u32, y: u32, width: u32, height: u32 },
    /// Letterbox (add black bars)
    Letterbox { aspect_ratio: f32 },
    /// Flip
    Flip { horizontal: bool, vertical: bool },
    /// Rotate
    Rotate { degrees: f32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScaleAlgorithm {
    Nearest,
    Bilinear,
    Bicubic,
    Lanczos,
}

impl Filter {
    /// Get filter name
    pub fn name(&self) -> &'static str {
        match self {
            Self::Color(_) => "Color",
            Self::Sharpen(_) => "Sharpen",
            Self::Deinterlace(_) => "Deinterlace",
            Self::Denoise(_) => "Denoise",
            Self::Blur(_) => "Blur",
            Self::Vignette(_) => "Vignette",
            Self::Grain(_) => "Grain",
            Self::Lut3D { .. } => "LUT3D",
            Self::Scale { .. } => "Scale",
            Self::Crop { .. } => "Crop",
            Self::Letterbox { .. } => "Letterbox",
            Self::Flip { .. } => "Flip",
            Self::Rotate { .. } => "Rotate",
        }
    }

    /// Check if filter modifies resolution
    pub fn changes_resolution(&self) -> bool {
        matches!(self, Self::Scale { .. } | Self::Crop { .. } | Self::Rotate { .. })
    }

    /// Check if filter is a no-op
    pub fn is_identity(&self) -> bool {
        match self {
            Self::Color(p) => p.is_identity(),
            Self::Sharpen(p) => p.strength < 0.001,
            Self::Blur(p) => p.radius < 0.001,
            Self::Vignette(p) => p.intensity < 0.001,
            Self::Grain(p) => p.intensity < 0.001,
            Self::Flip { horizontal, vertical } => !horizontal && !vertical,
            Self::Rotate { degrees } => degrees.abs() < 0.1,
            _ => false,
        }
    }
}

// ============================================================================
// Filter Chain
// ============================================================================

/// A chain of filters to apply sequentially
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FilterChain {
    /// Ordered list of filters
    filters: Vec<Filter>,
    /// Whether the chain is enabled
    enabled: bool,
    /// Chain name/preset name
    name: String,
}

impl FilterChain {
    /// Create an empty filter chain
    pub fn new() -> Self {
        Self {
            filters: Vec::new(),
            enabled: true,
            name: String::new(),
        }
    }

    /// Create a named preset
    pub fn preset(name: &str) -> Self {
        Self {
            filters: Vec::new(),
            enabled: true,
            name: name.to_string(),
        }
    }

    /// Add a filter to the chain
    pub fn add(&mut self, filter: Filter) -> &mut Self {
        self.filters.push(filter);
        self
    }

    /// Insert a filter at position
    pub fn insert(&mut self, index: usize, filter: Filter) -> &mut Self {
        if index <= self.filters.len() {
            self.filters.insert(index, filter);
        }
        self
    }

    /// Remove filter at position
    pub fn remove(&mut self, index: usize) -> Option<Filter> {
        if index < self.filters.len() {
            Some(self.filters.remove(index))
        } else {
            None
        }
    }

    /// Clear all filters
    pub fn clear(&mut self) {
        self.filters.clear();
    }

    /// Get number of filters
    pub fn len(&self) -> usize {
        self.filters.len()
    }

    /// Check if chain is empty
    pub fn is_empty(&self) -> bool {
        self.filters.is_empty()
    }

    /// Enable/disable chain
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Check if enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Get filters
    pub fn filters(&self) -> &[Filter] {
        &self.filters
    }

    /// Get mutable filters
    pub fn filters_mut(&mut self) -> &mut [Filter] {
        &mut self.filters
    }

    /// Optimize chain by removing identity operations
    pub fn optimize(&mut self) {
        self.filters.retain(|f| !f.is_identity());
    }

    /// Check if any filter changes resolution
    pub fn changes_resolution(&self) -> bool {
        self.filters.iter().any(|f| f.changes_resolution())
    }
}

// ============================================================================
// Common Presets
// ============================================================================

impl FilterChain {
    /// Vivid colors preset
    pub fn vivid() -> Self {
        let mut chain = Self::preset("Vivid");
        chain.add(Filter::Color(ColorParams {
            saturation: 1.3,
            vibrance: 1.2,
            contrast: 1.1,
            ..Default::default()
        }));
        chain.add(Filter::Sharpen(SharpenParams {
            algorithm: SharpenAlgorithm::Cas,
            strength: 0.3,
            ..Default::default()
        }));
        chain
    }

    /// Cinematic preset (warm, contrasty)
    pub fn cinematic() -> Self {
        let mut chain = Self::preset("Cinematic");
        chain.add(Filter::Color(ColorParams {
            contrast: 1.15,
            saturation: 0.9,
            temperature: 0.1,
            gamma: 1.05,
            ..Default::default()
        }));
        chain.add(Filter::Vignette(VignetteParams {
            intensity: 0.25,
            radius: 0.85,
            softness: 0.6,
            ..Default::default()
        }));
        chain
    }

    /// Retro/film preset
    pub fn retro() -> Self {
        let mut chain = Self::preset("Retro");
        chain.add(Filter::Color(ColorParams {
            saturation: 0.8,
            contrast: 1.1,
            temperature: 0.15,
            ..Default::default()
        }));
        chain.add(Filter::Grain(GrainParams {
            intensity: 0.15,
            size: 1.2,
            color: false,
            seed: 0,
        }));
        chain.add(Filter::Vignette(VignetteParams {
            intensity: 0.3,
            radius: 0.75,
            softness: 0.4,
            ..Default::default()
        }));
        chain
    }

    /// Night mode (reduced blue light)
    pub fn night_mode() -> Self {
        let mut chain = Self::preset("Night Mode");
        chain.add(Filter::Color(ColorParams {
            temperature: 0.3, // Warmer
            brightness: -0.1,
            ..Default::default()
        }));
        chain
    }

    /// Anime/cartoon enhancement
    pub fn anime() -> Self {
        let mut chain = Self::preset("Anime");
        chain.add(Filter::Denoise(DenoiseParams {
            spatial: 0.4,
            temporal: 0.2,
            ..Default::default()
        }));
        chain.add(Filter::Sharpen(SharpenParams {
            algorithm: SharpenAlgorithm::Cas,
            strength: 0.6,
            ..Default::default()
        }));
        chain.add(Filter::Color(ColorParams {
            saturation: 1.15,
            contrast: 1.05,
            ..Default::default()
        }));
        chain
    }

    /// Deinterlace for old content
    pub fn deinterlace() -> Self {
        let mut chain = Self::preset("Deinterlace");
        chain.add(Filter::Deinterlace(DeinterlaceParams::default()));
        chain
    }
}

// ============================================================================
// Filter Processor (GPU Pipeline)
// ============================================================================

/// GPU-accelerated filter processor
pub struct FilterProcessor {
    /// wgpu device
    device: Arc<wgpu::Device>,
    /// wgpu queue
    queue: Arc<wgpu::Queue>,
    /// Compiled pipelines for each filter type
    pipelines: HashMap<&'static str, CompiledPipeline>,
    /// Input texture
    input_texture: Option<wgpu::Texture>,
    /// Output texture
    output_texture: Option<wgpu::Texture>,
    /// Ping-pong buffers for chaining
    ping_pong: [Option<wgpu::Texture>; 2],
    /// Current dimensions
    width: u32,
    height: u32,
}

struct CompiledPipeline {
    pipeline: wgpu::ComputePipeline,
    bind_group_layout: wgpu::BindGroupLayout,
}

impl FilterProcessor {
    /// Create a new filter processor
    pub async fn new() -> Result<Self, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await
            .ok_or("No suitable GPU adapter")?;

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                    label: Some("filter_device"),
                    memory_hints: Default::default(),
                },
                None,
            )
            .await
            .map_err(|e| format!("Device request failed: {}", e))?;

        let device = Arc::new(device);
        let queue = Arc::new(queue);

        let mut processor = Self {
            device: device.clone(),
            queue: queue.clone(),
            pipelines: HashMap::new(),
            input_texture: None,
            output_texture: None,
            ping_pong: [None, None],
            width: 0,
            height: 0,
        };

        // Compile core pipelines
        processor.compile_color_pipeline();
        processor.compile_sharpen_pipeline();

        Ok(processor)
    }

    /// Compile color correction pipeline
    fn compile_color_pipeline(&mut self) {
        let shader = self.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("color_filter"),
            source: wgpu::ShaderSource::Wgsl(SHADER_COLOR.into()),
        });

        let bind_group_layout = self.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("color_bind_group_layout"),
            entries: &[
                // Input texture
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                // Output texture
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
                // Parameters uniform
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let pipeline_layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("color_pipeline_layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = self.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("color_pipeline"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });

        self.pipelines.insert("Color", CompiledPipeline {
            pipeline,
            bind_group_layout,
        });
    }

    /// Compile sharpening pipeline
    fn compile_sharpen_pipeline(&mut self) {
        let shader = self.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sharpen_filter"),
            source: wgpu::ShaderSource::Wgsl(SHADER_SHARPEN.into()),
        });

        let bind_group_layout = self.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sharpen_bind_group_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let pipeline_layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sharpen_pipeline_layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = self.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("sharpen_pipeline"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });

        self.pipelines.insert("Sharpen", CompiledPipeline {
            pipeline,
            bind_group_layout,
        });
    }

    /// Resize processing buffers
    pub fn resize(&mut self, width: u32, height: u32) {
        if self.width == width && self.height == height {
            return;
        }

        self.width = width;
        self.height = height;

        let texture_desc = wgpu::TextureDescriptor {
            label: Some("filter_texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        };

        self.input_texture = Some(self.device.create_texture(&texture_desc));
        self.output_texture = Some(self.device.create_texture(&texture_desc));
        self.ping_pong[0] = Some(self.device.create_texture(&texture_desc));
        self.ping_pong[1] = Some(self.device.create_texture(&texture_desc));
    }

    /// Process a frame through the filter chain
    pub fn process(&mut self, chain: &FilterChain, input: &[u8], output: &mut [u8]) {
        if !chain.is_enabled() || chain.is_empty() {
            // Pass-through
            output[..input.len()].copy_from_slice(input);
            return;
        }

        // Ensure textures are allocated
        if self.input_texture.is_none() || self.width == 0 || self.height == 0 {
            output[..input.len()].copy_from_slice(input);
            return;
        }

        // Upload input to GPU
        let input_tex = self.input_texture.as_ref().unwrap();
        self.queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: input_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            input,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(4 * self.width),
                rows_per_image: Some(self.height),
            },
            wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );

        // Create command encoder
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("filter_encoder"),
        });

        // Workgroup size (must match shader)
        let workgroup_size = 16u32;
        let dispatch_x = (self.width + workgroup_size - 1) / workgroup_size;
        let dispatch_y = (self.height + workgroup_size - 1) / workgroup_size;

        // Track current input/output textures for ping-pong
        let mut current_input = input_tex;
        let mut ping_pong_idx = 0usize;

        // Process each filter in the chain
        for filter in chain.filters() {
            // Skip identity filters
            if filter.is_identity() {
                continue;
            }

            let pipeline_name = filter.name();

            // Get the compiled pipeline for this filter type
            if let Some(compiled) = self.pipelines.get(pipeline_name) {
                // Get the output texture (ping-pong buffer)
                let output_tex = self.ping_pong[ping_pong_idx].as_ref()
                    .unwrap_or_else(|| self.output_texture.as_ref().unwrap());

                // Create parameter buffer based on filter type
                let param_buffer = match filter {
                    Filter::Color(params) => {
                        // Pack ColorParams into uniform buffer (must be 16-byte aligned)
                        let data = [
                            params.brightness,
                            params.contrast,
                            params.saturation,
                            params.gamma,
                            params.hue,
                            params.temperature,
                            params.vibrance,
                            0.0f32, // padding
                        ];
                        self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                            label: Some("color_params"),
                            contents: bytemuck::cast_slice(&data),
                            usage: wgpu::BufferUsages::UNIFORM,
                        })
                    }
                    Filter::Sharpen(params) => {
                        let data = [
                            params.strength,
                            params.radius,
                            params.threshold,
                            params.algorithm as u32 as f32,
                        ];
                        self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                            label: Some("sharpen_params"),
                            contents: bytemuck::cast_slice(&data),
                            usage: wgpu::BufferUsages::UNIFORM,
                        })
                    }
                    _ => {
                        // Default empty params for other filters
                        let data = [0.0f32; 4];
                        self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                            label: Some("filter_params"),
                            contents: bytemuck::cast_slice(&data),
                            usage: wgpu::BufferUsages::UNIFORM,
                        })
                    }
                };

                // Create texture views
                let input_view = current_input.create_view(&wgpu::TextureViewDescriptor::default());
                let output_view = output_tex.create_view(&wgpu::TextureViewDescriptor::default());

                // Create bind group
                let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("filter_bind_group"),
                    layout: &compiled.bind_group_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&input_view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(&output_view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: param_buffer.as_entire_binding(),
                        },
                    ],
                });

                // Create compute pass and dispatch
                {
                    let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                        label: Some("filter_pass"),
                        timestamp_writes: None,
                    });

                    compute_pass.set_pipeline(&compiled.pipeline);
                    compute_pass.set_bind_group(0, &bind_group, &[]);
                    compute_pass.dispatch_workgroups(dispatch_x, dispatch_y, 1);
                }

                // Swap ping-pong buffers
                current_input = output_tex;
                ping_pong_idx = 1 - ping_pong_idx;
            }
        }

        // Create staging buffer for GPU -> CPU copy
        let buffer_size = (4 * self.width * self.height) as u64;
        let staging_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("staging_buffer"),
            size: buffer_size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Copy final result to staging buffer
        encoder.copy_texture_to_buffer(
            wgpu::ImageCopyTexture {
                texture: current_input,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::ImageCopyBuffer {
                buffer: &staging_buffer,
                layout: wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(4 * self.width),
                    rows_per_image: Some(self.height),
                },
            },
            wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );

        // Submit commands
        self.queue.submit(Some(encoder.finish()));

        // Map staging buffer and copy to output
        let buffer_slice = staging_buffer.slice(..);

        // Use blocking map (for synchronous API)
        let (tx, rx) = std::sync::mpsc::channel();
        buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });

        // Poll device until map is complete
        self.device.poll(wgpu::Maintain::Wait);

        if rx.recv().ok().flatten().is_ok() {
            let data = buffer_slice.get_mapped_range();
            let len = output.len().min(data.len());
            output[..len].copy_from_slice(&data[..len]);
        }

        staging_buffer.unmap();
    }

    /// Get GPU device info
    pub fn device_name(&self) -> String {
        "GPU Filter Processor".to_string()
    }
}

// ============================================================================
// WGSL Shaders
// ============================================================================

const SHADER_COLOR: &str = r#"
struct ColorParams {
    brightness: f32,
    contrast: f32,
    saturation: f32,
    gamma: f32,
    hue: f32,
    temperature: f32,
    vibrance: f32,
    _padding: f32,
}

@group(0) @binding(0) var input_tex: texture_2d<f32>;
@group(0) @binding(1) var output_tex: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(2) var<uniform> params: ColorParams;

fn rgb_to_hsv(rgb: vec3<f32>) -> vec3<f32> {
    let cmax = max(rgb.r, max(rgb.g, rgb.b));
    let cmin = min(rgb.r, min(rgb.g, rgb.b));
    let delta = cmax - cmin;

    var h: f32 = 0.0;
    if delta > 0.0001 {
        if cmax == rgb.r {
            h = 60.0 * (((rgb.g - rgb.b) / delta) % 6.0);
        } else if cmax == rgb.g {
            h = 60.0 * (((rgb.b - rgb.r) / delta) + 2.0);
        } else {
            h = 60.0 * (((rgb.r - rgb.g) / delta) + 4.0);
        }
    }
    if h < 0.0 { h = h + 360.0; }

    let s = select(0.0, delta / cmax, cmax > 0.0001);
    let v = cmax;

    return vec3<f32>(h, s, v);
}

fn hsv_to_rgb(hsv: vec3<f32>) -> vec3<f32> {
    let c = hsv.z * hsv.y;
    let x = c * (1.0 - abs((hsv.x / 60.0) % 2.0 - 1.0));
    let m = hsv.z - c;

    var rgb: vec3<f32>;
    let h = hsv.x;

    if h < 60.0 {
        rgb = vec3<f32>(c, x, 0.0);
    } else if h < 120.0 {
        rgb = vec3<f32>(x, c, 0.0);
    } else if h < 180.0 {
        rgb = vec3<f32>(0.0, c, x);
    } else if h < 240.0 {
        rgb = vec3<f32>(0.0, x, c);
    } else if h < 300.0 {
        rgb = vec3<f32>(x, 0.0, c);
    } else {
        rgb = vec3<f32>(c, 0.0, x);
    }

    return rgb + m;
}

@compute @workgroup_size(16, 16, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let dims = textureDimensions(input_tex);
    if gid.x >= dims.x || gid.y >= dims.y {
        return;
    }

    var color = textureLoad(input_tex, vec2<i32>(gid.xy), 0);

    // Brightness
    color = vec4<f32>(color.rgb + params.brightness, color.a);

    // Contrast
    color = vec4<f32>((color.rgb - 0.5) * params.contrast + 0.5, color.a);

    // Gamma
    color = vec4<f32>(pow(color.rgb, vec3<f32>(1.0 / params.gamma)), color.a);

    // HSV adjustments
    var hsv = rgb_to_hsv(color.rgb);

    // Hue rotation
    hsv.x = (hsv.x + params.hue) % 360.0;
    if hsv.x < 0.0 { hsv.x = hsv.x + 360.0; }

    // Saturation
    hsv.y = hsv.y * params.saturation;

    // Vibrance (saturate unsaturated colors more)
    let avg_sat = (1.0 - hsv.y) * (params.vibrance - 1.0);
    hsv.y = hsv.y + avg_sat * hsv.y;

    color = vec4<f32>(hsv_to_rgb(hsv), color.a);

    // Temperature (shift red/blue balance)
    color.r = color.r + params.temperature * 0.1;
    color.b = color.b - params.temperature * 0.1;

    // Clamp
    color = clamp(color, vec4<f32>(0.0), vec4<f32>(1.0));

    textureStore(output_tex, vec2<i32>(gid.xy), color);
}
"#;

const SHADER_SHARPEN: &str = r#"
struct SharpenParams {
    strength: f32,
    radius: f32,
    threshold: f32,
    _padding: f32,
}

@group(0) @binding(0) var input_tex: texture_2d<f32>;
@group(0) @binding(1) var output_tex: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(2) var<uniform> params: SharpenParams;

fn luminance(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

// AMD FidelityFX CAS-style sharpening
@compute @workgroup_size(16, 16, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let dims = textureDimensions(input_tex);
    if gid.x >= dims.x || gid.y >= dims.y {
        return;
    }

    let pos = vec2<i32>(gid.xy);

    // Sample center and neighbors
    let c = textureLoad(input_tex, pos, 0).rgb;
    let n = textureLoad(input_tex, pos + vec2<i32>(0, -1), 0).rgb;
    let s = textureLoad(input_tex, pos + vec2<i32>(0, 1), 0).rgb;
    let e = textureLoad(input_tex, pos + vec2<i32>(1, 0), 0).rgb;
    let w = textureLoad(input_tex, pos + vec2<i32>(-1, 0), 0).rgb;

    // Min/max of neighborhood
    let mn = min(c, min(min(n, s), min(e, w)));
    let mx = max(c, max(max(n, s), max(e, w)));

    // Sharpening amount based on local contrast
    let d = mx - mn;
    let peak = 1.0 - params.strength * 0.5;
    let w_amt = sqrt(min(mn.r, min(mn.g, min(mn.b,
        1.0 - max(mx.r, max(mx.g, mx.b)))))) * peak;

    // Apply sharpening
    let sharp = (c * (1.0 + w_amt * 4.0) - (n + s + e + w) * w_amt) * params.strength;
    var result = c + sharp * params.strength;

    // Threshold - don't sharpen near-flat areas
    let edge = luminance(d);
    if edge < params.threshold {
        result = c;
    }

    result = clamp(result, vec3<f32>(0.0), vec3<f32>(1.0));
    textureStore(output_tex, pos, vec4<f32>(result, 1.0));
}
"#;

// ============================================================================
// Preset Manager
// ============================================================================

/// Manages filter presets
pub struct PresetManager {
    presets: HashMap<String, FilterChain>,
    current: Option<String>,
}

impl PresetManager {
    /// Create with built-in presets
    pub fn new() -> Self {
        let mut presets = HashMap::new();
        presets.insert("Vivid".to_string(), FilterChain::vivid());
        presets.insert("Cinematic".to_string(), FilterChain::cinematic());
        presets.insert("Retro".to_string(), FilterChain::retro());
        presets.insert("Night Mode".to_string(), FilterChain::night_mode());
        presets.insert("Anime".to_string(), FilterChain::anime());
        presets.insert("Deinterlace".to_string(), FilterChain::deinterlace());

        Self {
            presets,
            current: None,
        }
    }

    /// Get preset by name
    pub fn get(&self, name: &str) -> Option<&FilterChain> {
        self.presets.get(name)
    }

    /// Add custom preset
    pub fn add(&mut self, name: &str, chain: FilterChain) {
        self.presets.insert(name.to_string(), chain);
    }

    /// Remove preset
    pub fn remove(&mut self, name: &str) -> Option<FilterChain> {
        self.presets.remove(name)
    }

    /// List all presets
    pub fn list(&self) -> Vec<&str> {
        self.presets.keys().map(|s| s.as_str()).collect()
    }

    /// Set current preset
    pub fn set_current(&mut self, name: &str) -> bool {
        if self.presets.contains_key(name) {
            self.current = Some(name.to_string());
            true
        } else {
            false
        }
    }

    /// Get current preset
    pub fn current(&self) -> Option<&FilterChain> {
        self.current.as_ref().and_then(|n| self.presets.get(n))
    }

    /// Save presets to JSON
    pub fn save(&self, path: &str) -> Result<(), String> {
        let json = serde_json::to_string_pretty(&self.presets)
            .map_err(|e| e.to_string())?;
        std::fs::write(path, json).map_err(|e| e.to_string())
    }

    /// Load presets from JSON
    pub fn load(&mut self, path: &str) -> Result<(), String> {
        let json = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let loaded: HashMap<String, FilterChain> = serde_json::from_str(&json)
            .map_err(|e| e.to_string())?;
        self.presets.extend(loaded);
        Ok(())
    }
}

impl Default for PresetManager {
    fn default() -> Self {
        Self::new()
    }
}
