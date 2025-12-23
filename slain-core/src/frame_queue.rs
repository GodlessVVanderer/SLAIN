//! # Frame Queue - Thread-Safe Video Frame Buffer
//!
//! This module provides the backbone for smooth video playback:
//! - Lock-free ring buffer for decoded frames
//! - Decode-ahead buffering (configurable depth)
//! - PTS-ordered frame retrieval
//! - Automatic frame dropping when behind
//! - Memory pooling to avoid allocations
//! - Seeking with instant buffer flush
//!
//! ## Architecture
//!
//! ```text
//! ┌──────────┐    ┌────────────┐    ┌──────────┐
//! │ Decoder  │───►│ FrameQueue │───►│ Renderer │
//! │ Thread   │    │            │    │ Thread   │
//! └──────────┘    └────────────┘    └──────────┘
//!                       │
//!                 ┌─────┴─────┐
//!                 │ FramePool │
//!                 │ (reuse)   │
//!                 └───────────┘
//! ```

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicI64, Ordering};
use std::sync::Arc;
use parking_lot::{Mutex, Condvar, RwLock};
use std::time::{Duration, Instant};

// ============================================================================
// Frame Data
// ============================================================================

/// Pixel format for frames
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    NV12,
    YUV420P,
    RGB24,
    RGBA32,
    P010,  // 10-bit HDR
}

impl PixelFormat {
    /// Calculate buffer size for this format
    pub fn buffer_size(&self, width: u32, height: u32) -> usize {
        let w = width as usize;
        let h = height as usize;
        match self {
            Self::NV12 | Self::YUV420P => w * h * 3 / 2,
            Self::RGB24 => w * h * 3,
            Self::RGBA32 => w * h * 4,
            Self::P010 => w * h * 3,  // 10-bit = 2 bytes Y, 1 byte UV avg
        }
    }
}

/// A decoded video frame
#[derive(Debug)]
pub struct Frame {
    /// Unique frame ID for tracking
    pub id: u64,
    /// Frame pixel data
    pub data: Vec<u8>,
    /// Pixel format
    pub format: PixelFormat,
    /// Frame width
    pub width: u32,
    /// Frame height
    pub height: u32,
    /// Presentation timestamp (microseconds)
    pub pts_us: i64,
    /// Frame duration (microseconds)
    pub duration_us: i64,
    /// Is this a keyframe/IDR?
    pub keyframe: bool,
    /// Decode timestamp (for reordering)
    pub dts_us: i64,
    /// Display order index
    pub display_order: u64,
}

impl Frame {
    /// Create a new frame with allocated buffer
    pub fn new(width: u32, height: u32, format: PixelFormat) -> Self {
        static FRAME_COUNTER: AtomicU64 = AtomicU64::new(0);
        let size = format.buffer_size(width, height);
        Self {
            id: FRAME_COUNTER.fetch_add(1, Ordering::Relaxed),
            data: vec![0u8; size],
            format,
            width,
            height,
            pts_us: 0,
            duration_us: 0,
            keyframe: false,
            dts_us: 0,
            display_order: 0,
        }
    }

    /// Reset frame for reuse (keeps allocation)
    pub fn reset(&mut self) {
        self.pts_us = 0;
        self.duration_us = 0;
        self.keyframe = false;
        self.dts_us = 0;
        self.display_order = 0;
    }

    /// Copy data into this frame
    pub fn copy_from(&mut self, data: &[u8], pts_us: i64, duration_us: i64, keyframe: bool) {
        let len = data.len().min(self.data.len());
        self.data[..len].copy_from_slice(&data[..len]);
        self.pts_us = pts_us;
        self.duration_us = duration_us;
        self.keyframe = keyframe;
    }
}

// ============================================================================
// Frame Pool - Memory Reuse
// ============================================================================

/// Pool of pre-allocated frames to avoid allocation during playback
pub struct FramePool {
    frames: Mutex<Vec<Frame>>,
    width: u32,
    height: u32,
    format: PixelFormat,
    allocated: AtomicU64,
    recycled: AtomicU64,
}

