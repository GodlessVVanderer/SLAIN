//! # SLAIN Core
//!
//! Pure Rust GPU-accelerated video player and hardware toolkit.

#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]

// ============================================================================
// Core GPU / Hardware
// ============================================================================
pub mod driver_analysis;
pub mod gpu;
pub mod gpu_orchestrator;
pub mod hardware_bridge;
pub mod loader;
pub mod mux_prober;
pub mod vbios;

// ============================================================================
// Hardware Video Decoders
// ============================================================================
pub mod amf_decode;
pub mod amf_encoder;
pub mod decode;
pub mod h264_utils;
pub mod hw_decode;
pub mod nvdec;
pub mod vaapi_decode;

// ============================================================================
// Container Demuxers
// ============================================================================
pub mod avi_demux;
pub mod demuxer;
pub mod lav;
pub mod mkv;
pub mod mp4_demux;
pub mod ts_demux;

// ============================================================================
// DirectShow Integration (Windows)
// ============================================================================
#[cfg(windows)]
pub mod dshow;

// ============================================================================
// Video Processing Pipeline
// ============================================================================
pub mod deinterlace;
pub mod frame_interpolation;
pub mod video_pipeline;
pub mod vapoursynth_bridge;
pub mod potplayer_compat;
pub mod shader_filters;

// ============================================================================
// Media Processing
// ============================================================================
pub mod audio;
pub mod camera;
pub mod deinterlace;
pub mod filter_pipeline;
pub mod frame_interpolation;
pub mod frame_queue;
pub mod imaging;
pub mod pixel_convert;
pub mod shader_filters;
pub mod subtitles;
pub mod video_filters;

// ============================================================================
// Streaming / Network
// ============================================================================
pub mod debrid;
pub mod iptv;
pub mod protocol;
pub mod streaming;

// ============================================================================
// Pipelines (AVS/VS/Vulkan/CUDA)
// ============================================================================
pub mod bandwidth;
pub mod pipeline;
pub mod video_pipeline;
pub mod vapoursynth_bridge;

// ============================================================================
// Features
// ============================================================================
pub mod archive;
pub mod disc;
pub mod history;
pub mod media_library;
pub mod retro_tv;
pub mod tray;
pub mod voice;

// ============================================================================
// Experimental / Research
// ============================================================================
pub mod aegis;
pub mod block_mirror;
pub mod cosmic_movie;
pub mod forumyze;
pub mod fractal;
pub mod legal_evidence;
pub mod message_board;
pub mod starlight;

// ============================================================================
// Stubs (to be implemented)
// ============================================================================
pub mod render;
pub mod sync;

// ============================================================================
// Benchmarking
// ============================================================================
pub mod benchmark;
pub mod gpu_benchmark;

// ============================================================================
// Security
// ============================================================================
pub mod security_audit;

// ============================================================================
// Version
// ============================================================================
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
