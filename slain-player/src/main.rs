//! # SLAIN Video Player
//!
//! Pure Rust GPU-accelerated video player using slain-core modules.

use anyhow::Result;
use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::collections::VecDeque;
use std::thread;
use std::time::{Duration, Instant};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::io::{Read, ErrorKind};
use serde::Deserialize;
use parking_lot::Mutex;

// Import from our core library - NOT rewriting
use slain_core::avi_demux::AviDemuxer;
use slain_core::mkv::{MkvParser, MkvInfo, MkvTrack, MkvDemuxer};
use slain_core::mp4_demux::mp4::Mp4Demuxer;
use slain_core::ts_demux::{TsDemuxer, StreamCodec as TsStreamCodec};
use slain_core::audio::{AudioPlayer, audio_set_volume};
use slain_core::hw_decode::{find_best_decoder, available_decoders, HwCodec, HwDecoder, HwDecoderType, DecoderConfig};
use slain_core::pixel_convert::{PixelConverter, VideoFrame as PxVideoFrame, PixelFormat as PxFormat, ColorSpace};
use slain_core::bandwidth::window_monitor;
use slain_core::pipeline::{PipelineKind, PipelineManager};

// ============================================================================
// Playback State Machine
// ============================================================================

#[derive(Debug, Clone, PartialEq)]
enum PlaybackState {
    /// No file loaded, app just started
    Idle,
    /// File is being loaded/parsed
    Loading,
    /// Ready to play (file loaded, decoder ready)
    Ready,
    /// Actively playing
    Playing,
    /// Paused
    Paused,
    /// Error occurred
    Error(String),
}

// ============================================================================
// Shared Playback State (between decode thread and UI)
// ============================================================================

struct PlaybackShared {
    is_playing: AtomicBool,
    should_stop: AtomicBool,
    current_time_ms: AtomicU64,
    seek_requested: AtomicBool,
    seek_target_ms: AtomicU64,
    frame_queue: Mutex<VecDeque<RgbFrame>>,
}

impl PlaybackShared {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            is_playing: AtomicBool::new(false),
            should_stop: AtomicBool::new(false),
            current_time_ms: AtomicU64::new(0),
            seek_requested: AtomicBool::new(false),
            seek_target_ms: AtomicU64::new(0),
            frame_queue: Mutex::new(VecDeque::with_capacity(8)),
        })
    }
}

/// RGB frame ready for display
struct RgbFrame {
    data: Vec<u8>,  // RGB24
    width: u32,
    height: u32,
    pts_ms: u64,
}

#[derive(Clone, Copy)]
struct AppOptions {
    use_ffmpeg: bool,
}

impl AppOptions {
    fn from_args(args: &[String]) -> Self {
        let use_ffmpeg = args.iter().any(|arg| arg == "--ffmpeg");
        Self { use_ffmpeg }
    }
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let app_options = AppOptions::from_args(&args);
    let headless_requested = args
        .get(1)
        .map(|arg| arg == "--headless" || arg == "headless")
        .unwrap_or(false);

    if headless_requested {
        tracing_subscriber::fmt()
            .with_env_filter("slain=info,wgpu=warn,eframe=warn")
            .init();
        return run_headless(&args);
    }

    tracing_subscriber::fmt()
        .with_env_filter("slain=debug,wgpu=warn,eframe=warn")
        .init();

    tracing::info!("SLAIN Player v{}", env!("CARGO_PKG_VERSION"));

    // Log available decoders
    let decoders = available_decoders();
    tracing::info!("Available decoders: {:?}", decoders);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("SLAIN Player")
            .with_inner_size([1280.0, 720.0])
            .with_min_inner_size([640.0, 360.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };

    eframe::run_native(
        "SLAIN Player",
        options,
        Box::new(|cc| Ok(Box::new(SlainApp::new(cc, app_options)))),
    )
    .map_err(|e| anyhow::anyhow!("eframe error: {}", e))?;

    Ok(())
}

// ============================================================================
// Application State
// ============================================================================

struct SlainApp {
    // Playback state machine
    playback_state: PlaybackState,
    
    // Media state
    media_info: Option<MkvInfo>,
    video_path: Option<PathBuf>,
    
    // Shared state with decode thread
    shared: Arc<PlaybackShared>,
    decode_thread: Option<thread::JoinHandle<()>>,
    
    // Playback (local UI state)
    current_time_ms: u64,
    duration_ms: u64,
    volume: f32,
    
    // Audio player from slain-core
    audio_player: Option<AudioPlayer>,
    audio_started: bool,
    
    // Pipeline selection
    pipeline: PipelineKind,
    pipeline_manager: Option<PipelineManager>,

    // Decoder preference
    preferred_decoder: Option<HwDecoderType>,

    // Playback backend
    use_ffmpeg: bool,
    
    // Display
    video_texture: Option<TextureHandle>,
    frame_width: u32,
    frame_height: u32,
    last_frame_time: Instant,

    // Frame pacing
    playback_start_time: Option<Instant>,
    last_displayed_pts: u64,
    
    // UI state
    show_osd: bool,
    is_fullscreen: bool,
    #[allow(dead_code)]
    show_settings: bool,
    
    // Stats
    fps: f32,
    frame_count: u64,
    dropped_frames: u32,
    decoder_name: String,
}

impl SlainApp {
    fn new(cc: &eframe::CreationContext<'_>, options: AppOptions) -> Self {
        // Set up dark theme with modern styling
        let mut visuals = egui::Visuals::dark();

        // Darker background for video player
        visuals.panel_fill = egui::Color32::from_rgb(18, 18, 18);
        visuals.window_fill = egui::Color32::from_rgb(24, 24, 24);
        visuals.extreme_bg_color = egui::Color32::from_rgb(10, 10, 10);

        // Accent colors - subtle blue
        visuals.selection.bg_fill = egui::Color32::from_rgb(50, 100, 150);
        visuals.hyperlink_color = egui::Color32::from_rgb(100, 160, 220);

        // Widgets - rounded, subtle
        visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(35, 35, 35);
        visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(45, 45, 45);
        visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(60, 60, 65);
        visuals.widgets.active.bg_fill = egui::Color32::from_rgb(70, 100, 130);

        // Reduce corner rounding for modern look
        visuals.widgets.noninteractive.rounding = egui::Rounding::same(4.0);
        visuals.widgets.inactive.rounding = egui::Rounding::same(4.0);
        visuals.widgets.hovered.rounding = egui::Rounding::same(4.0);
        visuals.widgets.active.rounding = egui::Rounding::same(4.0);

        // Window rounding
        visuals.window_rounding = egui::Rounding::same(8.0);

        cc.egui_ctx.set_visuals(visuals);

        // Set up fonts with slightly larger text
        let mut style = (*cc.egui_ctx.style()).clone();
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(8.0, 4.0);
        cc.egui_ctx.set_style(style);

        Self {
            playback_state: PlaybackState::Idle,
            media_info: None,
            video_path: None,
            shared: PlaybackShared::new(),
            decode_thread: None,
            current_time_ms: 0,
            duration_ms: 0,
            volume: 1.0,
            audio_player: None, // Lazy init on file load
            audio_started: false,
            pipeline: PipelineKind::SoftwareOnly,
            pipeline_manager: None, // Lazy init on file load
            preferred_decoder: None, // Auto-detect best decoder
            use_ffmpeg: options.use_ffmpeg,
            video_texture: None,
            frame_width: 1920,
            frame_height: 1080,
            last_frame_time: Instant::now(),
            playback_start_time: None,
            last_displayed_pts: 0,
            show_osd: true,
            is_fullscreen: false,
            show_settings: false,
            fps: 0.0,
            frame_count: 0,
            dropped_frames: 0,
            decoder_name: "None".to_string(),
        }
    }
    
    /// Check if we're in a playable state
    fn is_playing(&self) -> bool {
        self.playback_state == PlaybackState::Playing
    }
    
    /// Check if file is loaded and ready
    fn is_ready(&self) -> bool {
        matches!(self.playback_state, PlaybackState::Ready | PlaybackState::Playing | PlaybackState::Paused)
    }

    fn start_audio_if_needed(&mut self) {
        if self.audio_started {
            return;
        }
        let Some(path) = self.video_path.as_ref() else {
            return;
        };

        if self.audio_player.is_none() {
            self.audio_player = Some(AudioPlayer::new());
        }
        if let Some(ref mut player) = self.audio_player {
            if let Err(e) = player.play_file(path) {
                tracing::warn!("Audio failed: {}", e);
                return;
            }
        }
        self.audio_started = true;
    }
    
    /// Open a media file using slain-core parsers
    fn open_file(&mut self, path: PathBuf) {
        tracing::info!("Opening: {:?}", path);

        // Reset playback state
        self.playback_start_time = None;
        self.last_displayed_pts = 0;
        self.current_time_ms = 0;
        self.audio_started = false;

        // Determine file type by extension
        let ext = path.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        
        match ext.as_str() {
            "mkv" | "webm" => self.open_mkv(&path),
            "mp4" | "m4v" | "mov" => self.open_mp4(&path),
            "avi" => self.open_avi(&path),
            "ts" | "mts" | "m2ts" => self.open_ts(&path),
            _ => {
                tracing::warn!("Unknown format: {}", ext);
                // Try MKV parser as fallback
                self.open_mkv(&path);
            }
        }
        
        self.video_path = Some(path);
    }
    
