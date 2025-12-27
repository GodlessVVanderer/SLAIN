//! DirectShow integration for LAV Filters
//!
//! Uses COM to build a DirectShow filter graph with LAV Splitter and LAV Video Decoder
//! for hardware-accelerated video decoding with CUVID.
//!
//! # Status
//!
//! This module provides the interface for LAV Filters integration.
//! Full DirectShow COM implementation is complex and not yet complete.
//!
//! # Alternatives
//!
//! For working hardware-accelerated video decoding, use:
//! - `slain_core::nvdec` - Direct NVDEC API (NVIDIA)
//! - `slain_core::hw_decode` - Unified hardware decoder interface

mod graph;
mod interfaces;
mod lav;
mod sample_grabber;

pub use graph::*;
pub use lav::*;
pub use sample_grabber::{CapturedFrame, FrameBuffer, GrabberMode, SampleGrabberConfig};

/// Check if LAV Filters are installed
pub fn lav_filters_installed() -> bool {
    lav::check_lav_installed()
}

/// Print LAV Filters installation status and instructions
pub fn check_lav_status() {
    graph::print_lav_status();
}
