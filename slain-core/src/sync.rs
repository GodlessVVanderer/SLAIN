//! Audio/Video Synchronization
//!
//! Uses audio as master clock. Video adjusts to match.

use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Sync action for video frame
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncAction {
    Display,
    Drop,
    Repeat,
    WaitMs(u32),
}

/// Audio clock - updated from audio callback
pub struct AudioClock {
    pts_us: AtomicI64,
    last_update: Mutex<Instant>,
    sample_rate: u32,
    samples_played: AtomicU64,
    playing: AtomicBool,
}

impl AudioClock {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            pts_us: AtomicI64::new(0),
            last_update: Mutex::new(Instant::now()),
            sample_rate,
            samples_played: AtomicU64::new(0),
            playing: AtomicBool::new(false),
        }
    }

    pub fn update(&self, pts_us: i64) {
        self.pts_us.store(pts_us, Ordering::SeqCst);
        *self.last_update.lock() = Instant::now();
        self.samples_played.store(0, Ordering::SeqCst);
    }

    pub fn add_samples(&self, n: u64) {
        self.samples_played.fetch_add(n, Ordering::SeqCst);
    }

    pub fn time_us(&self) -> i64 {
        if !self.playing.load(Ordering::SeqCst) {
            return self.pts_us.load(Ordering::SeqCst);
        }
        let base = self.pts_us.load(Ordering::SeqCst);
        let samples = self.samples_played.load(Ordering::SeqCst);
        let sample_us = (samples as i64 * 1_000_000) / self.sample_rate as i64;
        base + sample_us
    }

    pub fn time_ms(&self) -> i64 {
        self.time_us() / 1000
    }

    pub fn set_playing(&self, p: bool) {
        self.playing.store(p, Ordering::SeqCst);
        if p {
            *self.last_update.lock() = Instant::now();
        }
    }
}

/// Video clock
pub struct VideoClock {
    pts_us: AtomicI64,
    frame_dur_us: AtomicI64,
    last_display: Mutex<Instant>,
}

impl VideoClock {
    pub fn new(fps: f64) -> Self {
        let dur = if fps > 0.0 {
            (1_000_000.0 / fps) as i64
        } else {
            33333
        };
        Self {
            pts_us: AtomicI64::new(0),
            frame_dur_us: AtomicI64::new(dur),
            last_display: Mutex::new(Instant::now()),
        }
    }

    pub fn update(&self, pts_us: i64) {
        self.pts_us.store(pts_us, Ordering::SeqCst);
        *self.last_display.lock() = Instant::now();
    }

    pub fn set_fps(&self, fps: f64) {
        if fps > 0.0 {
            self.frame_dur_us
                .store((1_000_000.0 / fps) as i64, Ordering::SeqCst);
        }
    }

    pub fn pts_us(&self) -> i64 {
        self.pts_us.load(Ordering::SeqCst)
    }
    pub fn pts_ms(&self) -> i64 {
        self.pts_us() / 1000
    }
    pub fn frame_duration_us(&self) -> i64 {
        self.frame_dur_us.load(Ordering::SeqCst)
    }
}

/// A/V sync controller
pub struct SyncController {
    audio: Arc<AudioClock>,
    video: Arc<VideoClock>,
    threshold_us: i64,
    max_correction_us: i64,
    paused: AtomicBool,
    seeking: AtomicBool,
    frames_displayed: AtomicU64,
    frames_dropped: AtomicU64,
}

impl SyncController {
    pub fn new(sample_rate: u32, fps: f64) -> Self {
        Self {
            audio: Arc::new(AudioClock::new(sample_rate)),
            video: Arc::new(VideoClock::new(fps)),
            threshold_us: 40_000,       // 40ms
            max_correction_us: 100_000, // 100ms max correction
            paused: AtomicBool::new(true),
            seeking: AtomicBool::new(false),
            frames_displayed: AtomicU64::new(0),
            frames_dropped: AtomicU64::new(0),
        }
    }

    pub fn audio_clock(&self) -> Arc<AudioClock> {
        self.audio.clone()
    }
    pub fn video_clock(&self) -> Arc<VideoClock> {
        self.video.clone()
    }

    /// Drift in microseconds. Positive = video ahead.
    pub fn drift_us(&self) -> i64 {
        self.video.pts_us() - self.audio.time_us()
    }

    pub fn drift_ms(&self) -> i64 {
        self.drift_us() / 1000
    }

    /// What to do with frame at given PTS
    pub fn action(&self, frame_pts_us: i64) -> SyncAction {
        if self.paused.load(Ordering::SeqCst) || self.seeking.load(Ordering::SeqCst) {
            return SyncAction::Display;
        }

        let audio_time = self.audio.time_us();
        let drift = frame_pts_us - audio_time;

        if drift.abs() <= self.threshold_us {
            self.frames_displayed.fetch_add(1, Ordering::Relaxed);
            SyncAction::Display
        } else if drift > self.threshold_us {
            // Video ahead - wait
            let wait = drift.min(self.max_correction_us);
            SyncAction::WaitMs((wait / 1000) as u32)
        } else {
            // Video behind - drop
            if drift.abs() > self.max_correction_us {
                self.frames_dropped.fetch_add(1, Ordering::Relaxed);
                SyncAction::Drop
            } else {
                self.frames_displayed.fetch_add(1, Ordering::Relaxed);
                SyncAction::Display
            }
        }
    }

    pub fn begin_seek(&self) {
        self.seeking.store(true, Ordering::SeqCst);
    }

    pub fn end_seek(&self, pts_us: i64) {
        self.audio.update(pts_us);
        self.video.update(pts_us);
        self.seeking.store(false, Ordering::SeqCst);
    }

    pub fn set_paused(&self, p: bool) {
        self.paused.store(p, Ordering::SeqCst);
        self.audio.set_playing(!p);
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }

    pub fn stats(&self) -> (u64, u64) {
        (
            self.frames_displayed.load(Ordering::Relaxed),
            self.frames_dropped.load(Ordering::Relaxed),
        )
    }
}

/// Frame timer for smooth playback
pub struct FrameTimer {
    target: Duration,
    last: Instant,
    count: u64,
    start: Instant,
}

impl FrameTimer {
    pub fn new(fps: f64) -> Self {
        let target = if fps > 0.0 {
            Duration::from_secs_f64(1.0 / fps)
        } else {
            Duration::from_millis(33)
        };
        Self {
            target,
            last: Instant::now(),
            count: 0,
            start: Instant::now(),
        }
    }

    pub fn set_fps(&mut self, fps: f64) {
        if fps > 0.0 {
            self.target = Duration::from_secs_f64(1.0 / fps);
        }
    }

    pub fn wait(&mut self) {
        let elapsed = self.last.elapsed();
        if elapsed < self.target {
            std::thread::sleep(self.target - elapsed);
        }
        self.last = Instant::now();
        self.count += 1;
    }

    pub fn time_until_next(&self) -> Duration {
        let elapsed = self.last.elapsed();
        if elapsed >= self.target {
            Duration::ZERO
        } else {
            self.target - elapsed
        }
    }

    pub fn tick(&mut self) {
        self.last = Instant::now();
        self.count += 1;
    }

    pub fn fps(&self) -> f64 {
        let secs = self.start.elapsed().as_secs_f64();
        if secs > 0.0 {
            self.count as f64 / secs
        } else {
            0.0
        }
    }

    pub fn reset(&mut self) {
        self.last = Instant::now();
        self.count = 0;
        self.start = Instant::now();
    }
}
