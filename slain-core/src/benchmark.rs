//! # Professional Video Decoding Benchmark Suite
//!
//! Comprehensive benchmarking for SLAIN video decoders.
//!
//! Features:
//! - Hardware decoder benchmarks (NVDEC, AMF, VA-API)
//! - Software decoder benchmarks (OpenH264, dav1d)
//! - Statistical analysis (median, percentiles, std dev)
//! - Synthetic test pattern generation
//! - JSON report export
//! - Comparison utilities

use std::collections::HashMap;
use std::time::{Duration, Instant};
use serde::{Deserialize, Serialize};

// ============================================================================
// Benchmark Results
// ============================================================================

/// Individual frame timing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameTiming {
    pub frame_number: u64,
    pub decode_time_us: u64,
    pub size_bytes: usize,
    pub is_keyframe: bool,
}

/// Statistical summary of frame timings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimingStats {
    pub count: u64,
    pub total_time_us: u64,
    pub min_us: u64,
    pub max_us: u64,
    pub mean_us: f64,
    pub median_us: f64,
    pub std_dev_us: f64,
    pub p50_us: f64,
    pub p90_us: f64,
    pub p95_us: f64,
    pub p99_us: f64,
    pub fps: f64,
}

impl TimingStats {
    /// Calculate statistics from a list of frame timings
    pub fn from_timings(timings: &[FrameTiming]) -> Self {
        if timings.is_empty() {
            return Self::default();
        }

        let mut times: Vec<u64> = timings.iter().map(|t| t.decode_time_us).collect();
        times.sort_unstable();

        let count = times.len() as u64;
        let total: u64 = times.iter().sum();
        let min = *times.first().unwrap();
        let max = *times.last().unwrap();
        let mean = total as f64 / count as f64;

        // Median
        let median = if times.len() % 2 == 0 {
            let mid = times.len() / 2;
            (times[mid - 1] + times[mid]) as f64 / 2.0
        } else {
            times[times.len() / 2] as f64
        };

        // Standard deviation
        let variance: f64 = times.iter()
            .map(|&t| {
                let diff = t as f64 - mean;
                diff * diff
            })
            .sum::<f64>() / count as f64;
        let std_dev = variance.sqrt();

        // Percentiles
        let p50 = percentile(&times, 50.0);
        let p90 = percentile(&times, 90.0);
        let p95 = percentile(&times, 95.0);
        let p99 = percentile(&times, 99.0);

        // FPS (frames per second based on total time)
        let fps = if total > 0 {
            count as f64 / (total as f64 / 1_000_000.0)
        } else {
            0.0
        };

        Self {
            count,
            total_time_us: total,
            min_us: min,
            max_us: max,
            mean_us: mean,
            median_us: median,
            std_dev_us: std_dev,
            p50_us: p50,
            p90_us: p90,
            p95_us: p95,
            p99_us: p99,
            fps,
        }
    }
}

impl Default for TimingStats {
    fn default() -> Self {
        Self {
            count: 0,
            total_time_us: 0,
            min_us: 0,
            max_us: 0,
            mean_us: 0.0,
            median_us: 0.0,
            std_dev_us: 0.0,
            p50_us: 0.0,
            p90_us: 0.0,
            p95_us: 0.0,
            p99_us: 0.0,
            fps: 0.0,
        }
    }
}

fn percentile(sorted: &[u64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = (p / 100.0 * (sorted.len() - 1) as f64).round() as usize;
    sorted[idx.min(sorted.len() - 1)] as f64
}

/// Performance rating
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Rating {
    Excellent,  // 3x+ realtime
    Good,       // 1.5-3x realtime
    Acceptable, // 1-1.5x realtime
    Poor,       // Below realtime
}

impl Rating {
    pub fn from_fps(decode_fps: f64, target_fps: f64) -> Self {
        let ratio = decode_fps / target_fps;
        if ratio >= 3.0 {
            Rating::Excellent
        } else if ratio >= 1.5 {
            Rating::Good
        } else if ratio >= 1.0 {
            Rating::Acceptable
        } else {
            Rating::Poor
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Rating::Excellent => "Excellent",
            Rating::Good => "Good",
            Rating::Acceptable => "Acceptable",
            Rating::Poor => "Poor",
        }
    }

    pub fn emoji(&self) -> &'static str {
        match self {
            Rating::Excellent => "🚀",
            Rating::Good => "✅",
            Rating::Acceptable => "⚠️",
            Rating::Poor => "❌",
        }
    }
}