    fn open_mkv(&mut self, path: &PathBuf) {
        self.playback_state = PlaybackState::Loading;
        
        let mut parser = MkvParser::new();
        match parser.parse(path) {
            Ok(info) => {
                tracing::info!("MKV parsed: {} tracks, {} ms", 
                    info.tracks.len(), info.duration_ms);
                
                self.duration_ms = info.duration_ms;
                
                // Find video track for dimensions
                for track in &info.tracks {
                    if let MkvTrack::Video(v) = track {
                        self.frame_width = v.pixel_width;
                        self.frame_height = v.pixel_height;
                        tracing::info!("Video: {}x{} @ {} fps",
                            v.pixel_width, v.pixel_height, v.frame_rate.unwrap_or(0.0));
                        
                        // Find best decoder for codec
                        if let Ok(codec) = mkv_codec_to_hwcodec(&v.codec_id) {
                            if let Some(dec) = find_best_decoder(codec) {
                                self.decoder_name = format!("{:?}", dec);
                                tracing::info!("Using decoder: {:?}", dec);
                            }
                        }
                    }
                }
                
                self.media_info = Some(info);
                
                // Stop any existing decode thread
                self.stop_decode_thread();
                
                // Start decode thread
                let shared = self.shared.clone();
                let video_path = path.clone();
                let width = self.frame_width;
                let height = self.frame_height;
                let use_ffmpeg = self.use_ffmpeg;
                
                self.decode_thread = Some(thread::spawn(move || {
                    decode_loop(shared, video_path, width, height, use_ffmpeg);
                }));
                
                self.shared.is_playing.store(true, Ordering::SeqCst);
                self.playback_state = PlaybackState::Playing;
                self.playback_start_time = Some(Instant::now());
                window_monitor().set_playing(true);
                self.start_audio_if_needed();
            }
            Err(e) => {
                tracing::error!("MKV parse error: {}", e);
                self.playback_state = PlaybackState::Error(e.to_string());
            }
        }
    }
    
    fn stop_decode_thread(&mut self) {
        self.shared.should_stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.decode_thread.take() {
            let _ = handle.join();
        }
        self.shared.should_stop.store(false, Ordering::SeqCst);
        self.shared.frame_queue.lock().clear();
    }
    
    fn open_mp4(&mut self, path: &PathBuf) {
        self.playback_state = PlaybackState::Loading;
        tracing::info!("Opening MP4: {:?}", path);

        // Parse MP4 to get duration and video info
        use std::fs::File;
        use std::io::BufReader;

        let file = match File::open(path) {
            Ok(f) => f,
            Err(e) => {
                tracing::error!("Failed to open MP4: {}", e);
                self.playback_state = PlaybackState::Error(e.to_string());
                return;
            }
        };
        let reader = BufReader::new(file);

        match Mp4Demuxer::new(reader) {
            Ok(demuxer) => {
                // Get duration
                self.duration_ms = (demuxer.duration_us() / 1000) as u64;

                // Find video track for dimensions
                for (idx, stream) in demuxer.streams().iter().enumerate() {
                    if matches!(stream.codec_type, slain_core::mp4_demux::CodecType::Video) {
                        if let Some(vi) = demuxer.video_info(idx) {
                            self.frame_width = vi.width;
                            self.frame_height = vi.height;
                            tracing::info!("MP4 Video: {}x{}, duration: {}ms",
                                vi.width, vi.height, self.duration_ms);
                        }
                        break;
                    }
                }

                self.video_path = Some(path.clone());

                // Stop any existing decode thread
                self.stop_decode_thread();

                // Start decode thread
                let shared = self.shared.clone();
                let video_path = path.clone();
                let width = self.frame_width;
                let height = self.frame_height;
                let use_ffmpeg = self.use_ffmpeg;

                self.decode_thread = Some(thread::spawn(move || {
                    decode_loop(shared, video_path, width, height, use_ffmpeg);
                }));

                self.shared.is_playing.store(true, Ordering::SeqCst);
                self.playback_state = PlaybackState::Playing;
                self.playback_start_time = Some(Instant::now());
                window_monitor().set_playing(true);
                self.start_audio_if_needed();
            }
            Err(e) => {
                tracing::error!("MP4 parse error: {}", e);
                self.playback_state = PlaybackState::Error(e.to_string());
            }
        }
    }
    
    fn open_avi(&mut self, path: &PathBuf) {
        self.playback_state = PlaybackState::Loading;
        
        tracing::info!("Opening AVI: {:?}", path);
        use std::fs::File;
        use std::io::BufReader;

        let file = match File::open(path) {
            Ok(f) => f,
            Err(e) => {
                tracing::error!("Failed to open AVI: {}", e);
                self.playback_state = PlaybackState::Error(e.to_string());
                return;
            }
        };
        let reader = BufReader::new(file);

        match AviDemuxer::new(reader) {
            Ok(demuxer) => {
                let info = demuxer.info();
                self.duration_ms = (info.duration_us / 1000) as u64;
                self.frame_width = info.width;
                self.frame_height = info.height;
                tracing::info!(
                    "AVI Video: {}x{}, duration: {}ms",
                    info.width,
                    info.height,
                    self.duration_ms
                );

                self.video_path = Some(path.clone());

                self.stop_decode_thread();
                let shared = self.shared.clone();
                let video_path = path.clone();
                let width = self.frame_width;
                let height = self.frame_height;
                let use_ffmpeg = self.use_ffmpeg;

                self.decode_thread = Some(thread::spawn(move || {
                    decode_loop(shared, video_path, width, height, use_ffmpeg);
                }));

                self.shared.is_playing.store(true, Ordering::SeqCst);
                self.playback_state = PlaybackState::Playing;
                self.playback_start_time = Some(Instant::now());
                window_monitor().set_playing(true);
                self.start_audio_if_needed();
            }
            Err(e) => {
                tracing::error!("AVI parse error: {}", e);
                self.playback_state = PlaybackState::Error(e.to_string());
            }
        }
    }
    
    fn open_ts(&mut self, path: &PathBuf) {
        self.playback_state = PlaybackState::Loading;
        
        tracing::info!("Opening TS: {:?}", path);
        use std::fs::File;
        use std::io::BufReader;

        let file = match File::open(path) {
            Ok(f) => f,
            Err(e) => {
                tracing::error!("Failed to open TS: {}", e);
                self.playback_state = PlaybackState::Error(e.to_string());
                return;
            }
        };
        let reader = BufReader::new(file);

        match TsDemuxer::new(reader) {
            Ok(demuxer) => {
                let info = demuxer.info();
                let video_stream = info
                    .streams
                    .iter()
                    .find(|stream| matches!(stream.codec, TsStreamCodec::H264
                        | TsStreamCodec::H265
                        | TsStreamCodec::MPEG2Video
                        | TsStreamCodec::MPEG1Video));
                if video_stream.is_none() {
                    tracing::error!("No supported video stream found in TS");
                    self.playback_state = PlaybackState::Error("No video stream found".into());
                    return;
                }

                self.video_path = Some(path.clone());

                self.stop_decode_thread();
                let shared = self.shared.clone();
                let video_path = path.clone();
                let width = self.frame_width;
                let height = self.frame_height;
                let use_ffmpeg = self.use_ffmpeg;

                self.decode_thread = Some(thread::spawn(move || {
                    decode_loop(shared, video_path, width, height, use_ffmpeg);
                }));

                self.shared.is_playing.store(true, Ordering::SeqCst);
                self.playback_state = PlaybackState::Playing;
                self.playback_start_time = Some(Instant::now());
                window_monitor().set_playing(true);
                self.start_audio_if_needed();
            }
            Err(e) => {
                tracing::error!("TS parse error: {}", e);
                self.playback_state = PlaybackState::Error(e.to_string());
            }
        }
    }
    
    fn toggle_play(&mut self) {
        // Can only toggle if we have a file loaded
        if !self.is_ready() {
            tracing::warn!("Cannot play: no file loaded");
            return;
        }
        
        // Toggle between Playing and Paused
        match self.playback_state {
            PlaybackState::Playing => {
                self.playback_state = PlaybackState::Paused;
                self.shared.is_playing.store(false, Ordering::SeqCst);
                window_monitor().set_playing(false);
            }
            PlaybackState::Paused | PlaybackState::Ready => {
                self.playback_state = PlaybackState::Playing;
                self.shared.is_playing.store(true, Ordering::SeqCst);
                window_monitor().set_playing(true);
                self.start_audio_if_needed();
            }
            _ => {
                // Can't toggle in other states
            }
        }
    }
    