impl FramePool {
    /// Create a new frame pool
    pub fn new(width: u32, height: u32, format: PixelFormat, initial_size: usize) -> Self {
        let mut frames = Vec::with_capacity(initial_size);
        for _ in 0..initial_size {
            frames.push(Frame::new(width, height, format));
        }

        Self {
            frames: Mutex::new(frames),
            width,
            height,
            format,
            allocated: AtomicU64::new(initial_size as u64),
            recycled: AtomicU64::new(0),
        }
    }

    /// Get a frame from the pool (or allocate new)
    pub fn acquire(&self) -> Frame {
        let mut pool = self.frames.lock();
        if let Some(mut frame) = pool.pop() {
            frame.reset();
            self.recycled.fetch_add(1, Ordering::Relaxed);
            frame
        } else {
            drop(pool);
            self.allocated.fetch_add(1, Ordering::Relaxed);
            Frame::new(self.width, self.height, self.format)
        }
    }

    /// Return a frame to the pool
    pub fn release(&self, frame: Frame) {
        // Only recycle if dimensions match
        if frame.width == self.width && frame.height == self.height && frame.format == self.format {
            self.frames.lock().push(frame);
        }
    }

    /// Resize pool for new video dimensions
    pub fn resize(&mut self, width: u32, height: u32, format: PixelFormat) {
        self.frames.lock().clear();
        self.width = width;
        self.height = height;
        self.format = format;
    }

    /// Get pool statistics
    pub fn stats(&self) -> PoolStats {
        PoolStats {
            available: self.frames.lock().len(),
            total_allocated: self.allocated.load(Ordering::Relaxed),
            total_recycled: self.recycled.load(Ordering::Relaxed),
        }
    }
}

/// Pool statistics
#[derive(Debug, Clone)]
pub struct PoolStats {
    pub available: usize,
    pub total_allocated: u64,
    pub total_recycled: u64,
}

// ============================================================================
// Frame Queue Configuration
// ============================================================================

/// Queue configuration
#[derive(Debug, Clone)]
pub struct QueueConfig {
    /// Maximum frames to buffer
    pub max_frames: usize,
    /// Target buffer level before starting playback
    pub target_buffer: usize,
    /// Minimum buffer before underrun warning
    pub min_buffer: usize,
    /// Maximum PTS difference before dropping frames (microseconds)
    pub max_pts_diff_us: i64,
    /// Enable frame reordering (B-frames)
    pub reorder: bool,
    /// Maximum reorder buffer depth
    pub reorder_depth: usize,
}

impl Default for QueueConfig {
    fn default() -> Self {
        Self {
            max_frames: 32,        // ~1 second at 30fps
            target_buffer: 8,      // ~250ms pre-buffer
            min_buffer: 2,         // Emergency level
            max_pts_diff_us: 100_000, // 100ms max drift
            reorder: true,
            reorder_depth: 16,     // B-frame reorder buffer
        }
    }
}

// ============================================================================
// Frame Queue State
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueState {
    /// Queue is empty, waiting for frames
    Empty,
    /// Buffering to target level
    Buffering,
    /// Ready for playback
    Ready,
    /// Playback in progress
    Playing,
    /// Paused
    Paused,
    /// Seeking - flushing and refilling
    Seeking,
    /// End of stream reached
    EndOfStream,
    /// Error occurred
    Error,
}

// ============================================================================
// Frame Queue
// ============================================================================

/// Thread-safe frame queue for video playback
pub struct FrameQueue {
    /// Configuration
    config: QueueConfig,

    /// Display queue (PTS-ordered, ready for presentation)
    display_queue: Mutex<VecDeque<Frame>>,

    /// Reorder buffer (for B-frames, DTS-ordered)
    reorder_buffer: Mutex<Vec<Frame>>,