/// Complete benchmark result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkResult {
    pub name: String,
    pub decoder: String,
    pub codec: String,
    pub resolution: String,
    pub target_fps: f64,
    pub stats: TimingStats,
    pub rating: Rating,
    pub timestamp: String,
    pub system_info: SystemInfo,
    pub keyframe_stats: Option<TimingStats>,
    pub interframe_stats: Option<TimingStats>,
}

impl BenchmarkResult {
    /// Generate human-readable report
    pub fn report(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!("═══════════════════════════════════════════════════════════\n"));
        s.push_str(&format!("  {} Benchmark: {}\n", self.rating.emoji(), self.name));
        s.push_str(&format!("═══════════════════════════════════════════════════════════\n\n"));

        s.push_str(&format!("  Decoder:     {}\n", self.decoder));
        s.push_str(&format!("  Codec:       {}\n", self.codec));
        s.push_str(&format!("  Resolution:  {}\n", self.resolution));
        s.push_str(&format!("  Target FPS:  {:.1}\n\n", self.target_fps));

        s.push_str(&format!("  Results:\n"));
        s.push_str(&format!("  ─────────────────────────────────────────────────────────\n"));
        s.push_str(&format!("  Frames decoded:     {:>10}\n", self.stats.count));
        s.push_str(&format!("  Average FPS:        {:>10.1}\n", self.stats.fps));
        s.push_str(&format!("  Median decode:      {:>10.2} ms\n", self.stats.median_us as f64 / 1000.0));
        s.push_str(&format!("  Mean decode:        {:>10.2} ms\n", self.stats.mean_us / 1000.0));
        s.push_str(&format!("  Min decode:         {:>10.2} ms\n", self.stats.min_us as f64 / 1000.0));
        s.push_str(&format!("  Max decode:         {:>10.2} ms\n", self.stats.max_us as f64 / 1000.0));
        s.push_str(&format!("  Std deviation:      {:>10.2} ms\n", self.stats.std_dev_us / 1000.0));
        s.push_str(&format!("\n"));
        s.push_str(&format!("  Percentiles:\n"));
        s.push_str(&format!("    P50:              {:>10.2} ms\n", self.stats.p50_us / 1000.0));
        s.push_str(&format!("    P90:              {:>10.2} ms\n", self.stats.p90_us / 1000.0));
        s.push_str(&format!("    P95:              {:>10.2} ms\n", self.stats.p95_us / 1000.0));
        s.push_str(&format!("    P99:              {:>10.2} ms\n", self.stats.p99_us / 1000.0));
        s.push_str(&format!("\n"));
        s.push_str(&format!("  Rating: {} ({})\n", self.rating.emoji(), self.rating.as_str()));
        s.push_str(&format!("  ─────────────────────────────────────────────────────────\n"));

        if let Some(ref kf) = self.keyframe_stats {
            s.push_str(&format!("\n  Keyframe decode:    {:>10.2} ms (avg)\n", kf.mean_us / 1000.0));
        }
        if let Some(ref inf) = self.interframe_stats {
            s.push_str(&format!("  Interframe decode:  {:>10.2} ms (avg)\n", inf.mean_us / 1000.0));
        }

        s.push_str(&format!("\n  System: {} ({} cores)\n",
            self.system_info.cpu_name, self.system_info.cpu_cores));
        if let Some(ref gpu) = self.system_info.gpu_name {
            s.push_str(&format!("  GPU:    {}\n", gpu));
        }
        s.push_str(&format!("  Time:   {}\n", self.timestamp));

        s
    }

    /// Export as JSON
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }
}

/// System information for benchmark context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemInfo {
    pub cpu_name: String,
    pub cpu_cores: u32,
    pub cpu_threads: u32,
    pub ram_gb: u32,
    pub os: String,
    pub gpu_name: Option<String>,
    pub gpu_driver: Option<String>,
}