    fn seek(&mut self, time_ms: u64) {
        self.current_time_ms = time_ms.min(self.duration_ms);

        // Request seek in decode thread
        self.shared.seek_target_ms.store(self.current_time_ms, Ordering::SeqCst);
        self.shared.seek_requested.store(true, Ordering::SeqCst);

        // Reset playback timer to sync with new position
        if self.is_playing() {
            self.playback_start_time = Some(Instant::now() - Duration::from_millis(time_ms));
        }

        // TODO: Seek in audio
    }
    
    fn set_volume(&mut self, vol: f32) {
        self.volume = vol.clamp(0.0, 1.0);
        let _ = audio_set_volume(self.volume);
    }
    
    fn toggle_fullscreen(&mut self, ctx: &egui::Context) {
        self.is_fullscreen = !self.is_fullscreen;
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.is_fullscreen));
        
        // Update attention state
        window_monitor().set_fullscreen(self.is_fullscreen);
    }
}

// ============================================================================
// UI Rendering
// ============================================================================

impl eframe::App for SlainApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Request continuous repaints when playing
        if self.is_playing() {
            ctx.request_repaint();

            // Initialize playback start time on first play
            if self.playback_start_time.is_none() {
                self.playback_start_time = Some(Instant::now());
                self.last_displayed_pts = 0;
            }

            // Sync state from shared
            self.current_time_ms = self.shared.current_time_ms.load(Ordering::Relaxed);
        } else {
            // Reset playback timer when paused
            self.playback_start_time = None;
        }

        // Pull frame from queue with PTS-based timing
        let should_display = if let Some(start_time) = self.playback_start_time {
            let elapsed_ms = start_time.elapsed().as_millis() as u64;

            // Peek at next frame to check its PTS
            let queue = self.shared.frame_queue.lock();
            if let Some(frame) = queue.front() {
                // Display frame if its PTS has arrived, or if we're behind
                frame.pts_ms <= elapsed_ms + 5 // 5ms tolerance
            } else {
                false
            }
        } else {
            // Not playing, but still show first frame if available
            !self.shared.frame_queue.lock().is_empty()
        };

        if should_display {
            if let Some(frame) = self.shared.frame_queue.lock().pop_front() {
                // Calculate FPS
                let now = Instant::now();
                let delta = now.duration_since(self.last_frame_time);
                if delta.as_secs_f32() > 0.0 {
                    self.fps = 1.0 / delta.as_secs_f32();
                }
                self.last_frame_time = now;
                self.frame_count += 1;
                self.last_displayed_pts = frame.pts_ms;

                // Upload to GPU texture
                let image = ColorImage::from_rgb(
                    [frame.width as usize, frame.height as usize],
                    &frame.data,
                );

                self.video_texture = Some(ctx.load_texture(
                    "video_frame",
                    image,
                    TextureOptions::LINEAR,
                ));

                self.frame_width = frame.width;
                self.frame_height = frame.height;
            }
        }
        
        // Menu bar - subtle, minimal
        egui::TopBottomPanel::top("menu")
            .frame(egui::Frame::none()
                .fill(egui::Color32::from_rgb(25, 25, 28))
                .inner_margin(egui::Margin::symmetric(8.0, 4.0)))
            .show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Open...").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("Video", &["mkv", "mp4", "avi", "webm", "mov", "ts"])
                            .pick_file()
                        {
                            self.open_file(path);
                        }
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Exit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                
                ui.menu_button("View", |ui| {
                    ui.checkbox(&mut self.show_osd, "Show OSD (Tab)");
                    if ui.button("Fullscreen (F)").clicked() {
                        self.toggle_fullscreen(ctx);
                        ui.close_menu();
                    }
                });
                
                ui.menu_button("Pipeline", |ui| {
                    // Lazy init pipeline manager
                    if self.pipeline_manager.is_none() {
                        self.pipeline_manager = Some(PipelineManager::new());
                    }
                    if let Some(ref mut manager) = self.pipeline_manager {
                        let available = manager.available();
                        for p in available {
                            let selected = self.pipeline == p;
                            if ui.radio(selected, format!("{:?}", p)).clicked() {
                                self.pipeline = p;
                                manager.set_active(p);
                                ui.close_menu();
                            }
                        }
                    }
                });
                
                ui.menu_button("Audio", |ui| {
                    ui.label("Volume:");
                    let old_vol = self.volume;
                    ui.add(egui::Slider::new(&mut self.volume, 0.0..=1.0).show_value(false));
                    if self.volume != old_vol {
                        let _ = audio_set_volume(self.volume);
                    }
                });

                ui.menu_button("Decoder", |ui| {
                    let decoders = available_decoders();

                    // Auto option
                    let is_auto = self.preferred_decoder.is_none();
                    if ui.radio(is_auto, "Auto (best available)").clicked() {
                        self.preferred_decoder = None;
                        ui.close_menu();
                    }

                    ui.separator();

                    // List available decoders
                    for dec in &decoders {
                        let is_selected = self.preferred_decoder == Some(*dec);
                        let label = match dec {
                            HwDecoderType::Nvdec => "NVIDIA NVDEC",
                            HwDecoderType::Amf => "AMD AMF",
                            HwDecoderType::Vaapi => "VA-API (Linux)",
                            HwDecoderType::Software => "Software (CPU)",
                        };
                        if ui.radio(is_selected, label).clicked() {
                            self.preferred_decoder = Some(*dec);
                            ui.close_menu();
                        }
                    }

                    ui.separator();
                    ui.label(format!("Current: {}", self.decoder_name));
                });

                ui.menu_button("Settings", |ui| {
                    ui.checkbox(&mut self.show_osd, "Show OSD overlay");
                    ui.separator();
                    ui.label(format!("Resolution: {}x{}", self.frame_width, self.frame_height));
                    ui.label(format!("FPS: {:.1}", self.fps));
                    ui.label(format!("Frames: {} (dropped: {})", self.frame_count, self.dropped_frames));
                });
            });
        });
        
        // Video area
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(egui::Color32::BLACK))
            .show(ctx, |ui| {
                let rect = ui.available_rect_before_wrap();
                
                // Show video frame or state message
                match &self.playback_state {
                    PlaybackState::Idle => {
                        ui.centered_and_justified(|ui| {
                            ui.heading(
                                egui::RichText::new("Drop a video file here\nor use File → Open")
                                    .color(egui::Color32::GRAY)
                                    .size(24.0)
                            );
                        });
                    }
                    PlaybackState::Loading => {
                        ui.centered_and_justified(|ui| {
                            ui.heading(
                                egui::RichText::new("Loading...")
                                    .color(egui::Color32::GRAY)
                            );
                        });
                    }
                    PlaybackState::Error(msg) => {
                        ui.centered_and_justified(|ui| {
                            ui.heading(
                                egui::RichText::new(format!("Error: {}", msg))
                                    .color(egui::Color32::RED)
                            );
                        });
                    }
                    _ => {
                        // Ready, Playing, or Paused - show video frame
                        if let Some(texture) = &self.video_texture {
                            // Calculate aspect-correct size
                            let aspect = self.frame_width as f32 / self.frame_height as f32;
                            let panel_aspect = rect.width() / rect.height();
                            
                            let (w, h) = if aspect > panel_aspect {
                                (rect.width(), rect.width() / aspect)
                            } else {
                                (rect.height() * aspect, rect.height())
                            };
                            
                            let x = rect.min.x + (rect.width() - w) / 2.0;
                            let y = rect.min.y + (rect.height() - h) / 2.0;
                            
                            let video_rect = egui::Rect::from_min_size(
                                egui::pos2(x, y),
                                egui::vec2(w, h),
                            );
                            
                            ui.painter().image(
                                texture.id(),
                                video_rect,
                                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                                egui::Color32::WHITE,
                            );
                        } else {
                            // Waiting for first frame
                            ui.centered_and_justified(|ui| {
                                ui.heading(
                                    egui::RichText::new("Waiting for frames...")
                                        .color(egui::Color32::GRAY)
                                );
                            });
                        }
                    }
                }
                
                // OSD overlay
                if self.show_osd && self.video_path.is_some() {
                    let osd_rect = egui::Rect::from_min_size(
                        rect.min + egui::vec2(10.0, 10.0),
                        egui::vec2(250.0, 140.0),
                    );

                    #[allow(deprecated)]
                    ui.allocate_ui_at_rect(osd_rect, |ui| {
                        egui::Frame::popup(ui.style())
                            .fill(egui::Color32::from_black_alpha(200))
                            .rounding(8.0)
                            .inner_margin(10.0)
                            .show(ui, |ui| {
                                ui.label(format!("Time: {} / {}", 
                                    format_time(self.current_time_ms),
                                    format_time(self.duration_ms)));
                                ui.label(format!("Resolution: {}x{}", 
                                    self.frame_width, self.frame_height));
                                ui.label(format!("Decoder: {}", self.decoder_name));
                                ui.label(format!("Pipeline: {:?}", self.pipeline));
                                ui.label(format!("FPS: {:.1}", self.fps));
                                ui.label(format!("Volume: {:.0}%", self.volume * 100.0));
                            });
                    });
                }
            });
        
        // Bottom controls - styled like modern video player
        egui::TopBottomPanel::bottom("controls")
            .frame(egui::Frame::none()
                .fill(egui::Color32::from_rgba_unmultiplied(20, 20, 22, 240))
                .inner_margin(egui::Margin::symmetric(16.0, 8.0)))
            .min_height(70.0)
            .show(ctx, |ui| {
                // Progress bar - full width, thin
                let mut time_sec = self.current_time_ms as f64 / 1000.0;
                let duration_sec = self.duration_ms as f64 / 1000.0;

                // Custom progress bar styling
                let progress = if duration_sec > 0.0 { time_sec / duration_sec } else { 0.0 };
                let bar_height = 4.0;
                let bar_rect = ui.available_rect_before_wrap();
                let bar_rect = egui::Rect::from_min_size(
                    bar_rect.min,
                    egui::vec2(bar_rect.width(), bar_height),
                );

                // Background track
                ui.painter().rect_filled(
                    bar_rect,
                    egui::Rounding::same(2.0),
                    egui::Color32::from_rgb(50, 50, 55),
                );

                // Progress fill
                let fill_rect = egui::Rect::from_min_size(
                    bar_rect.min,
                    egui::vec2(bar_rect.width() * progress as f32, bar_height),
                );
                ui.painter().rect_filled(
                    fill_rect,
                    egui::Rounding::same(2.0),
                    egui::Color32::from_rgb(80, 140, 200),
                );

                // Invisible slider for interaction
                let slider = egui::Slider::new(&mut time_sec, 0.0..=duration_sec.max(1.0))
                    .show_value(false);

                let response = ui.add(slider);
                if response.changed() {
                    self.seek((time_sec * 1000.0) as u64);
                }

                ui.add_space(8.0);

                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 12.0;

                    // Play/Pause - larger, prominent
                    let icon = if self.is_playing() { "⏸" } else { "▶" };
                    let btn = egui::Button::new(
                        egui::RichText::new(icon).size(24.0).color(egui::Color32::WHITE)
                    ).fill(egui::Color32::TRANSPARENT);
                    if ui.add(btn).clicked() {
                        self.toggle_play();
                    }

                    // Skip back 10s
                    let btn = egui::Button::new(
                        egui::RichText::new("⏪").size(18.0).color(egui::Color32::from_gray(180))
                    ).fill(egui::Color32::TRANSPARENT);
                    if ui.add(btn).clicked() {
                        let new_time = self.current_time_ms.saturating_sub(10000);
                        self.seek(new_time);
                    }

                    // Skip forward 10s
                    let btn = egui::Button::new(
                        egui::RichText::new("⏩").size(18.0).color(egui::Color32::from_gray(180))
                    ).fill(egui::Color32::TRANSPARENT);
                    if ui.add(btn).clicked() {
                        let new_time = (self.current_time_ms + 10000).min(self.duration_ms);
                        self.seek(new_time);
                    }

                    // Stop
                    let btn = egui::Button::new(
                        egui::RichText::new("⏹").size(18.0).color(egui::Color32::from_gray(180))
                    ).fill(egui::Color32::TRANSPARENT);
                    if ui.add(btn).clicked() {
                        self.playback_state = PlaybackState::Ready;
                        self.shared.is_playing.store(false, Ordering::SeqCst);
                        self.current_time_ms = 0;
                    }
                    
                    // Time display - styled
                    ui.label(
                        egui::RichText::new(format!(
                            "{} / {}",
                            format_time(self.current_time_ms),
                            format_time(self.duration_ms)
                        ))
                        .size(13.0)
                        .color(egui::Color32::from_gray(200))
                    );

                    // Right side controls
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;

                        // Fullscreen
                        let btn = egui::Button::new(
                            egui::RichText::new("⛶").size(18.0).color(egui::Color32::from_gray(180))
                        ).fill(egui::Color32::TRANSPARENT);
                        if ui.add(btn).clicked() {
                            self.toggle_fullscreen(ctx);
                        }

                        // Volume slider - compact
                        let old_vol = self.volume;
                        ui.add(
                            egui::Slider::new(&mut self.volume, 0.0..=1.0)
                                .show_value(false)
                        );
                        if self.volume != old_vol {
                            let _ = audio_set_volume(self.volume);
                        }

                        // Volume icon - clickable to mute
                        let vol_icon = if self.volume == 0.0 { "🔇" }
                            else if self.volume < 0.5 { "🔉" }
                            else { "🔊" };
                        let btn = egui::Button::new(
                            egui::RichText::new(vol_icon).size(16.0)
                        ).fill(egui::Color32::TRANSPARENT);
                        if ui.add(btn).clicked() {
                            // Toggle mute
                            if self.volume > 0.0 {
                                self.volume = 0.0;
                            } else {
                                self.volume = 1.0;
                            }
                            let _ = audio_set_volume(self.volume);
                        }
                    });
                });
            });
        
        // Handle drag & drop
        ctx.input(|i| {
            for file in &i.raw.dropped_files {
                if let Some(path) = &file.path {
                    self.open_file(path.clone());
                }
            }
        });
        
        // Keyboard shortcuts
        ctx.input(|i| {
            if i.key_pressed(egui::Key::Space) {
                self.toggle_play();
            }
            if i.key_pressed(egui::Key::Tab) {
                self.show_osd = !self.show_osd;
            }
            if i.key_pressed(egui::Key::F) || 
               (i.key_pressed(egui::Key::Enter) && i.modifiers.alt) {
                self.toggle_fullscreen(ctx);
            }
            if i.key_pressed(egui::Key::Escape) && self.is_fullscreen {
                self.toggle_fullscreen(ctx);
            }
            // Arrow keys for seeking
            if i.key_pressed(egui::Key::ArrowRight) {
                self.seek(self.current_time_ms.saturating_add(5000));
            }
            if i.key_pressed(egui::Key::ArrowLeft) {
                self.seek(self.current_time_ms.saturating_sub(5000));
            }
            // Volume
            if i.key_pressed(egui::Key::ArrowUp) {
                self.set_volume(self.volume + 0.05);
            }
            if i.key_pressed(egui::Key::ArrowDown) {
                self.set_volume(self.volume - 0.05);
            }
        });
        
        // Update window focus state for bandwidth optimization
        ctx.input(|i| {
            window_monitor().set_focused(i.focused);
        });
    }
}