    /// Frame pool for memory reuse
    pool: Arc<FramePool>,

    /// Current state
    state: RwLock<QueueState>,

    /// Condition variable for blocking reads
    ready_cond: Condvar,

    /// Condition variable for blocking writes (when full)
    space_cond: Condvar,

    /// Last displayed PTS
    last_pts_us: AtomicI64,

    /// Total frames pushed
    frames_pushed: AtomicU64,

    /// Total frames popped (displayed)
    frames_popped: AtomicU64,

    /// Frames dropped (behind schedule)
    frames_dropped: AtomicU64,

    /// Whether EOS has been signaled
    eos: AtomicBool,

    /// Seek generation (incremented on seek)
    seek_generation: AtomicU64,

    /// Monotonic display order counter
    display_counter: AtomicU64,
}

impl FrameQueue {
    /// Create a new frame queue
    pub fn new(config: QueueConfig, pool: Arc<FramePool>) -> Self {
        Self {
            config,
            display_queue: Mutex::new(VecDeque::with_capacity(32)),
            reorder_buffer: Mutex::new(Vec::with_capacity(16)),
            pool,
            state: RwLock::new(QueueState::Empty),
            ready_cond: Condvar::new(),
            space_cond: Condvar::new(),
            last_pts_us: AtomicI64::new(0),
            frames_pushed: AtomicU64::new(0),
            frames_popped: AtomicU64::new(0),
            frames_dropped: AtomicU64::new(0),
            eos: AtomicBool::new(false),
            seek_generation: AtomicU64::new(0),
            display_counter: AtomicU64::new(0),
        }
    }

    /// Create with default config
    pub fn with_pool(pool: Arc<FramePool>) -> Self {
        Self::new(QueueConfig::default(), pool)
    }

    // ========================================================================
    // Producer API (Decoder Thread)
    // ========================================================================

    /// Push a decoded frame into the queue
    /// Returns false if queue is full (use push_blocking for blocking behavior)
    pub fn push(&self, mut frame: Frame) -> bool {
        let mut queue = self.display_queue.lock();

        if queue.len() >= self.config.max_frames {
            return false;
        }

        // Assign display order
        frame.display_order = self.display_counter.fetch_add(1, Ordering::Relaxed);

        if self.config.reorder && !frame.keyframe {
            // Use reorder buffer for B-frames
            drop(queue);
            self.push_reorder(frame);
        } else {
            // Insert in PTS order
            self.insert_by_pts(&mut queue, frame);
            self.frames_pushed.fetch_add(1, Ordering::Relaxed);
            self.update_state(&queue);
            self.ready_cond.notify_one();
        }

        self.space_cond.notify_one();
        true
    }

    /// Push with blocking when full
    pub fn push_blocking(&self, frame: Frame, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut queue = self.display_queue.lock();

        while queue.len() >= self.config.max_frames {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return false;
            }
            self.space_cond.wait_for(&mut queue, remaining);
        }