impl SystemInfo {
    pub fn detect() -> Self {
        Self {
            cpu_name: std::env::var("PROCESSOR_IDENTIFIER")
                .unwrap_or_else(|_| "Unknown CPU".to_string()),
            cpu_cores: num_cpus::get_physical() as u32,
            cpu_threads: num_cpus::get() as u32,
            ram_gb: 0, // Would need sys-info crate
            os: std::env::consts::OS.to_string(),
            gpu_name: None, // Filled by GPU detection
            gpu_driver: None,
        }
    }
}

// ============================================================================
// Benchmark Runner
// ============================================================================

/// Benchmark configuration
#[derive(Debug, Clone)]
pub struct BenchmarkConfig {
    pub name: String,
    pub warmup_frames: u32,
    pub test_frames: u32,
    pub target_fps: f64,
    pub codec: String,
    pub width: u32,
    pub height: u32,
}

impl Default for BenchmarkConfig {
    fn default() -> Self {
        Self {
            name: "Decode Benchmark".to_string(),
            warmup_frames: 30,
            test_frames: 300,
            target_fps: 30.0,
            codec: "H264".to_string(),
            width: 1920,
            height: 1080,
        }
    }
}

/// Benchmark runner
pub struct Benchmarker {
    config: BenchmarkConfig,
    timings: Vec<FrameTiming>,
    warmup_done: bool,
    frame_count: u64,
    start_time: Option<Instant>,
}

impl Benchmarker {
    pub fn new(config: BenchmarkConfig) -> Self {
        Self {
            config,
            timings: Vec::with_capacity(1000),
            warmup_done: false,
            frame_count: 0,
            start_time: None,
        }
    }

    /// Start the benchmark timer
    pub fn start(&mut self) {
        self.start_time = Some(Instant::now());
        self.timings.clear();
        self.frame_count = 0;
        self.warmup_done = false;
    }

    /// Record a frame decode timing
    pub fn record_frame(&mut self, decode_time: Duration, size_bytes: usize, is_keyframe: bool) {
        self.frame_count += 1;

        // Skip warmup frames
        if self.frame_count <= self.config.warmup_frames as u64 {
            if self.frame_count == self.config.warmup_frames as u64 {
                self.warmup_done = true;
                tracing::debug!("Warmup complete, starting measurement");
            }
            return;
        }

        self.timings.push(FrameTiming {
            frame_number: self.frame_count - self.config.warmup_frames as u64,
            decode_time_us: decode_time.as_micros() as u64,
            size_bytes,
            is_keyframe,
        });
    }

    /// Check if benchmark is complete
    pub fn is_complete(&self) -> bool {
        self.timings.len() >= self.config.test_frames as usize
    }

    /// Get current progress (0.0 to 1.0)
    pub fn progress(&self) -> f64 {
        if self.frame_count <= self.config.warmup_frames as u64 {
            0.0
        } else {
            (self.timings.len() as f64 / self.config.test_frames as f64).min(1.0)
        }
    }

    /// Finish benchmark and generate results
    pub fn finish(&self, decoder_name: &str) -> BenchmarkResult {
        let stats = TimingStats::from_timings(&self.timings);

        // Separate keyframe and interframe stats
        let keyframes: Vec<_> = self.timings.iter().filter(|t| t.is_keyframe).cloned().collect();
        let interframes: Vec<_> = self.timings.iter().filter(|t| !t.is_keyframe).cloned().collect();

        let keyframe_stats = if !keyframes.is_empty() {
            Some(TimingStats::from_timings(&keyframes))
        } else {
            None
        };

        let interframe_stats = if !interframes.is_empty() {
            Some(TimingStats::from_timings(&interframes))
        } else {
            None
        };

        let rating = Rating::from_fps(stats.fps, self.config.target_fps);

        BenchmarkResult {
            name: self.config.name.clone(),
            decoder: decoder_name.to_string(),
            codec: self.config.codec.clone(),
            resolution: format!("{}x{}", self.config.width, self.config.height),
            target_fps: self.config.target_fps,
            stats,
            rating,
            timestamp: chrono_lite_timestamp(),
            system_info: SystemInfo::detect(),
            keyframe_stats,
            interframe_stats,
        }
    }
}