// ============================================================================
// Helpers
// ============================================================================

fn format_time(ms: u64) -> String {
    let total_secs = ms / 1000;
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;
    
    if hours > 0 {
        format!("{:02}:{:02}:{:02}", hours, mins, secs)
    } else {
        format!("{:02}:{:02}", mins, secs)
    }
}

// ============================================================================
// Headless Playback
// ============================================================================

fn mp4_codec_to_hwcodec(codec: &slain_core::mp4_demux::CodecId) -> Result<HwCodec, String> {
    match codec {
        slain_core::mp4_demux::CodecId::Video(codec) => match codec {
            slain_core::mp4_demux::VideoCodec::H264 => Ok(HwCodec::H264),
            slain_core::mp4_demux::VideoCodec::H265 => Ok(HwCodec::H265),
            slain_core::mp4_demux::VideoCodec::VP8 => Ok(HwCodec::VP8),
            slain_core::mp4_demux::VideoCodec::VP9 => Ok(HwCodec::VP9),
            slain_core::mp4_demux::VideoCodec::AV1 => Ok(HwCodec::AV1),
            slain_core::mp4_demux::VideoCodec::MPEG2 => Ok(HwCodec::MPEG2),
            slain_core::mp4_demux::VideoCodec::VC1 => Ok(HwCodec::VC1),
            other => Err(format!("Unsupported MP4 codec: {:?}", other)),
        },
        other => Err(format!("Unsupported MP4 stream: {:?}", other)),
    }
}

fn mkv_codec_to_hwcodec(codec_id: &str) -> Result<HwCodec, String> {
    match codec_id {
        "V_MPEG4/ISO/AVC" => Ok(HwCodec::H264),
        "V_MPEGH/ISO/HEVC" => Ok(HwCodec::H265),
        "V_VP8" => Ok(HwCodec::VP8),
        "V_VP9" => Ok(HwCodec::VP9),
        "V_AV1" => Ok(HwCodec::AV1),
        "V_MPEG2" => Ok(HwCodec::MPEG2),
        "V_MS/VFW/FOURCC" => Ok(HwCodec::VC1),
        other => Err(format!("Unsupported MKV codec: {}", other)),
    }
}

struct HeadlessOptions {
    input: PathBuf,
    frames: u64,
    use_ffmpeg: bool,
}

fn run_headless(args: &[String]) -> Result<()> {
    let options = parse_headless_args(args)?;

    tracing::info!(
        "Headless playback starting: input={:?}, frames={}",
        options.input,
        options.frames
    );

    let ext = options
        .input
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let stats = if options.use_ffmpeg {
        decode_ffmpeg_headless(&options.input, options.frames)
            .map_err(|e| anyhow::anyhow!(e))?
    } else {
        match ext.as_str() {
            "mp4" | "m4v" | "mov" => decode_mp4_headless(&options.input, options.frames)
                .map_err(|e| anyhow::anyhow!(e))?,
            "mkv" | "webm" => decode_mkv_headless(&options.input, options.frames)
                .map_err(|e| anyhow::anyhow!(e))?,
            _ => {
                return Err(anyhow::anyhow!(
                    "Unsupported container for headless playback: {:?}",
                    ext
                ));
            }
        }
    };

    tracing::info!(
        "Headless playback complete: decoded_frames={}, duration_ms={}",
        stats.decoded_frames,
        stats.duration_ms
    );

    Ok(())
}