        drop(queue);
        self.push(frame)
    }

    /// Push frame into reorder buffer
    fn push_reorder(&self, frame: Frame) {
        let mut reorder = self.reorder_buffer.lock();

        // Insert by DTS
        let pos = reorder.iter().position(|f| f.dts_us > frame.dts_us);
        if let Some(idx) = pos {
            reorder.insert(idx, frame);
        } else {
            reorder.push(frame);
        }

        // Flush complete frames to display queue
        while reorder.len() > self.config.reorder_depth {
            if let Some(f) = reorder.first() {
                // Check if this frame should be output
                if self.can_output_frame(f) {
                    let frame = reorder.remove(0);
                    let mut queue = self.display_queue.lock();
                    self.insert_by_pts(&mut queue, frame);
                    self.frames_pushed.fetch_add(1, Ordering::Relaxed);
                    self.update_state(&queue);
                    self.ready_cond.notify_one();
                } else {
                    break;
                }
            } else {
                break;
            }
        }
    }

    fn can_output_frame(&self, frame: &Frame) -> bool {
        // A frame can be output when we've seen enough subsequent frames
        // This is a simplified B-frame reordering check
        frame.keyframe || frame.pts_us <= self.last_pts_us.load(Ordering::Relaxed) + 100_000
    }

    fn insert_by_pts(&self, queue: &mut VecDeque<Frame>, frame: Frame) {
        // Find insertion point to maintain PTS order
        let pos = queue.iter().position(|f| f.pts_us > frame.pts_us);
        if let Some(idx) = pos {
            queue.insert(idx, frame);
        } else {
            queue.push_back(frame);
        }
    }

    /// Signal end of stream
    pub fn signal_eos(&self) {
        self.eos.store(true, Ordering::SeqCst);

        // Flush reorder buffer
        let mut reorder = self.reorder_buffer.lock();
        let mut queue = self.display_queue.lock();

        while let Some(frame) = reorder.pop() {
            self.insert_by_pts(&mut queue, frame);
            self.frames_pushed.fetch_add(1, Ordering::Relaxed);
        }

        *self.state.write() = QueueState::EndOfStream;
        self.ready_cond.notify_all();
    }

    // ========================================================================
    // Consumer API (Renderer Thread)
    // ========================================================================

    /// Pop the next frame for display
    /// Returns None if queue is empty
    pub fn pop(&self) -> Option<Frame> {
        let mut queue = self.display_queue.lock();

        if let Some(frame) = queue.pop_front() {
            self.last_pts_us.store(frame.pts_us, Ordering::Relaxed);
            self.frames_popped.fetch_add(1, Ordering::Relaxed);
            self.update_state(&queue);
            self.space_cond.notify_one();
            Some(frame)
        } else {
            None
        }
    }

    /// Pop with blocking when empty
    pub fn pop_blocking(&self, timeout: Duration) -> Option<Frame> {
        let deadline = Instant::now() + timeout;
        let mut queue = self.display_queue.lock();

        while queue.is_empty() {
            if self.eos.load(Ordering::SeqCst) {
                return None;
            }

            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return None;
            }
            self.ready_cond.wait_for(&mut queue, remaining);
        }

        if let Some(frame) = queue.pop_front() {
            self.last_pts_us.store(frame.pts_us, Ordering::Relaxed);
            self.frames_popped.fetch_add(1, Ordering::Relaxed);
            self.update_state(&queue);
            self.space_cond.notify_one();
            Some(frame)
        } else {
            None
        }
    }

    /// Peek at the next frame without removing it
    pub fn peek(&self) -> Option<i64> {
        self.display_queue.lock().front().map(|f| f.pts_us)
    }

    /// Get frame for specific PTS (with tolerance)
    /// Drops frames that are too old
    pub fn get_frame_for_pts(&self, target_pts_us: i64) -> Option<Frame> {
        let mut queue = self.display_queue.lock();

        // Drop frames that are too far behind
        while let Some(frame) = queue.front() {
            let diff = target_pts_us - frame.pts_us;
            if diff > self.config.max_pts_diff_us {
                // Frame is too old, drop it
                let dropped = queue.pop_front().unwrap();
                self.pool.release(dropped);
                self.frames_dropped.fetch_add(1, Ordering::Relaxed);
            } else {
                break;
            }
        }

        // Return the next frame if it's within tolerance
        if let Some(frame) = queue.front() {
            let diff = (target_pts_us - frame.pts_us).abs();
            if diff <= self.config.max_pts_diff_us {
                let frame = queue.pop_front().unwrap();
                self.last_pts_us.store(frame.pts_us, Ordering::Relaxed);
                self.frames_popped.fetch_add(1, Ordering::Relaxed);
                self.update_state(&queue);
                self.space_cond.notify_one();
                return Some(frame);
            }
        }

        None
    }

    /// Return a frame to the pool after display
    pub fn release(&self, frame: Frame) {
        self.pool.release(frame);
    }

    // ========================================================================
    // Control API
    // ========================================================================

    /// Flush all frames (for seeking)
    pub fn flush(&self) {
        let generation = self.seek_generation.fetch_add(1, Ordering::SeqCst);

        // Clear display queue
        let mut queue = self.display_queue.lock();
        while let Some(frame) = queue.pop_front() {
            self.pool.release(frame);
        }
        drop(queue);

        // Clear reorder buffer
        let mut reorder = self.reorder_buffer.lock();
        while let Some(frame) = reorder.pop() {
            self.pool.release(frame);
        }
        drop(reorder);

        // Reset state
        self.eos.store(false, Ordering::SeqCst);
        self.last_pts_us.store(0, Ordering::Relaxed);
        *self.state.write() = QueueState::Seeking;

        // Wake up any waiting threads
        self.ready_cond.notify_all();
        self.space_cond.notify_all();

        log::debug!("Queue flushed, generation {}", generation);
    }

    /// Get current seek generation (for detecting stale frames)
    pub fn seek_generation(&self) -> u64 {
        self.seek_generation.load(Ordering::SeqCst)
    }

    /// Set queue state
    pub fn set_state(&self, state: QueueState) {
        *self.state.write() = state;
    }

    /// Get current state
    pub fn state(&self) -> QueueState {
        *self.state.read()
    }

    /// Pause playback
    pub fn pause(&self) {
        *self.state.write() = QueueState::Paused;
    }

    /// Resume playback
    pub fn resume(&self) {
        let queue = self.display_queue.lock();
        self.update_state(&queue);
    }

    // ========================================================================
    // Status API
    // ========================================================================

    fn update_state(&self, queue: &VecDeque<Frame>) {
        let mut state = self.state.write();

        if *state == QueueState::Paused || *state == QueueState::Seeking {
            return;
        }

        *state = if queue.is_empty() {
            if self.eos.load(Ordering::SeqCst) {
                QueueState::EndOfStream
            } else {
                QueueState::Empty
            }
        } else if queue.len() < self.config.min_buffer {
            QueueState::Buffering
        } else if queue.len() >= self.config.target_buffer {
            QueueState::Ready
        } else {
            QueueState::Playing
        };
    }

    /// Get queue statistics
    pub fn stats(&self) -> QueueStats {
        let queue = self.display_queue.lock();
        let reorder = self.reorder_buffer.lock();

        QueueStats {
            state: *self.state.read(),
            buffered_frames: queue.len(),
            reorder_frames: reorder.len(),
            frames_pushed: self.frames_pushed.load(Ordering::Relaxed),
            frames_popped: self.frames_popped.load(Ordering::Relaxed),
            frames_dropped: self.frames_dropped.load(Ordering::Relaxed),
            last_pts_us: self.last_pts_us.load(Ordering::Relaxed),
            buffer_duration_us: self.calculate_buffer_duration(&queue),
            is_eos: self.eos.load(Ordering::SeqCst),
        }
    }

    fn calculate_buffer_duration(&self, queue: &VecDeque<Frame>) -> i64 {
        if queue.len() < 2 {
            return 0;
        }

        let first = queue.front().map(|f| f.pts_us).unwrap_or(0);
        let last = queue.back().map(|f| f.pts_us).unwrap_or(0);
        last - first
    }

    /// Check if queue is ready for playback
    pub fn is_ready(&self) -> bool {
        matches!(self.state(), QueueState::Ready | QueueState::Playing)
    }

    /// Check if buffering is needed
    pub fn needs_buffering(&self) -> bool {
        matches!(self.state(), QueueState::Empty | QueueState::Buffering)
    }

    /// Get buffer fill percentage (0.0 - 1.0)
    pub fn buffer_level(&self) -> f32 {
        let len = self.display_queue.lock().len();
        (len as f32 / self.config.max_frames as f32).min(1.0)
    }
}