// Simple timestamp without chrono dependency
fn chrono_lite_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = duration.as_secs();
    // Convert to readable format (basic)
    let days = secs / 86400;
    let years = 1970 + days / 365;
    let remaining_days = days % 365;
    let months = remaining_days / 30 + 1;
    let day = remaining_days % 30 + 1;
    let hours = (secs % 86400) / 3600;
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;
    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        years, months, day, hours, minutes, seconds)
}

// ============================================================================
// Synthetic Test Data Generation
// ============================================================================

/// Generate synthetic H.264 NAL units for testing
pub struct SyntheticH264 {
    width: u32,
    height: u32,
    frame_num: u32,
}

impl SyntheticH264 {
    pub fn new(width: u32, height: u32) -> Self {
        Self { width, height, frame_num: 0 }
    }

    /// Generate a synthetic SPS NAL unit
    pub fn generate_sps(&self) -> Vec<u8> {
        // Minimal valid SPS for testing decoder initialization
        let mut nal = vec![0x00, 0x00, 0x00, 0x01, 0x67]; // NAL header + SPS type

        // Profile IDC (baseline)
        nal.push(0x42);
        // Constraint flags
        nal.push(0x00);
        // Level IDC (3.0)
        nal.push(0x1E);
        // SPS ID
        nal.push(0xE0);

        // Add encoded width/height (simplified)
        let width_mbs = (self.width + 15) / 16;
        let height_mbs = (self.height + 15) / 16;
        nal.extend_from_slice(&width_mbs.to_be_bytes()[2..]);
        nal.extend_from_slice(&height_mbs.to_be_bytes()[2..]);

        nal
    }

    /// Generate a synthetic PPS NAL unit
    pub fn generate_pps(&self) -> Vec<u8> {
        // Minimal valid PPS
        vec![0x00, 0x00, 0x00, 0x01, 0x68, 0xCE, 0x3C, 0x80]
    }

    /// Generate a synthetic IDR (keyframe) slice
    pub fn generate_idr(&mut self) -> Vec<u8> {
        self.frame_num = 0;
        let mut nal = vec![0x00, 0x00, 0x00, 0x01, 0x65]; // IDR slice

        // Generate random-ish data to simulate compressed frame
        let frame_size = (self.width * self.height / 8) as usize; // Rough estimate
        for i in 0..frame_size.min(10000) {
            nal.push(((i * 7 + self.frame_num as usize * 13) % 256) as u8);
        }

        self.frame_num += 1;
        nal
    }

    /// Generate a synthetic P-frame slice
    pub fn generate_p_frame(&mut self) -> Vec<u8> {
        let mut nal = vec![0x00, 0x00, 0x00, 0x01, 0x41]; // Non-IDR slice

        // Smaller than IDR (P-frames are typically smaller)
        let frame_size = (self.width * self.height / 32) as usize;
        for i in 0..frame_size.min(5000) {
            nal.push(((i * 11 + self.frame_num as usize * 17) % 256) as u8);
        }

        self.frame_num += 1;
        nal
    }

    /// Generate a complete test sequence
    pub fn generate_sequence(&mut self, num_frames: u32, gop_size: u32) -> Vec<Vec<u8>> {
        let mut frames = Vec::with_capacity(num_frames as usize + 2);

        // Start with SPS and PPS
        frames.push(self.generate_sps());
        frames.push(self.generate_pps());

        for i in 0..num_frames {
            if i % gop_size == 0 {
                frames.push(self.generate_idr());
            } else {
                frames.push(self.generate_p_frame());
            }
        }

        frames
    }
}

// ============================================================================
// Preset Benchmarks
// ============================================================================

/// Standard benchmark presets
#[derive(Debug, Clone, Copy)]
pub enum BenchmarkPreset {
    Quick,      // 100 frames, fast results
    Standard,   // 300 frames, balanced
    Thorough,   // 1000 frames, accurate
    Stress,     // 3000 frames, stress test
}

