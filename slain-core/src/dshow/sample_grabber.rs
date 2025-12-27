//! Sample Grabber for capturing decoded frames from DirectShow
//!
//! Implements ISampleGrabberCB to receive decoded video frames.

use parking_lot::Mutex;
use std::collections::VecDeque;
use std::sync::Arc;

use windows::core::GUID;

/// MEDIASUBTYPE_RGB24
const MEDIASUBTYPE_RGB24: GUID = GUID::from_u128(0xe436eb7d_524f_11ce_9f53_0020af0ba770);

/// MEDIASUBTYPE_NV12
const MEDIASUBTYPE_NV12: GUID = GUID::from_u128(0x3231564e_0000_0010_8000_00aa00389b71);

/// Captured video frame
#[derive(Clone)]
pub struct CapturedFrame {
    /// Frame data (RGB24 or NV12)
    pub data: Vec<u8>,
    /// Width in pixels
    pub width: u32,
    /// Height in pixels
    pub height: u32,
    /// Presentation time in 100ns units
    pub sample_time: i64,
    /// Frame number
    pub frame_number: u64,
    /// Is this a keyframe
    pub keyframe: bool,
}

/// Frame buffer for captured frames
pub struct FrameBuffer {
    /// Frame queue
    frames: Mutex<VecDeque<CapturedFrame>>,
    /// Maximum queue size
    max_size: usize,
    /// Total frames captured
    total_frames: std::sync::atomic::AtomicU64,
    /// Dropped frames count
    dropped_frames: std::sync::atomic::AtomicU64,
    /// Current video width
    width: std::sync::atomic::AtomicU32,
    /// Current video height
    height: std::sync::atomic::AtomicU32,
}

impl FrameBuffer {
    /// Create a new frame buffer
    pub fn new(max_size: usize) -> Arc<Self> {
        Arc::new(Self {
            frames: Mutex::new(VecDeque::with_capacity(max_size)),
            max_size,
            total_frames: std::sync::atomic::AtomicU64::new(0),
            dropped_frames: std::sync::atomic::AtomicU64::new(0),
            width: std::sync::atomic::AtomicU32::new(0),
            height: std::sync::atomic::AtomicU32::new(0),
        })
    }

    /// Push a new frame to the buffer
    pub fn push(&self, frame: CapturedFrame) {
        let mut queue = self.frames.lock();

        // Update dimensions
        self.width
            .store(frame.width, std::sync::atomic::Ordering::Relaxed);
        self.height
            .store(frame.height, std::sync::atomic::Ordering::Relaxed);

        // Drop oldest frame if buffer is full
        if queue.len() >= self.max_size {
            queue.pop_front();
            self.dropped_frames
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }

        queue.push_back(frame);
        self.total_frames
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    /// Pop a frame from the buffer
    pub fn pop(&self) -> Option<CapturedFrame> {
        self.frames.lock().pop_front()
    }

    /// Peek at the next frame without removing it
    pub fn peek(&self) -> Option<CapturedFrame> {
        self.frames.lock().front().cloned()
    }

    /// Get number of frames in buffer
    pub fn len(&self) -> usize {
        self.frames.lock().len()
    }

    /// Check if buffer is empty
    pub fn is_empty(&self) -> bool {
        self.frames.lock().is_empty()
    }

    /// Clear all frames
    pub fn clear(&self) {
        self.frames.lock().clear();
    }

    /// Get total frames captured
    pub fn total_frames(&self) -> u64 {
        self.total_frames.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Get dropped frames count
    pub fn dropped_frames(&self) -> u64 {
        self.dropped_frames
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Get current video dimensions
    pub fn dimensions(&self) -> (u32, u32) {
        (
            self.width.load(std::sync::atomic::Ordering::Relaxed),
            self.height.load(std::sync::atomic::Ordering::Relaxed),
        )
    }
}

/// Sample grabber callback mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrabberMode {
    /// Buffer samples in memory
    Buffer,
    /// Callback for each sample
    Callback,
    /// One-shot grab single frame
    OneShot,
}

/// Configuration for sample grabber
#[derive(Debug, Clone)]
pub struct SampleGrabberConfig {
    /// Grabber mode
    pub mode: GrabberMode,
    /// Desired output format (RGB24, NV12, etc.)
    pub output_format: GUID,
    /// Buffer size (number of frames)
    pub buffer_size: usize,
    /// Enable one-shot mode
    pub one_shot: bool,
}

impl Default for SampleGrabberConfig {
    fn default() -> Self {
        Self {
            mode: GrabberMode::Buffer,
            output_format: MEDIASUBTYPE_RGB24,
            buffer_size: 8,
            one_shot: false,
        }
    }
}

// ============================================================================
// Sample Grabber Callback Implementation
// ============================================================================

/// Callback data passed to the sample grabber
pub struct SampleGrabberCallback {
    /// Frame buffer
    buffer: Arc<FrameBuffer>,
    /// Media type info
    width: u32,
    height: u32,
    stride: i32,
    format: GUID,
    /// Frame counter
    frame_count: std::sync::atomic::AtomicU64,
}

impl SampleGrabberCallback {
    /// Create a new callback
    pub fn new(buffer: Arc<FrameBuffer>) -> Self {
        Self {
            buffer,
            width: 0,
            height: 0,
            stride: 0,
            format: MEDIASUBTYPE_RGB24,
            frame_count: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Set media type info
    pub fn set_media_type(&mut self, width: u32, height: u32, stride: i32, format: GUID) {
        self.width = width;
        self.height = height;
        self.stride = stride;
        self.format = format;
    }

    /// Process a sample (called from DirectShow thread)
    pub fn on_sample(&self, sample_time: f64, data: &[u8]) {
        let frame_num = self
            .frame_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        // Convert data based on format
        let frame_data = if self.format == MEDIASUBTYPE_RGB24 {
            // RGB24 - might need to flip vertically
            if self.stride < 0 {
                // Bottom-up DIB, need to flip
                self.flip_vertical_rgb24(data)
            } else {
                data.to_vec()
            }
        } else if self.format == MEDIASUBTYPE_NV12 {
            // NV12 - no conversion needed
            data.to_vec()
        } else {
            // Unknown format, just copy
            data.to_vec()
        };

        let frame = CapturedFrame {
            data: frame_data,
            width: self.width,
            height: self.height,
            sample_time: (sample_time * 10_000_000.0) as i64,
            frame_number: frame_num,
            keyframe: frame_num == 0, // Assume first frame is keyframe
        };

        self.buffer.push(frame);
    }

    /// Flip RGB24 data vertically (for bottom-up DIB)
    fn flip_vertical_rgb24(&self, data: &[u8]) -> Vec<u8> {
        let row_size = (self.width * 3) as usize;
        let height = self.height as usize;
        let mut flipped = vec![0u8; data.len()];

        for y in 0..height {
            let src_row = &data[y * row_size..(y + 1) * row_size];
            let dst_row = &mut flipped[(height - 1 - y) * row_size..(height - y) * row_size];
            dst_row.copy_from_slice(src_row);
        }

        flipped
    }

    /// Get frame buffer
    pub fn buffer(&self) -> &Arc<FrameBuffer> {
        &self.buffer
    }
}

// ============================================================================
// Null Renderer
// ============================================================================

/// Configuration for null renderer (discards output)
#[derive(Debug, Clone, Default)]
pub struct NullRendererConfig {
    /// Sync to clock
    pub sync_to_clock: bool,
}