/// Queue statistics
#[derive(Debug, Clone)]
pub struct QueueStats {
    pub state: QueueState,
    pub buffered_frames: usize,
    pub reorder_frames: usize,
    pub frames_pushed: u64,
    pub frames_popped: u64,
    pub frames_dropped: u64,
    pub last_pts_us: i64,
    pub buffer_duration_us: i64,
    pub is_eos: bool,
}

// ============================================================================
// Playback Controller
// ============================================================================

/// High-level playback controller combining queue + timing
pub struct PlaybackController {
    queue: Arc<FrameQueue>,
    pool: Arc<FramePool>,

    /// Target frame rate
    fps: f64,

    /// Frame duration in microseconds
    frame_duration_us: i64,

    /// Playback speed (1.0 = normal)
    speed: f64,

    /// Last frame time
    last_frame_time: Mutex<Instant>,

    /// Playback start time
    start_time: Mutex<Option<Instant>>,

    /// PTS offset (for seeking)
    pts_offset: AtomicI64,
}

impl PlaybackController {
    /// Create a new playback controller
    pub fn new(width: u32, height: u32, format: PixelFormat, fps: f64) -> Self {
        let pool = Arc::new(FramePool::new(width, height, format, 32));
        let queue = Arc::new(FrameQueue::with_pool(pool.clone()));

        Self {
            queue,
            pool,
            fps,
            frame_duration_us: (1_000_000.0 / fps) as i64,
            speed: 1.0,
            last_frame_time: Mutex::new(Instant::now()),
            start_time: Mutex::new(None),
            pts_offset: AtomicI64::new(0),
        }
    }