fn parse_headless_args(args: &[String]) -> Result<HeadlessOptions> {
    let mut input: Option<PathBuf> = None;
    let mut frames: u64 = 120;
    let mut use_ffmpeg = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--headless" | "headless" => {
                i += 1;
            }
            "--input" | "-i" => {
                let value = args.get(i + 1).ok_or_else(|| {
                    anyhow::anyhow!("Missing value for --input")
                })?;
                input = Some(PathBuf::from(value));
                i += 2;
            }
            "--frames" | "-n" => {
                let value = args.get(i + 1).ok_or_else(|| {
                    anyhow::anyhow!("Missing value for --frames")
                })?;
                frames = value.parse::<u64>().map_err(|e| {
                    anyhow::anyhow!("Invalid frame count {}: {}", value, e)
                })?;
                i += 2;
            }
            "--ffmpeg" => {
                use_ffmpeg = true;
                i += 1;
            }
            "--help" | "-h" => {
                print_headless_usage();
                std::process::exit(0);
            }
            _ => {
                i += 1;
            }
        }
    }

    let input = input.ok_or_else(|| {
        print_headless_usage();
        anyhow::anyhow!("Missing required --input for headless playback")
    })?;

    Ok(HeadlessOptions {
        input,
        frames,
        use_ffmpeg,
    })
}

fn print_headless_usage() {
    eprintln!(
        "\nHeadless playback usage:\n  slain --headless --input <file> [--frames <n>] [--ffmpeg]\n"
    );
}

// ============================================================================
// Decode Thread
// ============================================================================

struct HeadlessStats {
    decoded_frames: u64,
    duration_ms: u64,
}

#[derive(Deserialize)]
struct FfmpegProbe {
    streams: Vec<FfmpegStream>,
}

#[derive(Deserialize)]
struct FfmpegStream {
    width: Option<u32>,
    height: Option<u32>,
    avg_frame_rate: Option<String>,
}

struct FfmpegVideoInfo {
    width: u32,
    height: u32,
    frame_rate: Option<f64>,
}

struct FfmpegReader {
    child: Child,
    stdout: ChildStdout,
}

fn parse_ffmpeg_rational(value: &str) -> Option<f64> {
    let mut parts = value.split('/');
    let numerator = parts.next()?.trim().parse::<f64>().ok()?;
    let denominator = parts.next()?.trim().parse::<f64>().ok()?;
    if denominator == 0.0 {
        None
    } else {
        Some(numerator / denominator)
    }
}

fn probe_ffmpeg_video(path: &PathBuf) -> Result<FfmpegVideoInfo, String> {
    let output = Command::new("ffprobe")
        .arg("-v")
        .arg("error")
        .arg("-select_streams")
        .arg("v:0")
        .arg("-show_entries")
        .arg("stream=width,height,avg_frame_rate")
        .arg("-of")
        .arg("json")
        .arg(path)
        .output()
        .map_err(|e| format!("ffprobe spawn failed: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "ffprobe error: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let probe: FfmpegProbe =
        serde_json::from_slice(&output.stdout).map_err(|e| format!("ffprobe parse: {}", e))?;
    let stream = probe
        .streams
        .into_iter()
        .next()
        .ok_or_else(|| "ffprobe returned no video streams".to_string())?;

    let width = stream
        .width
        .ok_or_else(|| "ffprobe missing width".to_string())?;
    let height = stream
        .height
        .ok_or_else(|| "ffprobe missing height".to_string())?;
    let frame_rate = stream
        .avg_frame_rate
        .as_deref()
        .and_then(parse_ffmpeg_rational);

    Ok(FfmpegVideoInfo {
        width,
        height,
        frame_rate,
    })
}

fn spawn_ffmpeg_reader(path: &PathBuf, seek_ms: Option<u64>) -> Result<FfmpegReader, String> {
    let mut command = Command::new("ffmpeg");
    command
        .arg("-v")
        .arg("error")
        .arg("-hide_banner")
        .arg("-nostdin");

    if let Some(seek_ms) = seek_ms {
        command.arg("-ss").arg(format!("{:.3}", seek_ms as f64 / 1000.0));
    }

    command
        .arg("-i")
        .arg(path)
        .arg("-an")
        .arg("-f")
        .arg("rawvideo")
        .arg("-pix_fmt")
        .arg("rgb24")
        .arg("-");

    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("ffmpeg spawn failed: {}", e))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "ffmpeg stdout unavailable".to_string())?;

    Ok(FfmpegReader { child, stdout })
}

fn decode_ffmpeg_headless(path: &PathBuf, target_frames: u64) -> Result<HeadlessStats, String> {
    let info = probe_ffmpeg_video(path)?;
    let mut reader = spawn_ffmpeg_reader(path, None)?;
    let frame_size = info.width as usize * info.height as usize * 3;
    let mut buffer = vec![0u8; frame_size];
    let frame_duration_ms = info
        .frame_rate
        .map(|fps| (1000.0 / fps).round() as u64)
        .unwrap_or(33);
    let mut decoded_frames: u64 = 0;
    let mut last_pts_ms: u64 = 0;

    while decoded_frames < target_frames {
        match reader.stdout.read_exact(&mut buffer) {
            Ok(()) => {
                last_pts_ms = decoded_frames * frame_duration_ms;
                decoded_frames += 1;
            }
            Err(e) if e.kind() == ErrorKind::UnexpectedEof => {
                return Err("ffmpeg ended before target frames".to_string());
            }
            Err(e) => {
                return Err(format!("ffmpeg read error: {}", e));
            }
        }
    }

    let _ = reader.child.kill();

    Ok(HeadlessStats {
        decoded_frames,
        duration_ms: last_pts_ms,
    })
}

fn decode_mp4_headless(path: &PathBuf, target_frames: u64) -> Result<HeadlessStats, String> {
    use std::fs::File;
    use std::io::BufReader;

    let file = File::open(path).map_err(|e| format!("Open error: {}", e))?;
    let reader = BufReader::new(file);

    let mut demuxer = Mp4Demuxer::new(reader).map_err(|e| format!("Demux init: {}", e))?;

    let streams = demuxer.streams();
    let (video_idx, video_info) = streams
        .iter()
        .enumerate()
        .find(|(_, s)| matches!(s.codec_type, slain_core::mp4_demux::CodecType::Video))
        .ok_or_else(|| "No video stream found in MP4".to_string())?;

    let (vid_w, vid_h) = if let Some(vi) = demuxer.video_info(video_idx) {
        (vi.width, vi.height)
    } else {
        (1920, 1080)
    };

    let codec = mp4_codec_to_hwcodec(&video_info.codec)
        .map_err(|e| format!("Unsupported MP4 codec for headless: {}", e))?;

    let config = DecoderConfig {
        codec,
        width: vid_w,
        height: vid_h,
        preferred_backend: None,
        allow_software_fallback: true,
        extra_data: Some(video_info.extra_data.clone()),
    };

    let mut decoder = HwDecoder::new(config)?;

    let mut decoded_frames: u64 = 0;
    let mut last_pts_ms: u64 = 0;
    let mut converter: Option<PixelConverter> = None;
    let mut converter_format: Option<PxFormat> = None;
    let mut converter_dims: Option<(u32, u32)> = None;

    while decoded_frames < target_frames {
        let packet = demuxer
            .read_packet()
            .ok_or_else(|| "Reached end of MP4 before target frames".to_string())?;

        if packet.stream_index != video_idx as u32 {
            continue;
        }

        match decoder.decode(&packet.data, packet.pts) {
            Ok(Some(decoded)) => {
                let src_format = match decoded.format {
                    slain_core::hw_decode::PixelFormat::NV12 => PxFormat::NV12,
                    slain_core::hw_decode::PixelFormat::P010 => PxFormat::P010,
                    _ => PxFormat::YUV420P,
                };

                let needs_new_converter = converter.is_none()
                    || converter_format != Some(src_format)
                    || converter_dims != Some((decoded.width, decoded.height));

                if needs_new_converter {
                    converter = Some(PixelConverter::new(
                        src_format,
                        PxFormat::RGB24,
                        decoded.width as usize,
                        decoded.height as usize,
                        ColorSpace::BT709,
                    ));
                    converter_format = Some(src_format);
                    converter_dims = Some((decoded.width, decoded.height));
                }

                let mut src_frame = PxVideoFrame::new(
                    decoded.width as usize,
                    decoded.height as usize,
                    src_format,
                );
                src_frame.data = decoded.data;

                let mut dst_frame = PxVideoFrame::new(
                    decoded.width as usize,
                    decoded.height as usize,
                    PxFormat::RGB24,
                );

                if let Some(ref converter) = converter {
                    converter
                        .convert(&src_frame, &mut dst_frame)
                        .map_err(|e| format!("Pixel convert error: {}", e))?;
                }

                let pts_ms = if packet.pts_ms > 0 {
                    packet.pts_ms as u64
                } else {
                    decoded_frames * 33
                };
                last_pts_ms = pts_ms;
                decoded_frames += 1;
            }
            Ok(None) => {}
            Err(e) => {
                return Err(format!("Decode error: {}", e));
            }
        }
    }

    Ok(HeadlessStats {
        decoded_frames,
        duration_ms: last_pts_ms,
    })
}

