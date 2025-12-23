//! # SLAIN Core
//! 
//! Pure Rust GPU-accelerated video player and hardware toolkit.

#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]

// ============================================================================
// Core GPU / Hardware
// ============================================================================
pub mod gpu;
pub mod loader;
pub mod gpu_orchestrator;
pub mod driver_analysis;
pub mod hardware_bridge;
pub mod mux_prober;
pub mod vbios;

// ============================================================================
// Hardware Video Decoders
// ============================================================================
pub mod nvdec;
pub mod amf_decode;
pub mod amf_encoder;
pub mod vaapi_decode;
pub mod hw_decode;
pub mod decode;

// ============================================================================
// Container Demuxers
// ============================================================================
pub mod mkv;
pub mod avi_demux;
pub mod ts_demux;
pub mod mp4_demux;

// ============================================================================
// Media Processing
// ============================================================================
pub mod audio;
pub mod pixel_convert;
pub mod gpu_video_processor;
pub mod frame_queue;
pub mod subtitles;
pub mod imaging;

// ============================================================================
// Streaming / Network
// ============================================================================
pub mod streaming;
pub mod iptv;
pub mod debrid;
pub mod protocol;

// ============================================================================
// Pipelines (AVS/VS/Vulkan/CUDA)
// ============================================================================
pub mod pipeline;
pub mod bandwidth;

// ============================================================================
// Features
// ============================================================================
pub mod disc;
pub mod archive;
pub mod history;
pub mod media_library;
pub mod tray;
pub mod voice;
pub mod retro_tv;

// ============================================================================
// Experimental / Research
// ============================================================================
pub mod aegis;
pub mod forumyze;
pub mod cosmic_movie;
pub mod starlight;
pub mod block_mirror;

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
// Version
// ============================================================================
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