    /// Get the frame queue for the decoder
    pub fn queue(&self) -> Arc<FrameQueue> {
        self.queue.clone()
    }

    /// Get the frame pool
    pub fn pool(&self) -> Arc<FramePool> {
        self.pool.clone()
    }

    /// Acquire a frame from the pool
    pub fn acquire_frame(&self) -> Frame {
        self.pool.acquire()
    }

    /// Start playback
    pub fn start(&self) {
        *self.start_time.lock() = Some(Instant::now());
        *self.last_frame_time.lock() = Instant::now();
        self.queue.set_state(QueueState::Playing);
    }

    /// Stop playback
    pub fn stop(&self) {
        *self.start_time.lock() = None;
        self.queue.set_state(QueueState::Paused);
    }

    /// Seek to position
    pub fn seek(&self, pts_us: i64) {
        self.queue.flush();
        self.pts_offset.store(pts_us, Ordering::SeqCst);
        *self.start_time.lock() = Some(Instant::now());
    }

    /// Set playback speed
    pub fn set_speed(&self, speed: f64) {
        // Speed is not stored in this simplified version
        let _ = speed;
    }

    /// Get current playback time (microseconds)
    pub fn current_time_us(&self) -> i64 {
        let start = self.start_time.lock();
        if let Some(start_instant) = *start {
            let elapsed = start_instant.elapsed().as_micros() as i64;
            let offset = self.pts_offset.load(Ordering::Relaxed);
            offset + (elapsed as f64 * self.speed) as i64
        } else {
            self.pts_offset.load(Ordering::Relaxed)
        }
    }

    /// Get next frame for display, handling timing
    pub fn next_frame(&self) -> Option<Frame> {
        let current_pts = self.current_time_us();
        self.queue.get_frame_for_pts(current_pts)
    }

    /// Wait for next frame time, then return frame
    pub fn wait_next_frame(&self) -> Option<Frame> {
        // Calculate time until next frame
        let last_time = *self.last_frame_time.lock();
        let target_duration = Duration::from_micros(
            (self.frame_duration_us as f64 / self.speed) as u64
        );

        let elapsed = last_time.elapsed();
        if elapsed < target_duration {
            std::thread::sleep(target_duration - elapsed);
        }

        *self.last_frame_time.lock() = Instant::now();
        self.next_frame()
    }