fn decode_mkv_headless(path: &PathBuf, target_frames: u64) -> Result<HeadlessStats, String> {
    use std::fs::File;
    use std::io::BufReader;

    let mut parser = MkvParser::new();
    let info = parser.parse(path)?;

    let file = File::open(path).map_err(|e| format!("Open error: {}", e))?;
    let reader = BufReader::new(file);

    let mut demuxer = MkvDemuxer::new(reader, info.clone())?;

    let video_track_num = demuxer
        .video_track()
        .ok_or_else(|| "No video track found in MKV".to_string())?;

    let (vid_w, vid_h, codec_id) = info
        .tracks
        .iter()
        .find_map(|t| {
            if let MkvTrack::Video(v) = t {
                Some((v.pixel_width, v.pixel_height, v.codec_id.clone()))
            } else {
                None
            }
        })
        .unwrap_or((1920, 1080, String::new()));

    let codec = mkv_codec_to_hwcodec(&codec_id)
        .map_err(|e| format!("Unsupported MKV codec for headless: {}", e))?;

    let config = DecoderConfig {
        codec,
        width: vid_w,
        height: vid_h,
        preferred_backend: None,
        allow_software_fallback: true,
        extra_data: None,
    };

    let mut decoder = HwDecoder::new(config)?;

    let mut decoded_frames: u64 = 0;
    let mut last_pts_ms: u64 = 0;
    let mut converter: Option<PixelConverter> = None;
    let mut converter_format: Option<PxFormat> = None;
    let mut converter_dims: Option<(u32, u32)> = None;

    while decoded_frames < target_frames {
        let packet = demuxer
            .read_packet()
            .ok_or_else(|| "Reached end of MKV before target frames".to_string())?;

        if packet.track_number != video_track_num {
            continue;
        }

        match decoder.decode(&packet.data, packet.pts_ms) {
            Ok(Some(decoded)) => {
                let src_format = match decoded.format {
                    slain_core::hw_decode::PixelFormat::NV12 => PxFormat::NV12,
                    slain_core::hw_decode::PixelFormat::P010 => PxFormat::P010,
                    _ => PxFormat::YUV420P,
                };

                let needs_new_converter = converter.is_none()
                    || converter_format != Some(src_format)
                    || converter_dims != Some((decoded.width, decoded.height));

                if needs_new_converter {
                    converter = Some(PixelConverter::new(
                        src_format,
                        PxFormat::RGB24,
                        decoded.width as usize,
                        decoded.height as usize,
                        ColorSpace::BT709,
                    ));
                    converter_format = Some(src_format);
                    converter_dims = Some((decoded.width, decoded.height));
                }

                let mut src_frame = PxVideoFrame::new(
                    decoded.width as usize,
                    decoded.height as usize,
                    src_format,
                );
                src_frame.data = decoded.data;

                let mut dst_frame = PxVideoFrame::new(
                    decoded.width as usize,
                    decoded.height as usize,
                    PxFormat::RGB24,
                );

                if let Some(ref converter) = converter {
                    converter
                        .convert(&src_frame, &mut dst_frame)
                        .map_err(|e| format!("Pixel convert error: {}", e))?;
                }

                last_pts_ms = packet.pts_ms.max(0) as u64;
                decoded_frames += 1;
            }
            Ok(None) => {}
            Err(e) => {
                return Err(format!("Decode error: {}", e));
            }
        }
    }

    Ok(HeadlessStats {
        decoded_frames,
        duration_ms: last_pts_ms,
    })
}

fn decode_ffmpeg(shared: Arc<PlaybackShared>, path: &PathBuf, fallback_width: u32, fallback_height: u32) -> Result<(), String> {
    let info = probe_ffmpeg_video(path).unwrap_or_else(|err| {
        tracing::warn!("ffprobe failed: {}", err);
        FfmpegVideoInfo {
            width: fallback_width.max(1),
            height: fallback_height.max(1),
            frame_rate: None,
        }
    });

    let mut reader = spawn_ffmpeg_reader(path, None)?;
    let frame_size = info.width as usize * info.height as usize * 3;
    let mut buffer = vec![0u8; frame_size];
    let frame_duration_ms = info
        .frame_rate
        .map(|fps| (1000.0 / fps).round() as u64)
        .unwrap_or(33);
    let mut frame_number: u64 = 0;
    let mut base_pts_ms: u64 = 0;

    while !shared.should_stop.load(Ordering::SeqCst) {
        if shared.frame_queue.lock().len() >= 4 {
            thread::sleep(Duration::from_millis(5));
            continue;
        }

        if shared.seek_requested.load(Ordering::SeqCst) {
            let target = shared.seek_target_ms.load(Ordering::SeqCst);
            shared.seek_requested.store(false, Ordering::SeqCst);
            shared.frame_queue.lock().clear();
            let _ = reader.child.kill();
            reader = spawn_ffmpeg_reader(path, Some(target))?;
            frame_number = 0;
            base_pts_ms = target;
            continue;
        }

        if !shared.is_playing.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(10));
            continue;
        }

        match reader.stdout.read_exact(&mut buffer) {
            Ok(()) => {
                let pts_ms = base_pts_ms + frame_number * frame_duration_ms;
                shared.current_time_ms.store(pts_ms, Ordering::SeqCst);
                shared.frame_queue.lock().push_back(RgbFrame {
                    data: buffer.clone(),
                    width: info.width,
                    height: info.height,
                    pts_ms,
                });
                frame_number += 1;
            }
            Err(e) if e.kind() == ErrorKind::UnexpectedEof => {
                tracing::info!("End of ffmpeg stream");
                break;
            }
            Err(e) => {
                return Err(format!("ffmpeg read error: {}", e));
            }
        }
    }

    let _ = reader.child.kill();

    Ok(())
}

/// Main decode loop - runs in separate thread
/// Reads packets from demuxer → decodes → converts to RGB → pushes to queue
fn decode_loop(shared: Arc<PlaybackShared>, path: PathBuf, width: u32, height: u32, use_ffmpeg: bool) {
    tracing::info!("Decode thread started for {:?}", path);
    
    let ext = path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    
    let mut result = if use_ffmpeg {
        decode_ffmpeg(shared.clone(), &path, width, height)
    } else {
        match ext.as_str() {
            "mp4" | "m4v" | "mov" => decode_mp4(shared.clone(), &path, width, height),
            "mkv" | "webm" => decode_mkv(shared.clone(), &path, width, height),
            "avi" => decode_avi(shared.clone(), &path, width, height),
            "ts" | "mts" | "m2ts" => decode_ts(shared.clone(), &path, width, height),
            _ => Err(format!("Unsupported container: {}", ext)),
        }
    };

    if let Err(e) = &result {
        tracing::error!("Decode failed: {}", e);
        if !use_ffmpeg {
            tracing::info!("Attempting ffmpeg fallback");
            result = decode_ffmpeg(shared.clone(), &path, width, height);
        }
    }

    if let Err(e) = result {
        tracing::error!("Decode failed after fallback: {}", e);
    }
    
    tracing::info!("Decode thread finished");
}