impl BenchmarkPreset {
    pub fn config(&self, codec: &str, width: u32, height: u32) -> BenchmarkConfig {
        let (warmup, test, name) = match self {
            BenchmarkPreset::Quick => (10, 100, "Quick Test"),
            BenchmarkPreset::Standard => (30, 300, "Standard Benchmark"),
            BenchmarkPreset::Thorough => (60, 1000, "Thorough Benchmark"),
            BenchmarkPreset::Stress => (100, 3000, "Stress Test"),
        };

        BenchmarkConfig {
            name: name.to_string(),
            warmup_frames: warmup,
            test_frames: test,
            target_fps: if height >= 2160 { 60.0 } else if height >= 1080 { 30.0 } else { 24.0 },
            codec: codec.to_string(),
            width,
            height,
        }
    }
}

// ============================================================================
// Comparison Utilities
// ============================================================================

/// Compare two benchmark results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkComparison {
    pub baseline: BenchmarkResult,
    pub comparison: BenchmarkResult,
    pub fps_diff_percent: f64,
    pub median_diff_percent: f64,
    pub p99_diff_percent: f64,
    pub winner: String,
}

impl BenchmarkComparison {
    pub fn compare(baseline: BenchmarkResult, comparison: BenchmarkResult) -> Self {
        let fps_diff = ((comparison.stats.fps - baseline.stats.fps) / baseline.stats.fps) * 100.0;
        let median_diff = ((baseline.stats.median_us - comparison.stats.median_us) / baseline.stats.median_us) * 100.0;
        let p99_diff = ((baseline.stats.p99_us - comparison.stats.p99_us) / baseline.stats.p99_us) * 100.0;

        let winner = if comparison.stats.fps > baseline.stats.fps {
            comparison.decoder.clone()
        } else {
            baseline.decoder.clone()
        };

        Self {
            baseline,
            comparison,
            fps_diff_percent: fps_diff,
            median_diff_percent: median_diff,
            p99_diff_percent: p99_diff,
            winner,
        }
    }

    pub fn report(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!("\n═══════════════════════════════════════════════════════════\n"));
        s.push_str(&format!("  Benchmark Comparison\n"));
        s.push_str(&format!("═══════════════════════════════════════════════════════════\n\n"));

        s.push_str(&format!("  {} vs {}\n\n", self.baseline.decoder, self.comparison.decoder));

        s.push_str(&format!("                    {:>15} {:>15}\n",
            self.baseline.decoder, self.comparison.decoder));
        s.push_str(&format!("  ─────────────────────────────────────────────────────────\n"));
        s.push_str(&format!("  FPS:              {:>15.1} {:>15.1}\n",
            self.baseline.stats.fps, self.comparison.stats.fps));
        s.push_str(&format!("  Median (ms):      {:>15.2} {:>15.2}\n",
            self.baseline.stats.median_us / 1000.0, self.comparison.stats.median_us / 1000.0));
        s.push_str(&format!("  P99 (ms):         {:>15.2} {:>15.2}\n",
            self.baseline.stats.p99_us / 1000.0, self.comparison.stats.p99_us / 1000.0));

        s.push_str(&format!("\n  Difference:\n"));
        s.push_str(&format!("  ─────────────────────────────────────────────────────────\n"));
        s.push_str(&format!("  FPS:              {:>+.1}%\n", self.fps_diff_percent));
        s.push_str(&format!("  Median:           {:>+.1}% faster\n", self.median_diff_percent));
        s.push_str(&format!("  P99:              {:>+.1}% faster\n", self.p99_diff_percent));

        s.push_str(&format!("\n  Winner: {} 🏆\n", self.winner));

        s
    }
}

// ============================================================================
// Benchmark Suite
// ============================================================================

/// Run a complete benchmark suite across multiple decoders
pub struct BenchmarkSuite {
    results: Vec<BenchmarkResult>,
}

impl BenchmarkSuite {
    pub fn new() -> Self {
        Self { results: Vec::new() }
    }

    pub fn add_result(&mut self, result: BenchmarkResult) {
        self.results.push(result);
    }

    pub fn results(&self) -> &[BenchmarkResult] {
        &self.results
    }

