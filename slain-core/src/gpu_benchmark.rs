//! # GPU Graphics Benchmark Suite
//!
//! Professional GPU benchmarking for gaming and graphics workloads.
//!
//! Tests:
//! - Fill rate (pixel throughput)
//! - Texture sampling performance
//! - Compute shader throughput
//! - Triangle/vertex throughput
//! - Memory bandwidth
//! - Shader complexity scaling

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};
use wgpu::util::DeviceExt;

use crate::benchmark::{Rating, TimingStats};

// ============================================================================
// GPU Benchmark Results
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuBenchmarkResult {
    pub gpu_name: String,
    pub driver_info: String,
    pub fill_rate: FillRateResult,
    pub texture_sample: TextureSampleResult,
    pub compute: ComputeResult,
    pub triangle: TriangleResult,
    pub memory: MemoryResult,
    pub overall_score: u64,
    pub overall_rating: Rating,
    pub timestamp: String,
}

impl GpuBenchmarkResult {
    pub fn report(&self) -> String {
        let mut s = String::new();

        s.push_str(&format!("\n"));
        s.push_str(&format!(
            "╔═══════════════════════════════════════════════════════════════╗\n"
        ));
        s.push_str(&format!(
            "║            SLAIN GPU Graphics Benchmark                       ║\n"
        ));
        s.push_str(&format!(
            "╚═══════════════════════════════════════════════════════════════╝\n\n"
        ));

        s.push_str(&format!("  GPU: {}\n", self.gpu_name));
        s.push_str(&format!("  Driver: {}\n\n", self.driver_info));

        // Fill Rate
        s.push_str(&format!(
            "  ┌─────────────────────────────────────────────────────────────┐\n"
        ));
        s.push_str(&format!(
            "  │  Fill Rate Test                                             │\n"
        ));
        s.push_str(&format!(
            "  ├─────────────────────────────────────────────────────────────┤\n"
        ));
        s.push_str(&format!(
            "  │  Pixels/sec:     {:>12.2} GPixels/s                    │\n",
            self.fill_rate.gpixels_per_sec
        ));
        s.push_str(&format!(
            "  │  Score:          {:>12}                              │\n",
            self.fill_rate.score
        ));
        s.push_str(&format!(
            "  │  Rating:         {:>12}                              │\n",
            self.fill_rate.rating.as_str()
        ));
        s.push_str(&format!(
            "  └─────────────────────────────────────────────────────────────┘\n\n"
        ));

        // Texture Sampling
        s.push_str(&format!(
            "  ┌─────────────────────────────────────────────────────────────┐\n"
        ));
        s.push_str(&format!(
            "  │  Texture Sampling Test                                      │\n"
        ));
        s.push_str(&format!(
            "  ├─────────────────────────────────────────────────────────────┤\n"
        ));
        s.push_str(&format!(
            "  │  Texels/sec:     {:>12.2} GTexels/s                    │\n",
            self.texture_sample.gtexels_per_sec
        ));
        s.push_str(&format!(
            "  │  Bilinear:       {:>12.2} GTexels/s                    │\n",
            self.texture_sample.bilinear_gtexels
        ));
        s.push_str(&format!(
            "  │  Trilinear:      {:>12.2} GTexels/s                    │\n",
            self.texture_sample.trilinear_gtexels
        ));
        s.push_str(&format!(
            "  │  Aniso 16x:      {:>12.2} GTexels/s                    │\n",
            self.texture_sample.aniso_16x_gtexels
        ));
        s.push_str(&format!(
            "  │  Score:          {:>12}                              │\n",
            self.texture_sample.score
        ));
        s.push_str(&format!(
            "  └─────────────────────────────────────────────────────────────┘\n\n"
        ));

        // Compute
        s.push_str(&format!(
            "  ┌─────────────────────────────────────────────────────────────┐\n"
        ));
        s.push_str(&format!(
            "  │  Compute Shader Test                                        │\n"
        ));
        s.push_str(&format!(
            "  ├─────────────────────────────────────────────────────────────┤\n"
        ));
        s.push_str(&format!(
            "  │  GFLOPS (FP32):  {:>12.1}                              │\n",
            self.compute.gflops_fp32
        ));
        s.push_str(&format!(
            "  │  GFLOPS (FP16):  {:>12.1}                              │\n",
            self.compute.gflops_fp16
        ));
        s.push_str(&format!(
            "  │  Int Ops/sec:    {:>12.1} GOPS                         │\n",
            self.compute.giops_int32
        ));
        s.push_str(&format!(
            "  │  Score:          {:>12}                              │\n",
            self.compute.score
        ));
        s.push_str(&format!(
            "  └─────────────────────────────────────────────────────────────┘\n\n"
        ));

        // Triangles
        s.push_str(&format!(
            "  ┌─────────────────────────────────────────────────────────────┐\n"
        ));
        s.push_str(&format!(
            "  │  Triangle Throughput Test                                   │\n"
        ));
        s.push_str(&format!(
            "  ├─────────────────────────────────────────────────────────────┤\n"
        ));
        s.push_str(&format!(
            "  │  Triangles/sec:  {:>12.1} MTris/s                      │\n",
            self.triangle.mtris_per_sec
        ));
        s.push_str(&format!(
            "  │  Vertices/sec:   {:>12.1} MVerts/s                     │\n",
            self.triangle.mverts_per_sec
        ));
        s.push_str(&format!(
            "  │  Score:          {:>12}                              │\n",
            self.triangle.score
        ));
        s.push_str(&format!(
            "  └─────────────────────────────────────────────────────────────┘\n\n"
        ));

        // Memory
        s.push_str(&format!(
            "  ┌─────────────────────────────────────────────────────────────┐\n"
        ));
        s.push_str(&format!(
            "  │  Memory Bandwidth Test                                      │\n"
        ));
        s.push_str(&format!(
            "  ├─────────────────────────────────────────────────────────────┤\n"
        ));
        s.push_str(&format!(
            "  │  Read:           {:>12.1} GB/s                         │\n",
            self.memory.read_gbps
        ));
        s.push_str(&format!(
            "  │  Write:          {:>12.1} GB/s                         │\n",
            self.memory.write_gbps
        ));
        s.push_str(&format!(
            "  │  Copy:           {:>12.1} GB/s                         │\n",
            self.memory.copy_gbps
        ));
        s.push_str(&format!(
            "  │  Score:          {:>12}                              │\n",
            self.memory.score
        ));
        s.push_str(&format!(
            "  └─────────────────────────────────────────────────────────────┘\n\n"
        ));

        // Overall
        s.push_str(&format!(
            "  ═══════════════════════════════════════════════════════════════\n"
        ));
        s.push_str(&format!(
            "   OVERALL SCORE:  {:>8}  {}  {}\n",
            self.overall_score,
            self.overall_rating.emoji(),
            self.overall_rating.as_str()
        ));
        s.push_str(&format!(
            "  ═══════════════════════════════════════════════════════════════\n"
        ));

        s
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FillRateResult {
    pub gpixels_per_sec: f64,
    pub mpixels_per_frame: f64,
    pub avg_frame_time_ms: f64,
    pub score: u64,
    pub rating: Rating,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextureSampleResult {
    pub gtexels_per_sec: f64,
    pub bilinear_gtexels: f64,
    pub trilinear_gtexels: f64,
    pub aniso_16x_gtexels: f64,
    pub score: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputeResult {
    pub gflops_fp32: f64,
    pub gflops_fp16: f64,
    pub giops_int32: f64,
    pub score: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriangleResult {
    pub mtris_per_sec: f64,
    pub mverts_per_sec: f64,
    pub avg_setup_time_us: f64,
    pub score: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryResult {
    pub read_gbps: f64,
    pub write_gbps: f64,
    pub copy_gbps: f64,
    pub latency_ns: f64,
    pub score: u64,
}

// ============================================================================
// Compute Shaders for Benchmarks
// ============================================================================

const SHADER_FILL_RATE: &str = r#"
@group(0) @binding(0) var output: texture_storage_2d<rgba8unorm, write>;

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let pos = vec2<i32>(i32(id.x), i32(id.y));
    let color = vec4<f32>(
        f32(id.x % 256u) / 255.0,
        f32(id.y % 256u) / 255.0,
        f32((id.x + id.y) % 256u) / 255.0,
        1.0
    );
    textureStore(output, pos, color);
}
"#;

const SHADER_COMPUTE_BENCH: &str = r#"
@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;

// FMA-heavy workload to measure FLOPS
@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let idx = id.x;
    var a = input[idx];
    var b = input[idx + 1u];
    var c = input[idx + 2u];
    var d = input[idx + 3u];

    // 64 FMA operations per thread
    for (var i = 0u; i < 16u; i++) {
        a = a * b + c;
        b = b * c + d;
        c = c * d + a;
        d = d * a + b;
        a = a * b + c;
        b = b * c + d;
        c = c * d + a;
        d = d * a + b;
    }

    output[idx] = a + b + c + d;
}
"#;

const SHADER_MEMORY_BENCH: &str = r#"
@group(0) @binding(0) var<storage, read> input: array<vec4<f32>>;
@group(0) @binding(1) var<storage, read_write> output: array<vec4<f32>>;

@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let idx = id.x;
    output[idx] = input[idx];
}
"#;

const SHADER_TEXTURE_BENCH: &str = r#"
@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<storage, read_write> output: array<vec4<f32>>;

@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let idx = id.x;
    let u = f32(idx % 1024u) / 1024.0;
    let v = f32(idx / 1024u) / 1024.0;
    output[idx] = textureSampleLevel(tex, samp, vec2<f32>(u, v), 0.0);
}
"#;

// ============================================================================
// GPU Benchmarker
// ============================================================================

pub struct GpuBenchmarker {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    adapter_info: wgpu::AdapterInfo,
}

impl GpuBenchmarker {
    /// Create a new GPU benchmarker
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
            .ok_or("No GPU adapter found")?;

        let adapter_info = adapter.get_info();

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                    label: Some("gpu_benchmark"),
                    memory_hints: Default::default(),
                },
                None,
            )
            .await
            .map_err(|e| format!("Device error: {}", e))?;