/// MKV decoding using MkvDemuxer + hw_decode + pixel_convert
fn decode_mkv(shared: Arc<PlaybackShared>, path: &PathBuf, width: u32, height: u32) -> Result<(), String> {
    use std::fs::File;
    use std::io::BufReader;
    
    // First parse MKV info
    let mut parser = MkvParser::new();
    let info = parser.parse(path)?;
    
    // Open file for demuxer
    let file = File::open(path).map_err(|e| format!("Open error: {}", e))?;
    let reader = BufReader::new(file);
    
    let mut demuxer = MkvDemuxer::new(reader, info.clone())?;
    
    // Get video track info
    let video_track_num = match demuxer.video_track() {
        Some(t) => t,
        None => {
            tracing::error!("No video track found in MKV");
            return Err("No video track found".into());
        }
    };
    
    // Get dimensions from track info
    let (vid_w, vid_h, codec_id) = info
        .tracks
        .iter()
        .find_map(|t| {
            if let MkvTrack::Video(v) = t {
                Some((v.pixel_width, v.pixel_height, v.codec_id.clone()))
            } else {
                None
            }
        })
        .unwrap_or((width, height, String::new()));
    
    tracing::info!("MKV demuxer ready: {}x{}, video track {}", vid_w, vid_h, video_track_num);
    
    // Create hardware decoder (tries NVDEC → AMF → VAAPI → Software)
    let codec = mkv_codec_to_hwcodec(&codec_id)?;
    let config = DecoderConfig {
        codec,
        width: vid_w,
        height: vid_h,
        preferred_backend: None,  // Auto-detect best available
        allow_software_fallback: true,
        extra_data: None,
    };
    
    let mut decoder = HwDecoder::new(config)?;
    
    let mut _frame_number: u64 = 0;

    while !shared.should_stop.load(Ordering::SeqCst) {
        if !shared.is_playing.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(10));
            continue;
        }
        
        if shared.frame_queue.lock().len() >= 4 {
            thread::sleep(Duration::from_millis(5));
            continue;
        }
        
        // Handle seek
        if shared.seek_requested.load(Ordering::SeqCst) {
            let target = shared.seek_target_ms.load(Ordering::SeqCst);
            let _ = demuxer.seek(target);
            shared.seek_requested.store(false, Ordering::SeqCst);
            shared.frame_queue.lock().clear();
        }
        
        // Read next packet
        match demuxer.read_packet() {
            Some(packet) => {
                // Only decode video track
                if packet.track_number != video_track_num {
                    continue;
                }
                
                // Decode
                match decoder.decode(&packet.data, packet.pts_ms) {
                    Ok(Some(decoded)) => {
                        // Convert decoded format to RGB
                        // NVDEC outputs NV12, Software outputs YUV420
                        let src_format = match decoded.format {
                            slain_core::hw_decode::PixelFormat::NV12 => PxFormat::NV12,
                            slain_core::hw_decode::PixelFormat::P010 => PxFormat::P010,
                            _ => PxFormat::YUV420P,
                        };
                        
                        // Create converter for this format if needed
                        let converter = PixelConverter::new(
                            src_format,
                            PxFormat::RGB24,
                            decoded.width as usize,
                            decoded.height as usize,
                            ColorSpace::BT709,
                        );
                        
                        let mut src_frame = PxVideoFrame::new(
                            decoded.width as usize,
                            decoded.height as usize,
                            src_format,
                        );
                        src_frame.data = decoded.data;
                        
                        let mut dst_frame = PxVideoFrame::new(
                            decoded.width as usize,
                            decoded.height as usize,
                            PxFormat::RGB24,
                        );
                        
                        if let Err(e) = converter.convert(&src_frame, &mut dst_frame) {
                            tracing::warn!("Pixel convert error: {}", e);
                            continue;
                        }
                        
                        let pts_ms = packet.pts_ms.max(0) as u64;
                        shared.current_time_ms.store(pts_ms, Ordering::SeqCst);
                        
                        shared.frame_queue.lock().push_back(RgbFrame {
                            data: dst_frame.data,
                            width: decoded.width,
                            height: decoded.height,
                            pts_ms,
                        });

                        _frame_number += 1;
                    }
                    Ok(None) => {}
                    Err(e) => {
                        tracing::warn!("Decode error: {}", e);
                    }
                }
                
                thread::sleep(Duration::from_millis(16));
            }
            None => {
                tracing::info!("End of MKV file");
                break;
            }
        }
    }
    
    Ok(())
}

/// AVI decoding using AviDemuxer + hw_decode + pixel_convert
fn decode_avi(shared: Arc<PlaybackShared>, path: &PathBuf, width: u32, height: u32) -> Result<(), String> {
    use std::fs::File;
    use std::io::BufReader;

    let file = File::open(path).map_err(|e| format!("Open error: {}", e))?;
    let reader = BufReader::new(file);

    let mut demuxer = AviDemuxer::new(reader)?;
    let info = demuxer.info();

    let video_stream = info
        .streams
        .iter()
        .find(|stream| matches!(stream.stream_type, slain_core::avi_demux::StreamType::Video))
        .ok_or_else(|| "No video stream found in AVI".to_string())?;

    let (vid_w, vid_h) = (
        video_stream.width.unwrap_or(width),
        video_stream.height.unwrap_or(height),
    );

    let codec = match video_stream.codec {
        slain_core::avi_demux::CodecType::H264 => HwCodec::H264,
        other => {
            return Err(format!("Unsupported AVI codec: {:?}", other));
        }
    };

    let config = DecoderConfig {
        codec,
        width: vid_w,
        height: vid_h,
        preferred_backend: None,
        allow_software_fallback: true,
        extra_data: None,
    };

    let mut decoder = HwDecoder::new(config)?;
    let mut frame_number: u64 = 0;

    while !shared.should_stop.load(Ordering::SeqCst) {
        if !shared.is_playing.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(10));
            continue;
        }

        if shared.frame_queue.lock().len() >= 4 {
            thread::sleep(Duration::from_millis(5));
            continue;
        }

        if shared.seek_requested.load(Ordering::SeqCst) {
            let target = shared.seek_target_ms.load(Ordering::SeqCst);
            let _ = demuxer.seek((target as i64) * 1000);
            shared.seek_requested.store(false, Ordering::SeqCst);
            shared.frame_queue.lock().clear();
        }

        match demuxer.read_packet() {
            Some(packet) => {
                if packet.stream_index != video_stream.index {
                    continue;
                }

                match decoder.decode(&packet.data, packet.pts) {
                    Ok(Some(decoded)) => {
                        let src_format = match decoded.format {
                            slain_core::hw_decode::PixelFormat::NV12 => PxFormat::NV12,
                            slain_core::hw_decode::PixelFormat::P010 => PxFormat::P010,
                            _ => PxFormat::YUV420P,
                        };

                        let converter = PixelConverter::new(
                            src_format,
                            PxFormat::RGB24,
                            decoded.width as usize,
                            decoded.height as usize,
                            ColorSpace::BT709,
                        );

                        let mut src_frame = PxVideoFrame::new(
                            decoded.width as usize,
                            decoded.height as usize,
                            src_format,
                        );
                        src_frame.data = decoded.data;

                        let mut dst_frame = PxVideoFrame::new(
                            decoded.width as usize,
                            decoded.height as usize,
                            PxFormat::RGB24,
                        );

                        if let Err(e) = converter.convert(&src_frame, &mut dst_frame) {
                            tracing::warn!("Pixel convert error: {}", e);
                            continue;
                        }

                        let pts_ms = if packet.pts > 0 {
                            (packet.pts as u64) / 1000
                        } else {
                            frame_number * 33
                        };
                        shared.current_time_ms.store(pts_ms, Ordering::SeqCst);

                        shared.frame_queue.lock().push_back(RgbFrame {
                            data: dst_frame.data,
                            width: decoded.width,
                            height: decoded.height,
                            pts_ms,
                        });

                        frame_number += 1;
                    }
                    Ok(None) => {}
                    Err(e) => {
                        tracing::warn!("Decode error: {}", e);
                    }
                }

                thread::sleep(Duration::from_millis(16));
            }
            None => {
                tracing::info!("End of AVI file");
                break;
            }
        }
    }

    Ok(())
}

/// TS decoding using TsDemuxer + hw_decode + pixel_convert
fn decode_ts(shared: Arc<PlaybackShared>, path: &PathBuf, width: u32, height: u32) -> Result<(), String> {
    use std::fs::File;
    use std::io::BufReader;

    let file = File::open(path).map_err(|e| format!("Open error: {}", e))?;
    let reader = BufReader::new(file);

    let mut demuxer = TsDemuxer::new(reader)?;
    let info = demuxer.info();

    let video_stream = info
        .streams
        .iter()
        .find(|stream| matches!(stream.codec, TsStreamCodec::H264
            | TsStreamCodec::H265
            | TsStreamCodec::MPEG2Video
            | TsStreamCodec::MPEG1Video))
        .ok_or_else(|| "No video stream found in TS".to_string())?;

    let codec = match video_stream.codec {
        TsStreamCodec::H264 => HwCodec::H264,
        TsStreamCodec::H265 => HwCodec::H265,
        TsStreamCodec::MPEG2Video => HwCodec::MPEG2,
        TsStreamCodec::MPEG1Video => HwCodec::MPEG2,
        other => {
            return Err(format!("Unsupported TS codec: {:?}", other));
        }
    };

    let config = DecoderConfig {
        codec,
        width,
        height,
        preferred_backend: None,
        allow_software_fallback: true,
        extra_data: None,
    };

    let mut decoder = HwDecoder::new(config)?;
    let mut frame_number: u64 = 0;

    while !shared.should_stop.load(Ordering::SeqCst) {
        if !shared.is_playing.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(10));
            continue;
        }

        if shared.frame_queue.lock().len() >= 4 {
            thread::sleep(Duration::from_millis(5));
            continue;
        }

        if shared.seek_requested.load(Ordering::SeqCst) {
            shared.seek_requested.store(false, Ordering::SeqCst);
            shared.frame_queue.lock().clear();
        }

        match demuxer.read_packet() {
            Some(packet) => {
                if packet.pid != video_stream.pid {
                    continue;
                }

                let pts = packet.pts.unwrap_or(0);
                match decoder.decode(&packet.data, pts) {
                    Ok(Some(decoded)) => {
                        let src_format = match decoded.format {
                            slain_core::hw_decode::PixelFormat::NV12 => PxFormat::NV12,
                            slain_core::hw_decode::PixelFormat::P010 => PxFormat::P010,
                            _ => PxFormat::YUV420P,
                        };

                        let converter = PixelConverter::new(
                            src_format,
                            PxFormat::RGB24,
                            decoded.width as usize,
                            decoded.height as usize,
                            ColorSpace::BT709,
                        );

                        let mut src_frame = PxVideoFrame::new(
                            decoded.width as usize,
                            decoded.height as usize,
                            src_format,
                        );
                        src_frame.data = decoded.data;

                        let mut dst_frame = PxVideoFrame::new(
                            decoded.width as usize,
                            decoded.height as usize,
                            PxFormat::RGB24,
                        );

                        if let Err(e) = converter.convert(&src_frame, &mut dst_frame) {
                            tracing::warn!("Pixel convert error: {}", e);
                            continue;
                        }

                        let pts_ms = if pts > 0 {
                            (pts as u64) / 1000
                        } else {
                            frame_number * 33
                        };
                        shared.current_time_ms.store(pts_ms, Ordering::SeqCst);

                        shared.frame_queue.lock().push_back(RgbFrame {
                            data: dst_frame.data,
                            width: decoded.width,
                            height: decoded.height,
                            pts_ms,
                        });

                        frame_number += 1;
                    }
                    Ok(None) => {}
                    Err(e) => {
                        tracing::warn!("Decode error: {}", e);
                    }
                }

                thread::sleep(Duration::from_millis(16));
            }
            None => {
                tracing::info!("End of TS file");
                break;
            }
        }
    }

    Ok(())
}