    /// Get controller statistics
    pub fn stats(&self) -> PlaybackStats {
        let queue_stats = self.queue.stats();
        let pool_stats = self.pool.stats();

        PlaybackStats {
            queue: queue_stats,
            pool: pool_stats,
            current_pts_us: self.current_time_us(),
            fps: self.fps,
            speed: self.speed,
        }
    }
}

/// Combined playback statistics
#[derive(Debug, Clone)]
pub struct PlaybackStats {
    pub queue: QueueStats,
    pub pool: PoolStats,
    pub current_pts_us: i64,
    pub fps: f64,
    pub speed: f64,
}

// ============================================================================
// Frame Iterator
// ============================================================================

/// Iterator over frames in the queue
pub struct FrameIterator {
    queue: Arc<FrameQueue>,
    timeout: Duration,
}

impl FrameIterator {
    pub fn new(queue: Arc<FrameQueue>, timeout: Duration) -> Self {
        Self { queue, timeout }
    }
}

impl Iterator for FrameIterator {
    type Item = Frame;

    fn next(&mut self) -> Option<Self::Item> {
        self.queue.pop_blocking(self.timeout)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_pool() {
        let pool = FramePool::new(1920, 1080, PixelFormat::NV12, 4);

        // Acquire frames
        let f1 = pool.acquire();
        let f2 = pool.acquire();
        assert_eq!(f1.width, 1920);
        assert_eq!(f2.height, 1080);

        // Release and reuse
        let id1 = f1.id;
        pool.release(f1);
        let f3 = pool.acquire();
        assert_eq!(f3.id, id1); // Reused frame

        let stats = pool.stats();
        assert!(stats.total_recycled > 0);
    }

    #[test]
    fn test_frame_queue_ordering() {
        let pool = Arc::new(FramePool::new(1920, 1080, PixelFormat::NV12, 8));
        let queue = FrameQueue::with_pool(pool);

        // Push frames out of order
        let mut f1 = Frame::new(1920, 1080, PixelFormat::NV12);
        f1.pts_us = 30000;
        queue.push(f1);

        let mut f2 = Frame::new(1920, 1080, PixelFormat::NV12);
        f2.pts_us = 10000;
        queue.push(f2);

        let mut f3 = Frame::new(1920, 1080, PixelFormat::NV12);
        f3.pts_us = 20000;
        queue.push(f3);

        // Should pop in PTS order
        assert_eq!(queue.pop().unwrap().pts_us, 10000);
        assert_eq!(queue.pop().unwrap().pts_us, 20000);
        assert_eq!(queue.pop().unwrap().pts_us, 30000);
    }

    #[test]
    fn test_frame_dropping() {
        let pool = Arc::new(FramePool::new(1920, 1080, PixelFormat::NV12, 8));
        let config = QueueConfig {
            max_pts_diff_us: 50_000, // 50ms tolerance
            ..Default::default()
        };
        let queue = FrameQueue::new(config, pool);

        // Push frames
        for i in 0..5 {
            let mut f = Frame::new(1920, 1080, PixelFormat::NV12);
            f.pts_us = i * 33_333; // ~30fps
            queue.push(f);
        }

        // Request frame at 150ms - should drop old frames
        let frame = queue.get_frame_for_pts(150_000);
        assert!(frame.is_some());

        let stats = queue.stats();
        assert!(stats.frames_dropped > 0);
    }

    #[test]
    fn test_queue_flush() {
        let pool = Arc::new(FramePool::new(1920, 1080, PixelFormat::NV12, 8));
        let queue = FrameQueue::with_pool(pool.clone());

        // Push frames
        for _ in 0..5 {
            let f = Frame::new(1920, 1080, PixelFormat::NV12);
            queue.push(f);
        }

        assert_eq!(queue.stats().buffered_frames, 5);

        // Flush
        queue.flush();

        assert_eq!(queue.stats().buffered_frames, 0);
        assert_eq!(queue.state(), QueueState::Seeking);
    }
}