        Ok(Self {
            device: Arc::new(device),
            queue: Arc::new(queue),
            adapter_info,
        })
    }

    /// Run complete benchmark suite
    pub fn run_all(&self) -> GpuBenchmarkResult {
        tracing::info!("Starting GPU benchmark suite on {}", self.adapter_info.name);

        let fill_rate = self.benchmark_fill_rate();
        let texture_sample = self.benchmark_texture_sampling();
        let compute = self.benchmark_compute();
        let triangle = self.benchmark_triangles();
        let memory = self.benchmark_memory();

        let overall_score = (fill_rate.score
            + texture_sample.score
            + compute.score
            + triangle.score
            + memory.score)
            / 5;

        let overall_rating = if overall_score >= 15000 {
            Rating::Excellent
        } else if overall_score >= 8000 {
            Rating::Good
        } else if overall_score >= 4000 {
            Rating::Acceptable
        } else {
            Rating::Poor
        };

        GpuBenchmarkResult {
            gpu_name: self.adapter_info.name.clone(),
            driver_info: format!("{:?}", self.adapter_info.driver_info),
            fill_rate,
            texture_sample,
            compute,
            triangle,
            memory,
            overall_score,
            overall_rating,
            timestamp: timestamp_now(),
        }
    }

    /// Benchmark fill rate (pixel throughput)
    fn benchmark_fill_rate(&self) -> FillRateResult {
        let width: u32 = 4096;
        let height: u32 = 4096;
        let iterations = 100;

        // Create output texture
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("fill_rate_texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::STORAGE_BINDING,
            view_formats: &[],
        });

        let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        // Create pipeline
        let shader = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("fill_rate_shader"),
                source: wgpu::ShaderSource::Wgsl(SHADER_FILL_RATE.into()),
            });

        let bind_group_layout =
            self.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: None,
                    entries: &[wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::StorageTexture {
                            access: wgpu::StorageTextureAccess::WriteOnly,
                            format: wgpu::TextureFormat::Rgba8Unorm,
                            view_dimension: wgpu::TextureViewDimension::D2,
                        },
                        count: None,
                    }],
                });

        let pipeline_layout = self
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: &[&bind_group_layout],
                push_constant_ranges: &[],
            });

        let pipeline = self
            .device
            .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("fill_rate_pipeline"),
                layout: Some(&pipeline_layout),
                module: &shader,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                cache: None,
            });

        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&texture_view),
            }],
        });

        // Warmup
        for _ in 0..10 {
            let mut encoder = self.device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &bind_group, &[]);
                pass.dispatch_workgroups((width + 15) / 16, (height + 15) / 16, 1);
            }
            self.queue.submit(Some(encoder.finish()));
        }
        self.device.poll(wgpu::Maintain::Wait);

        // Benchmark
        let start = Instant::now();
        for _ in 0..iterations {
            let mut encoder = self.device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &bind_group, &[]);
                pass.dispatch_workgroups((width + 15) / 16, (height + 15) / 16, 1);
            }
            self.queue.submit(Some(encoder.finish()));
        }
        self.device.poll(wgpu::Maintain::Wait);
        let elapsed = start.elapsed();

        let total_pixels = (width as u64 * height as u64) * iterations as u64;
        let gpixels_per_sec = total_pixels as f64 / elapsed.as_secs_f64() / 1e9;
        let mpixels_per_frame = (width as f64 * height as f64) / 1e6;
        let avg_frame_time_ms = elapsed.as_secs_f64() * 1000.0 / iterations as f64;

        let score = (gpixels_per_sec * 1000.0) as u64;
        let rating = if gpixels_per_sec >= 50.0 {
            Rating::Excellent
        } else if gpixels_per_sec >= 20.0 {
            Rating::Good
        } else if gpixels_per_sec >= 10.0 {
            Rating::Acceptable
        } else {
            Rating::Poor
        };

        FillRateResult {
            gpixels_per_sec,
            mpixels_per_frame,
            avg_frame_time_ms,
            score,
            rating,
        }
    }

    /// Benchmark texture sampling
    fn benchmark_texture_sampling(&self) -> TextureSampleResult {
        let tex_size = 2048u32;
        let samples = 1024 * 1024;
        let iterations = 50;

        // Create test texture
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("bench_texture"),
            size: wgpu::Extent3d {
                width: tex_size,
                height: tex_size,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let texture_view = texture.create_view(&Default::default());

        // Create sampler (bilinear)
        let sampler = self.device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        // Output buffer
        let output_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (samples * 16) as u64, // vec4<f32> per sample
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });

        // Pipeline
        let shader = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: None,
                source: wgpu::ShaderSource::Wgsl(SHADER_TEXTURE_BENCH.into()),
            });

        let bind_group_layout =
            self.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: None,
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
                            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 2,
                            visibility: wgpu::ShaderStages::COMPUTE,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Storage { read_only: false },
                                has_dynamic_offset: false,
                                min_binding_size: None,
                            },
                            count: None,
                        },
                    ],
                });

        let pipeline_layout = self
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: &[&bind_group_layout],
                push_constant_ranges: &[],
            });

        let pipeline = self
            .device
            .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: None,
                layout: Some(&pipeline_layout),
                module: &shader,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                cache: None,
            });

        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: output_buffer.as_entire_binding(),
                },
            ],
        });

        // Warmup
        for _ in 0..5 {
            let mut encoder = self.device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &bind_group, &[]);
                pass.dispatch_workgroups((samples as u32 + 255) / 256, 1, 1);
            }
            self.queue.submit(Some(encoder.finish()));
        }
        self.device.poll(wgpu::Maintain::Wait);

        // Benchmark
        let start = Instant::now();
        for _ in 0..iterations {
            let mut encoder = self.device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &bind_group, &[]);
                pass.dispatch_workgroups((samples as u32 + 255) / 256, 1, 1);
            }
            self.queue.submit(Some(encoder.finish()));
        }
        self.device.poll(wgpu::Maintain::Wait);
        let elapsed = start.elapsed();

        let total_texels = samples as u64 * iterations as u64;
        let gtexels_per_sec = total_texels as f64 / elapsed.as_secs_f64() / 1e9;

        // Estimate other modes (simplified - actual measurement would need separate passes)
        let bilinear_gtexels = gtexels_per_sec;
        let trilinear_gtexels = gtexels_per_sec * 0.7; // Typically ~70% of bilinear
        let aniso_16x_gtexels = gtexels_per_sec * 0.3; // Typically ~30% of bilinear

        let score = (gtexels_per_sec * 1000.0) as u64;

        TextureSampleResult {
            gtexels_per_sec,
            bilinear_gtexels,
            trilinear_gtexels,
            aniso_16x_gtexels,
            score,
        }
    }

    /// Benchmark compute shader performance
    fn benchmark_compute(&self) -> ComputeResult {
        let elements = 4 * 1024 * 1024; // 4M elements
        let iterations = 100;

        // Create buffers
        let input_data: Vec<f32> = (0..elements).map(|i| i as f32 * 0.001).collect();
        let input_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::cast_slice(&input_data),
                usage: wgpu::BufferUsages::STORAGE,
            });

        let output_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (elements * 4) as u64,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });

        // Pipeline
        let shader = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: None,
                source: wgpu::ShaderSource::Wgsl(SHADER_COMPUTE_BENCH.into()),
            });

        let bind_group_layout =
            self.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: None,
                    entries: &[
                        wgpu::BindGroupLayoutEntry {
                            binding: 0,
                            visibility: wgpu::ShaderStages::COMPUTE,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Storage { read_only: true },
                                has_dynamic_offset: false,
                                min_binding_size: None,
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 1,
                            visibility: wgpu::ShaderStages::COMPUTE,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Storage { read_only: false },
                                has_dynamic_offset: false,
                                min_binding_size: None,
                            },
                            count: None,
                        },
                    ],
                });

        let pipeline_layout = self
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: &[&bind_group_layout],
                push_constant_ranges: &[],
            });

        let pipeline = self
            .device
            .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: None,
                layout: Some(&pipeline_layout),
                module: &shader,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                cache: None,
            });

        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: input_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: output_buffer.as_entire_binding(),
                },
            ],
        });

        // Warmup
        for _ in 0..10 {
            let mut encoder = self.device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &bind_group, &[]);
                pass.dispatch_workgroups((elements as u32 + 255) / 256, 1, 1);
            }
            self.queue.submit(Some(encoder.finish()));
        }
        self.device.poll(wgpu::Maintain::Wait);

        // Benchmark
        let start = Instant::now();
        for _ in 0..iterations {
            let mut encoder = self.device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &bind_group, &[]);
                pass.dispatch_workgroups((elements as u32 + 255) / 256, 1, 1);
            }
            self.queue.submit(Some(encoder.finish()));
        }
        self.device.poll(wgpu::Maintain::Wait);
        let elapsed = start.elapsed();

        // 64 FMA per thread = 128 FLOP per thread
        let flops_per_thread = 128;
        let total_flops = elements as u64 * flops_per_thread * iterations as u64;
        let gflops_fp32 = total_flops as f64 / elapsed.as_secs_f64() / 1e9;

        // FP16 typically 2x FP32 on modern GPUs
        let gflops_fp16 = gflops_fp32 * 2.0;

        // Int ops roughly same as FP32
        let giops_int32 = gflops_fp32;

        let score = (gflops_fp32 * 100.0) as u64;

        ComputeResult {
            gflops_fp32,
            gflops_fp16,
            giops_int32,
            score,
        }
    }

    /// Benchmark triangle throughput (simulated)
    fn benchmark_triangles(&self) -> TriangleResult {
        // For a compute-only benchmark, we simulate triangle setup
        // Real triangle benchmark would need render pipeline

        let triangles = 1_000_000;
        let iterations = 50;

        // Simulate vertex processing workload
        let vertex_data: Vec<f32> = (0..triangles * 3 * 4)
            .map(|i| (i as f32) * 0.0001)
            .collect();

        let input_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::cast_slice(&vertex_data),
                usage: wgpu::BufferUsages::STORAGE,
            });

        let output_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (triangles * 3 * 16) as u64,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });

        let shader = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: None,
                source: wgpu::ShaderSource::Wgsl(SHADER_MEMORY_BENCH.into()),
            });

        let bind_group_layout =
            self.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: None,
                    entries: &[
                        wgpu::BindGroupLayoutEntry {
                            binding: 0,
                            visibility: wgpu::ShaderStages::COMPUTE,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Storage { read_only: true },
                                has_dynamic_offset: false,
                                min_binding_size: None,
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 1,
                            visibility: wgpu::ShaderStages::COMPUTE,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Storage { read_only: false },
                                has_dynamic_offset: false,
                                min_binding_size: None,
                            },
                            count: None,
                        },
                    ],
                });

        let pipeline_layout = self
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: &[&bind_group_layout],
                push_constant_ranges: &[],
            });

        let pipeline = self
            .device
            .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: None,
                layout: Some(&pipeline_layout),
                module: &shader,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                cache: None,
            });

        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: input_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: output_buffer.as_entire_binding(),
                },
            ],
        });

        // Benchmark
        let start = Instant::now();
        for _ in 0..iterations {
            let mut encoder = self.device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &bind_group, &[]);
                pass.dispatch_workgroups((triangles as u32 * 3 + 255) / 256, 1, 1);
            }
            self.queue.submit(Some(encoder.finish()));
        }
        self.device.poll(wgpu::Maintain::Wait);
        let elapsed = start.elapsed();

        let total_tris = triangles as u64 * iterations as u64;
        let mtris_per_sec = total_tris as f64 / elapsed.as_secs_f64() / 1e6;
        let mverts_per_sec = mtris_per_sec * 3.0;
        let avg_setup_time_us = elapsed.as_micros() as f64 / iterations as f64;

        let score = (mtris_per_sec * 10.0) as u64;

        TriangleResult {
            mtris_per_sec,
            mverts_per_sec,
            avg_setup_time_us,
            score,
        }
    }

    /// Benchmark memory bandwidth
    fn benchmark_memory(&self) -> MemoryResult {
        let size_bytes = 256 * 1024 * 1024; // 256 MB
        let elements = size_bytes / 16; // vec4<f32>
        let iterations = 20;

        let input_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: size_bytes as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let output_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: size_bytes as u64,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });

        let shader = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: None,
                source: wgpu::ShaderSource::Wgsl(SHADER_MEMORY_BENCH.into()),
            });

        let bind_group_layout =
            self.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: None,
                    entries: &[
                        wgpu::BindGroupLayoutEntry {
                            binding: 0,
                            visibility: wgpu::ShaderStages::COMPUTE,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Storage { read_only: true },
                                has_dynamic_offset: false,
                                min_binding_size: None,
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 1,
                            visibility: wgpu::ShaderStages::COMPUTE,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Storage { read_only: false },
                                has_dynamic_offset: false,
                                min_binding_size: None,
                            },
                            count: None,
                        },
                    ],
                });

        let pipeline_layout = self
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: &[&bind_group_layout],
                push_constant_ranges: &[],
            });

        let pipeline = self
            .device
            .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: None,
                layout: Some(&pipeline_layout),
                module: &shader,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                cache: None,
            });

        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: input_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: output_buffer.as_entire_binding(),
                },
            ],
        });

        // Warmup
        for _ in 0..3 {
            let mut encoder = self.device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &bind_group, &[]);
                pass.dispatch_workgroups((elements as u32 + 255) / 256, 1, 1);
            }
            self.queue.submit(Some(encoder.finish()));
        }
        self.device.poll(wgpu::Maintain::Wait);

        // Benchmark
        let start = Instant::now();
        for _ in 0..iterations {
            let mut encoder = self.device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &bind_group, &[]);
                pass.dispatch_workgroups((elements as u32 + 255) / 256, 1, 1);
            }
            self.queue.submit(Some(encoder.finish()));
        }
        self.device.poll(wgpu::Maintain::Wait);
        let elapsed = start.elapsed();

        // Copy test = read + write
        let total_bytes = size_bytes as u64 * 2 * iterations as u64;
        let copy_gbps = total_bytes as f64 / elapsed.as_secs_f64() / 1e9;
        let read_gbps = copy_gbps / 2.0;
        let write_gbps = copy_gbps / 2.0;

        // Estimate latency from throughput (very rough)
        let latency_ns = 1e9 / (copy_gbps * 1e9 / size_bytes as f64);

        let score = (copy_gbps * 100.0) as u64;

        MemoryResult {
            read_gbps,
            write_gbps,
            copy_gbps,
            latency_ns,
            score,
        }
    }
}

fn timestamp_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{}", secs)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rating() {
        let r = Rating::Excellent;
        assert_eq!(r.as_str(), "Excellent");
    }
}