    /// Generate full suite report
    pub fn report(&self) -> String {
        let mut s = String::new();

        s.push_str(&format!("\n"));
        s.push_str(&format!("╔═══════════════════════════════════════════════════════════╗\n"));
        s.push_str(&format!("║          SLAIN Decoder Benchmark Suite                    ║\n"));
        s.push_str(&format!("╚═══════════════════════════════════════════════════════════╝\n\n"));

        // Summary table
        s.push_str(&format!("  Summary:\n"));
        s.push_str(&format!("  ─────────────────────────────────────────────────────────────\n"));
        s.push_str(&format!("  {:20} {:>10} {:>12} {:>10} {:>8}\n",
            "Decoder", "FPS", "Median (ms)", "P99 (ms)", "Rating"));
        s.push_str(&format!("  ─────────────────────────────────────────────────────────────\n"));

        for r in &self.results {
            s.push_str(&format!("  {:20} {:>10.1} {:>12.2} {:>10.2} {:>8}\n",
                r.decoder,
                r.stats.fps,
                r.stats.median_us / 1000.0,
                r.stats.p99_us / 1000.0,
                r.rating.as_str()));
        }

        // Find best performer
        if let Some(best) = self.results.iter().max_by(|a, b| {
            a.stats.fps.partial_cmp(&b.stats.fps).unwrap()
        }) {
            s.push_str(&format!("\n  🏆 Best: {} ({:.1} FPS)\n", best.decoder, best.stats.fps));
        }

        s
    }

    /// Export suite to JSON
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(&self.results).unwrap_or_default()
    }
}

// ============================================================================
// CPU Core Count (inline implementation to avoid extra dependency)
// ============================================================================

mod num_cpus {
    pub fn get() -> usize {
        std::thread::available_parallelism()
            .map(|p| p.get())
            .unwrap_or(1)
    }

    pub fn get_physical() -> usize {
        // Approximate - assumes SMT with 2 threads per core
        (get() + 1) / 2
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timing_stats() {
        let timings = vec![
            FrameTiming { frame_number: 1, decode_time_us: 1000, size_bytes: 1000, is_keyframe: true },
            FrameTiming { frame_number: 2, decode_time_us: 500, size_bytes: 500, is_keyframe: false },
            FrameTiming { frame_number: 3, decode_time_us: 750, size_bytes: 600, is_keyframe: false },
            FrameTiming { frame_number: 4, decode_time_us: 600, size_bytes: 550, is_keyframe: false },
            FrameTiming { frame_number: 5, decode_time_us: 800, size_bytes: 700, is_keyframe: false },
        ];

        let stats = TimingStats::from_timings(&timings);
        assert_eq!(stats.count, 5);
        assert_eq!(stats.min_us, 500);
        assert_eq!(stats.max_us, 1000);
        assert!(stats.fps > 0.0);
    }

    #[test]
    fn test_rating() {
        assert_eq!(Rating::from_fps(180.0, 60.0), Rating::Excellent);
        assert_eq!(Rating::from_fps(120.0, 60.0), Rating::Good);
        assert_eq!(Rating::from_fps(70.0, 60.0), Rating::Acceptable);
        assert_eq!(Rating::from_fps(40.0, 60.0), Rating::Poor);
    }

    #[test]
    fn test_benchmarker() {
        let config = BenchmarkConfig {
            warmup_frames: 2,
            test_frames: 5,
            ..Default::default()
        };
        let mut bench = Benchmarker::new(config);
        bench.start();

        // Record warmup frames
        bench.record_frame(Duration::from_micros(1000), 1000, true);
        bench.record_frame(Duration::from_micros(500), 500, false);

        // Record test frames
        for i in 0..5 {
            bench.record_frame(Duration::from_micros(600 + i * 100), 500, i == 0);
        }

        assert!(bench.is_complete());
        let result = bench.finish("TestDecoder");
        assert_eq!(result.stats.count, 5);
    }

    #[test]
    fn test_synthetic_h264() {
        let mut gen = SyntheticH264::new(1920, 1080);
        let frames = gen.generate_sequence(10, 5);

        assert_eq!(frames.len(), 12); // SPS + PPS + 10 frames
        assert!(frames[0][4] == 0x67); // SPS
        assert!(frames[1][4] == 0x68); // PPS
        assert!(frames[2][4] == 0x65); // IDR
        assert!(frames[3][4] == 0x41); // P-frame
    }
}