/// Real MP4 decoding using mp4_demux + hw_decode + pixel_convert
fn decode_mp4(shared: Arc<PlaybackShared>, path: &PathBuf, width: u32, height: u32) -> Result<(), String> {
    use std::fs::File;
    use std::io::BufReader;
    
    let file = File::open(path).map_err(|e| format!("Open error: {}", e))?;
    let reader = BufReader::new(file);
    
    let mut demuxer = Mp4Demuxer::new(reader).map_err(|e| format!("Demux init: {}", e))?;
    
    // Get streams info
    let streams = demuxer.streams();
    let (video_idx, video_info) = match streams.iter()
        .enumerate()
        .find(|(_, s)| matches!(s.codec_type, slain_core::mp4_demux::CodecType::Video)) 
    {
        Some((idx, info)) => (idx, info),
        None => {
            tracing::error!("No video stream found in MP4");
            return Err("No video stream found".into());
        }
    };
    tracing::info!("Video stream {}: codec={:?}", video_idx, video_info.codec);
    
    // Get video dimensions from demuxer
    let (vid_w, vid_h) = if let Some(vi) = demuxer.video_info(video_idx) {
        (vi.width, vi.height)
    } else {
        (width, height)
    };
    
    // Create hardware decoder (tries NVDEC → AMF → VAAPI → Software)
    let codec = mp4_codec_to_hwcodec(&video_info.codec)?;
    let config = DecoderConfig {
        codec,
        width: vid_w,
        height: vid_h,
        preferred_backend: None,  // Auto-detect best available
        allow_software_fallback: true,
        extra_data: Some(video_info.extra_data.clone()),
    };
    
    let mut decoder = HwDecoder::new(config)?;
    
    tracing::info!("MP4 decode ready: {}x{}", vid_w, vid_h);
    
    let mut frame_number: u64 = 0;
    
    while !shared.should_stop.load(Ordering::SeqCst) {
        // Only decode when playing
        if !shared.is_playing.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(10));
            continue;
        }
        
        // Don't overflow the queue
        if shared.frame_queue.lock().len() >= 4 {
            thread::sleep(Duration::from_millis(5));
            continue;
        }
        
        // Handle seek
        if shared.seek_requested.load(Ordering::SeqCst) {
            let _target = shared.seek_target_ms.load(Ordering::SeqCst);
            shared.seek_requested.store(false, Ordering::SeqCst);
            shared.frame_queue.lock().clear();
        }
        
        // Read next packet
        match demuxer.read_packet() {
            Some(packet) => {
                // Only decode video packets
                if packet.stream_index != video_idx as u32 {
                    continue;
                }
                
                // Decode packet
                match decoder.decode(&packet.data, packet.pts) {
                    Ok(Some(decoded)) => {
                        // Convert decoded format to RGB
                        let src_format = match decoded.format {
                            slain_core::hw_decode::PixelFormat::NV12 => PxFormat::NV12,
                            slain_core::hw_decode::PixelFormat::P010 => PxFormat::P010,
                            _ => PxFormat::YUV420P,
                        };
                        
                        let converter = PixelConverter::new(
                            src_format,
                            PxFormat::RGB24,
                            decoded.width as usize,
                            decoded.height as usize,
                            ColorSpace::BT709,
                        );
                        
                        let mut src_frame = PxVideoFrame::new(
                            decoded.width as usize,
                            decoded.height as usize,
                            src_format,
                        );
                        src_frame.data = decoded.data;
                        
                        let mut dst_frame = PxVideoFrame::new(
                            decoded.width as usize,
                            decoded.height as usize,
                            PxFormat::RGB24,
                        );
                        
                        if let Err(e) = converter.convert(&src_frame, &mut dst_frame) {
                            tracing::warn!("Pixel convert error: {}", e);
                            continue;
                        }
                        
                        // Use pre-calculated PTS in milliseconds from demuxer
                        let pts_ms = if packet.pts_ms > 0 {
                            packet.pts_ms as u64
                        } else {
                            frame_number * 33
                        };
                        
                        shared.current_time_ms.store(pts_ms, Ordering::SeqCst);
                        
                        shared.frame_queue.lock().push_back(RgbFrame {
                            data: dst_frame.data,
                            width: decoded.width,
                            height: decoded.height,
                            pts_ms,
                        });
                        
                        frame_number += 1;
                    }
                    Ok(None) => {
                        // Decoder needs more data
                    }
                    Err(e) => {
                        tracing::warn!("Decode error: {}", e);
                    }
                }
                
                // Pace to target frame rate
                thread::sleep(Duration::from_millis(16)); // ~60fps target
            }
            None => {
                tracing::info!("End of file");
                break;
            }
        }
    }
    
    Ok(())
}

/// Fallback test pattern for formats without demuxer
fn decode_test_pattern(shared: Arc<PlaybackShared>, width: u32, height: u32) {
    let frame_duration = Duration::from_millis(33);
    let mut frame_number: u64 = 0;
    
    loop {
        if shared.should_stop.load(Ordering::SeqCst) {
            break;
        }
        
        if !shared.is_playing.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(10));
            continue;
        }
        
        if shared.frame_queue.lock().len() >= 4 {
            thread::sleep(Duration::from_millis(5));
            continue;
        }
        
        if shared.seek_requested.load(Ordering::SeqCst) {
            let target = shared.seek_target_ms.load(Ordering::SeqCst);
            shared.current_time_ms.store(target, Ordering::SeqCst);
            shared.seek_requested.store(false, Ordering::SeqCst);
            frame_number = (target as f64 / 33.33) as u64;
            shared.frame_queue.lock().clear();
        }
        
        let pts_ms = frame_number * 33;
        shared.current_time_ms.store(pts_ms, Ordering::SeqCst);
        
        let frame = generate_test_frame(width, height, frame_number, pts_ms);
        shared.frame_queue.lock().push_back(frame);
        frame_number += 1;
        
        thread::sleep(frame_duration);
    }
}

/// Generate a test pattern frame for pipeline verification
fn generate_test_frame(width: u32, height: u32, frame_num: u64, pts_ms: u64) -> RgbFrame {
    let mut data = vec![0u8; (width * height * 3) as usize];
    
    // Moving gradient pattern
    let offset = (frame_num % 256) as u8;
    
    for y in 0..height {
        for x in 0..width {
            let idx = ((y * width + x) * 3) as usize;
            
            // Color bars pattern with animation
            let bar = (x * 8 / width) as u8;
            let r = ((bar & 1) * 255).wrapping_add(offset);
            let g = (((bar >> 1) & 1) * 255).wrapping_add(offset);
            let b = (((bar >> 2) & 1) * 255).wrapping_add(offset);
            
            data[idx] = r;
            data[idx + 1] = g;
            data[idx + 2] = b;
        }
    }
    
    // Draw frame counter in top-left (simple)
    // (Just a visual marker, not actual text rendering)
    let marker_size = 20;
    let marker_x = 10;
    let marker_y = 10;
    let marker_color = ((frame_num * 3) % 256) as u8;
    
    for y in marker_y..(marker_y + marker_size).min(height) {
        for x in marker_x..(marker_x + marker_size).min(width) {
            let idx = ((y * width + x) * 3) as usize;
            data[idx] = marker_color;
            data[idx + 1] = 255 - marker_color;
            data[idx + 2] = 128;
        }
    }
    
    RgbFrame {
        data,
        width,
        height,
        pts_ms,
    }
}
