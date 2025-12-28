//! # SLAIN Video Player
//!
//! Pure Rust GPU-accelerated video player using slain-core modules.

use anyhow::Result;
use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};
use parking_lot::Mutex;
use serde::Deserialize;
use std::collections::VecDeque;
use std::io::{ErrorKind, Read};
use std::path::PathBuf;
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

// Import from our core library - NOT rewriting
use slain_core::audio::{audio_set_volume, AudioPlayer};
use slain_core::avi_demux::AviDemuxer;
use slain_core::bandwidth::window_monitor;
use slain_core::filter_pipeline::{
    ContainerFormat, FilterChainSpec, FilterRegistry, PipelineProfile, PipelineProfileSelector,
    ProfileScope,
};
use slain_core::h264_utils::{avcc_to_annexb, is_annexb, parse_avcc_extradata};
use slain_core::hw_decode::{
    available_decoders, find_best_decoder, DecoderConfig, HwCodec, HwDecoder, HwDecoderType,
};
use slain_core::mkv::{MkvDemuxer, MkvInfo, MkvParser, MkvTrack};
use slain_core::mp4_demux::mp4::Mp4Demuxer;
use slain_core::pipeline::{PipelineKind, PipelineManager};
use slain_core::filter_pipeline::{
    ContainerFormat,
    FilterRegistry,
    FilterChainSpec,
    PipelineProfile,
    PipelineProfileSelector,
    ProfileScope,
};
use slain_core::h264_utils::{parse_avcc_extradata, avcc_to_annexb, is_annexb};

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
    data: Vec<u8>, // RGB24
    width: u32,
    height: u32,
    #[allow(dead_code)] // Used for frame pacing in future
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

    // Log available decoders with details
    let decoders = available_decoders();
    tracing::info!("Available decoders: {:?}", decoders);

    // Check NVDEC specifically
    if slain_core::nvdec::nvdec_available() {
        let caps = slain_core::nvdec::nvdec_capabilities();
        tracing::info!(
            "NVDEC AVAILABLE: {} (compute {}.{})",
            caps.device_name,
            caps.compute_capability.0,
            caps.compute_capability.1
        );
        tracing::info!("NVDEC codecs: {:?}", caps.supported_codecs);
    } else {
        tracing::warn!("NVDEC NOT AVAILABLE - will use software decoder");
        tracing::warn!(
            "Make sure NVIDIA drivers are installed and nvcuda.dll/nvcuvid.dll are accessible"
        );
    }

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

    // External decode support
    ffmpeg_available: bool,

    // Audio player from slain-core
    audio_player: Option<AudioPlayer>,
    audio_started: bool,

    // Pipeline selection
    pipeline: PipelineKind,
    pipeline_manager: Option<PipelineManager>,

    // Filter pipeline profiles
    pipeline_profiles: PipelineProfileSelector,
    current_container: Option<ContainerFormat>,

    // Decoder preference
    #[allow(dead_code)] // Will be used for decoder selection UI
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
    show_controls: bool,

    // Stats
    fps: f32,
    frame_count: u64,
    #[allow(dead_code)]
    dropped_frames: u32,
    decoder_name: String,
}

impl SlainApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Configure dark theme with custom colors
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = egui::Color32::from_rgb(25, 25, 30);
        visuals.window_fill = egui::Color32::from_rgb(30, 30, 35);
        visuals.extreme_bg_color = egui::Color32::from_rgb(15, 15, 18);

        // Widget styling
        visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(45, 45, 50);
        visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(60, 60, 70);
        visuals.widgets.active.bg_fill = egui::Color32::from_rgb(80, 80, 100);

        // Rounded corners
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

        // Auto-detect best pipeline
        let default_pipeline = if slain_core::nvdec::nvdec_available() {
            PipelineKind::Nvdec
        } else {
            PipelineKind::SoftwareOnly
        };

        // Prefer NVDEC if available
        let preferred_decoder = if slain_core::nvdec::nvdec_available() {
            Some(HwDecoderType::Nvdec)
        } else {
            None
        };
        
        let default_chain = FilterRegistry::global()
            .read()
            .chain_spec_for(&ContainerFormat::Mp4);
        let global_profile = PipelineProfile::new(
            "Default",
            default_pipeline,
            None,
            default_chain,
        );

        Self {
            playback_state: PlaybackState::Idle,
            media_info: None,
            video_path: None,
            shared: PlaybackShared::new(),
            decode_thread: None,
            current_time_ms: 0,
            duration_ms: 0,
            volume: 1.0,
            audio_player: None,
            audio_started: false,
            pipeline: default_pipeline,
            pipeline_manager: None,
            pipeline_profiles: PipelineProfileSelector::new(global_profile),
            current_container: None,
            preferred_decoder,
            video_texture: None,
            frame_width: 1920,
            frame_height: 1080,
            last_frame_time: Instant::now(),
            playback_start_time: None,
            last_displayed_pts: 0,
            show_osd: true,
            is_fullscreen: false,
            show_settings: false,
            show_controls: false,
            fps: 0.0,
            frame_count: 0,
            dropped_frames: 0,
            decoder_name: "None".to_string(),
        }
    }

    fn apply_backend_change(&mut self) {
        if let Some(path) = self.video_path.clone() {
            self.open_file(path);
        }
    }

    fn apply_pipeline_profile(&mut self, path: Option<&PathBuf>) {
        let profile = self.pipeline_profiles.profile_for(path.map(|p| p.as_path()));
        self.pipeline = profile.pipeline_kind;
        if let Some(ref mut manager) = self.pipeline_manager {
            manager.set_active(profile.pipeline_kind);
        }
    }
    
    /// Check if we're in a playable state
    fn is_playing(&self) -> bool {
        self.playback_state == PlaybackState::Playing
    }

    /// Check if file is loaded and ready
    fn is_ready(&self) -> bool {
        matches!(
            self.playback_state,
            PlaybackState::Ready | PlaybackState::Playing | PlaybackState::Paused
        )
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

        self.apply_pipeline_profile(Some(&path));

        // Determine file type by extension
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        self.current_container = ContainerFormat::from_extension(&ext);
        
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
                tracing::info!(
                    "MKV parsed: {} tracks, {} ms",
                    info.tracks.len(),
                    info.duration_ms
                );

                self.duration_ms = info.duration_ms;

                // Find video track for dimensions
                for track in &info.tracks {
                    if let MkvTrack::Video(v) = track {
                        self.frame_width = v.pixel_width;
                        self.frame_height = v.pixel_height;
                        tracing::info!(
                            "Video: {}x{} @ {} fps",
                            v.pixel_width,
                            v.pixel_height,
                            v.frame_rate.unwrap_or(0.0)
                        );

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
                self.duration_ms = (demuxer.duration_us() / 1000) as u64;

                let streams = demuxer.streams();
                for (idx, stream) in streams.iter().enumerate() {
                    if matches!(stream.codec_type, slain_core::mp4_demux::CodecType::Video) {
                        if let Some(vi) = demuxer.video_info(idx) {
                            self.frame_width = vi.width;
                            self.frame_height = vi.height;
                            tracing::info!(
                                "MP4 Video: {}x{}, duration: {}ms",
                                vi.width,
                                vi.height,
                                self.duration_ms
                            );
                        }
                        break;
                    }
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

                self.decode_thread = Some(thread::spawn(move || {
                    decode_loop(shared, video_path, width, height);
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
                let video_stream = info.streams.iter().find(|stream| {
                    matches!(
                        stream.codec,
                        TsStreamCodec::H264
                            | TsStreamCodec::H265
                            | TsStreamCodec::MPEG2Video
                            | TsStreamCodec::MPEG1Video
                    )
                });
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

                self.decode_thread = Some(thread::spawn(move || {
                    decode_loop(shared, video_path, width, height);
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
        if !self.is_ready() {
            tracing::warn!("Cannot play: no file loaded");
            return;
        }

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
            _ => {}
        }
    }

    fn seek(&mut self, time_ms: u64) {
        self.current_time_ms = time_ms.min(self.duration_ms);

        self.shared
            .seek_target_ms
            .store(self.current_time_ms, Ordering::SeqCst);
        self.shared.seek_requested.store(true, Ordering::SeqCst);

        if self.is_playing() {
            self.playback_start_time = Some(Instant::now() - Duration::from_millis(time_ms));
        }
    }

    fn set_volume(&mut self, vol: f32) {
        self.volume = vol.clamp(0.0, 1.0);
        let _ = audio_set_volume(self.volume);
    }

    fn toggle_fullscreen(&mut self, ctx: &egui::Context) {
        self.is_fullscreen = !self.is_fullscreen;
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.is_fullscreen));
        window_monitor().set_fullscreen(self.is_fullscreen);
    }
}

// ============================================================================
// UI Rendering
// ============================================================================

impl eframe::App for SlainApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.is_playing() {
            ctx.request_repaint();
            self.current_time_ms = self.shared.current_time_ms.load(Ordering::Relaxed);
        }

        // Pull frame from queue and upload to texture
        if let Some(frame) = self.shared.frame_queue.lock().pop_front() {
            let now = Instant::now();
            let delta = now.duration_since(self.last_frame_time);
            if delta.as_secs_f32() > 0.0 {
                self.fps = 1.0 / delta.as_secs_f32();
            }
            self.last_frame_time = now;
            self.frame_count += 1;

            let image =
                ColorImage::from_rgb([frame.width as usize, frame.height as usize], &frame.data);

            self.video_texture =
                Some(ctx.load_texture("video_frame", image, TextureOptions::LINEAR));

            self.frame_width = frame.width;
            self.frame_height = frame.height;
        }

        // Menu bar
        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Open...").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter(
                                "Video",
                                &["mkv", "mp4", "avi", "webm", "mov", "ts", "m2ts"],
                            )
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

                ui.menu_button("Playback", |ui| {
                    let mut use_ffmpeg = self.use_ffmpeg;
                    let checkbox = egui::Checkbox::new(
                        &mut use_ffmpeg,
                        "Use FFmpeg sidecar (max compatibility)",
                    );
                    let response = ui.add_enabled(self.ffmpeg_available, checkbox);
                    if response.changed() {
                        self.use_ffmpeg = use_ffmpeg;
                        self.apply_backend_change();
                        ui.close_menu();
                    }

                    if !self.ffmpeg_available {
                        ui.label("FFmpeg not found on PATH.");
                    }

                    ui.separator();
                    ui.label(format!(
                        "Backend: {}",
                        if self.use_ffmpeg { "FFmpeg" } else { "Native" }
                    ));
                });

                ui.menu_button("Pipeline", |ui| {
                    if self.pipeline_manager.is_none() {
                        self.pipeline_manager = Some(PipelineManager::new());
                    }
                    if let Some(ref mut manager) = self.pipeline_manager {
                        let available = manager.available();
                        let scope = self.pipeline_profiles.scope_for(self.video_path.as_deref());
                        for p in available {
                            let selected = self.pipeline == p;
                            if ui.radio(selected, format!("{:?}", p)).clicked() {
                                self.pipeline = p;
                                let current = self.pipeline_profiles.profile_for(self.video_path.as_deref()).clone();
                                match scope {
                                    ProfileScope::Global => {
                                        self.pipeline_profiles.set_global(PipelineProfile::new(
                                            current.name,
                                            p,
                                            current.config.clone(),
                                            current.filter_chain.clone(),
                                        ));
                                    }
                                    ProfileScope::PerFile => {
                                        if let Some(path) = self.video_path.as_ref() {
                                            self.pipeline_profiles.set_for_file(
                                                path.clone(),
                                                PipelineProfile::new(
                                                    current.name,
                                                    p,
                                                    current.config.clone(),
                                                    current.filter_chain.clone(),
                                                ),
                                            );
                                        }
                                    }
                                }
                                manager.set_active(p);
                                ui.close_menu();
                            }
                        }
                    }
                });

                ui.menu_button("Filters", |ui| {
                    let registry = FilterRegistry::global();
                    let active_scope = self.pipeline_profiles.scope_for(self.video_path.as_deref());

                    ui.heading("Filter Registry");
                    for filter in registry.read().list_filters() {
                        ui.horizontal(|ui| {
                            ui.label(filter.name);
                            ui.label(format!("priority {}", filter.priority));
                        });
                    }

                    ui.separator();
                    ui.heading("Default Container Chains");
                    for container in [
                        ContainerFormat::Mp4,
                        ContainerFormat::Mkv,
                        ContainerFormat::Avi,
                        ContainerFormat::Ts,
                    ] {
                        let chain = registry.read().chain_spec_for(&container);
                        ui.label(format!(
                            "{}: {}",
                            container.label(),
                            chain.display_chain()
                        ));
                    }

                    ui.separator();
                    ui.heading("Active Pipeline Profile");
                    ui.label(format!(
                        "Scope: {}",
                        match active_scope {
                            ProfileScope::Global => "Global",
                            ProfileScope::PerFile => "Per-file",
                        }
                    ));

                    let active_profile = self.pipeline_profiles.profile_for(self.video_path.as_deref());
                    ui.label(format!("Profile: {}", active_profile.name));
                    ui.label(format!("Pipeline: {:?}", active_profile.pipeline_kind));
                    ui.label(format!(
                        "Filter Chain: {}",
                        active_profile.filter_chain.display_chain()
                    ));

                    if let Some(container) = self.current_container.as_ref() {
                        ui.separator();
                        ui.heading("Current Container Override");
                        let override_exists = registry.read().user_override_spec(container).is_some();
                        ui.label(format!("Container: {}", container.label()));
                        ui.label(if override_exists {
                            "Override: enabled"
                        } else {
                            "Override: none"
                        });

                        ui.horizontal(|ui| {
                            if ui.button("Disable filters for container").clicked() {
                                registry.write().set_user_override(
                                    container.clone(),
                                    FilterChainSpec::empty(Some(container.clone())),
                                );
                            }
                            if ui.button("Reset to defaults").clicked() {
                                registry.write().clear_user_override(container);
                            }
                        });
                    }

                    if let Some(path) = self.video_path.as_ref() {
                        ui.separator();
                        ui.heading("Profile Scope");
                        if matches!(active_scope, ProfileScope::Global) {
                            if ui.button("Use per-file profile for this file").clicked() {
                                let current = self.pipeline_profiles.global().clone();
                                self.pipeline_profiles.set_for_file(path.clone(), current);
                            }
                        } else if ui.button("Revert to global profile").clicked() {
                            self.pipeline_profiles.clear_for_file(path);
                        }
                    }
                });
                
                ui.menu_button("Audio", |ui| {
                    ui.label("Volume:");
                    ui.add(egui::Slider::new(&mut self.volume, 0.0..=1.0).show_value(false));
                });

                ui.menu_button("Help", |ui| {
                    if ui.button("Controls & Shortcuts").clicked() {
                        self.show_controls = true;
                        ui.close_menu();
                    }
                });
            });
        });

        // Video area
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(egui::Color32::BLACK))
            .show(ctx, |ui| {
                let rect = ui.available_rect_before_wrap();

                match &self.playback_state {
                    PlaybackState::Idle => {
                        ui.centered_and_justified(|ui| {
                            ui.heading(
                                egui::RichText::new("Drop a video file here\nor use File → Open")
                                    .color(egui::Color32::GRAY)
                                    .size(24.0),
                            );
                        });
                    }
                    PlaybackState::Loading => {
                        ui.centered_and_justified(|ui| {
                            ui.heading(
                                egui::RichText::new("Loading...").color(egui::Color32::GRAY),
                            );
                        });
                    }
                    PlaybackState::Error(msg) => {
                        ui.centered_and_justified(|ui| {
                            ui.heading(
                                egui::RichText::new(format!("Error: {}", msg))
                                    .color(egui::Color32::RED),
                            );
                        });
                    }
                    _ => {
                        if let Some(texture) = &self.video_texture {
                            let aspect = self.frame_width as f32 / self.frame_height as f32;
                            let panel_aspect = rect.width() / rect.height();

                            let (w, h) = if aspect > panel_aspect {
                                (rect.width(), rect.width() / aspect)
                            } else {
                                (rect.height() * aspect, rect.height())
                            };

                            let x = rect.min.x + (rect.width() - w) / 2.0;
                            let y = rect.min.y + (rect.height() - h) / 2.0;

                            let video_rect =
                                egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, h));

                            ui.painter().image(
                                texture.id(),
                                video_rect,
                                egui::Rect::from_min_max(
                                    egui::pos2(0.0, 0.0),
                                    egui::pos2(1.0, 1.0),
                                ),
                                egui::Color32::WHITE,
                            );
                        } else {
                            ui.centered_and_justified(|ui| {
                                ui.heading(
                                    egui::RichText::new("Waiting for frames...")
                                        .color(egui::Color32::GRAY),
                                );
                            });
                        }
                    }
                }

                // OSD overlay
                if self.show_osd && self.video_path.is_some() {
                    let osd_rect = egui::Rect::from_min_size(
                        rect.min + egui::vec2(10.0, 10.0),
                        egui::vec2(260.0, 170.0),
                    );

                    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(osd_rect), |ui| {
                        egui::Frame::popup(ui.style())
                            .fill(egui::Color32::from_black_alpha(200))
                            .rounding(8.0)
                            .inner_margin(10.0)
                            .show(ui, |ui| {
                                ui.label(format!(
                                    "Time: {} / {}",
                                    format_time(self.current_time_ms),
                                    format_time(self.duration_ms)
                                ));
                                ui.label(format!(
                                    "Resolution: {}x{}",
                                    self.frame_width, self.frame_height
                                ));
                                ui.label(format!("Decoder: {}", self.decoder_name));
                                ui.label(format!("Pipeline: {:?}", self.pipeline));
                                ui.label(format!(
                                    "Backend: {}",
                                    if self.use_ffmpeg { "FFmpeg" } else { "Native" }
                                ));
                                ui.label(format!(
                                    "FFmpeg: {}",
                                    if self.ffmpeg_available {
                                        "available"
                                    } else {
                                        "missing"
                                    }
                                ));
                                ui.label(format!("FPS: {:.1}", self.fps));
                                ui.label(format!("Volume: {:.0}%", self.volume * 100.0));
                            });
                    });
                }
            });

        // Bottom controls
        egui::TopBottomPanel::bottom("controls")
            .min_height(60.0)
            .show(ctx, |ui| {
                ui.add_space(8.0);

                let mut time_sec = self.current_time_ms as f64 / 1000.0;
                let duration_sec = self.duration_ms as f64 / 1000.0;

                let slider = egui::Slider::new(&mut time_sec, 0.0..=duration_sec.max(1.0))
                    .show_value(false)
                    .trailing_fill(true);

                if ui.add(slider).changed() {
                    self.seek((time_sec * 1000.0) as u64);
                }

                ui.horizontal(|ui| {
                    let icon = if self.is_playing() { "⏸" } else { "▶" };
                    if ui.button(egui::RichText::new(icon).size(20.0)).clicked() {
                        self.toggle_play();
                    }

                    if ui.button(egui::RichText::new("⏹").size(20.0)).clicked() {
                        self.playback_state = PlaybackState::Ready;
                        self.shared.is_playing.store(false, Ordering::SeqCst);
                        self.current_time_ms = 0;
                    }

                    ui.label(format!(
                        "{} / {}",
                        format_time(self.current_time_ms),
                        format_time(self.duration_ms)
                    ));

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(egui::RichText::new("⛶").size(18.0)).clicked() {
                            self.toggle_fullscreen(ctx);
                        }

                        ui.add(
                            egui::Slider::new(&mut self.volume, 0.0..=1.0)
                                .show_value(false)
                                .fixed_decimals(0),
                        );

                        let vol_icon = if self.volume == 0.0 {
                            "🔇"
                        } else if self.volume < 0.5 {
                            "🔉"
                        } else {
                            "🔊"
                        };
                        ui.label(vol_icon);
                    });
                });

                ui.add_space(4.0);
            });

        if self.show_controls {
            egui::Window::new("Controls & Shortcuts")
                .open(&mut self.show_controls)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("Playback");
                    ui.separator();
                    ui.label("Space: Play/Pause");
                    ui.label("⏪ / ⏩ buttons: Seek ±10s");
                    ui.label("Arrow Left/Right: Seek ±5s");
                    ui.label("Arrow Up/Down: Volume ±5%");
                    ui.label("F or Alt+Enter: Toggle fullscreen");
                    ui.label("Tab: Toggle OSD");
                    ui.label("Esc: Exit fullscreen");
                    ui.separator();
                    ui.label("Mouse");
                    ui.label("Drag & drop: Open media");
                });
        }

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
            if i.key_pressed(egui::Key::F) || (i.key_pressed(egui::Key::Enter) && i.modifiers.alt) {
                self.toggle_fullscreen(ctx);
            }
            if i.key_pressed(egui::Key::Escape) && self.is_fullscreen {
                self.toggle_fullscreen(ctx);
            }
            if i.key_pressed(egui::Key::ArrowRight) {
                self.seek(self.current_time_ms.saturating_add(5000));
            }
            if i.key_pressed(egui::Key::ArrowLeft) {
                self.seek(self.current_time_ms.saturating_sub(5000));
            }
            if i.key_pressed(egui::Key::ArrowUp) {
                self.set_volume(self.volume + 0.05);
            }
            if i.key_pressed(egui::Key::ArrowDown) {
                self.set_volume(self.volume - 0.05);
            }

            // Mouse wheel: volume (default) or seek (with Shift)
            // Try both scroll_delta and smooth_scroll_delta for compatibility
            let scroll = if i.raw_scroll_delta.y != 0.0 {
                i.raw_scroll_delta.y
            } else {
                i.smooth_scroll_delta.y
            };

            if scroll.abs() > 0.1 {
                if i.modifiers.shift {
                    // Shift + wheel = seek (5 seconds per notch)
                    let seek_amount = if scroll > 0.0 { 5000i64 } else { -5000i64 };
                    if seek_amount > 0 {
                        self.seek(self.current_time_ms.saturating_add(seek_amount as u64));
                    } else {
                        self.seek(self.current_time_ms.saturating_sub((-seek_amount) as u64));
                    }
                } else {
                    // Wheel = volume (5% per notch)
                    let vol_change = if scroll > 0.0 { 0.05 } else { -0.05 };
                    self.set_volume(self.volume + vol_change);
                }
            }
        });

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

    let stats =
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

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--headless" | "headless" => {
                i += 1;
            }
            "--input" | "-i" => {
                let value = args
                    .get(i + 1)
                    .ok_or_else(|| anyhow::anyhow!("Missing value for --input"))?;
                input = Some(PathBuf::from(value));
                i += 2;
            }
            "--frames" | "-n" => {
                let value = args
                    .get(i + 1)
                    .ok_or_else(|| anyhow::anyhow!("Missing value for --frames"))?;
                frames = value
                    .parse::<u64>()
                    .map_err(|e| anyhow::anyhow!("Invalid frame count {}: {}", value, e))?;
                i += 2;
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

    Ok(HeadlessOptions { input, frames })
}

fn print_headless_usage() {
    eprintln!("\nHeadless playback usage:\n  slain --headless --input <file> [--frames <n>]\n");
}

// ============================================================================
// Decode Thread
// ============================================================================

struct HeadlessStats {
    decoded_frames: u64,
    duration_ms: u64,
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

                let mut src_frame =
                    PxVideoFrame::new(decoded.width as usize, decoded.height as usize, src_format);
                src_frame.data = decoded.data;

                let mut dst_frame = PxVideoFrame::new(
                    decoded.width as usize,
                    decoded.height as usize,
                    PxFormat::RGB24,
                );

                if let Some(ref conv) = converter {
                    conv.convert(&src_frame, &mut dst_frame)
                        .map_err(|e| format!("Pixel convert error: {}", e))?;
                }

                let pts_ms = if packet.pts > 0 {
                    (packet.pts as u64) / 1000
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

                let mut src_frame =
                    PxVideoFrame::new(decoded.width as usize, decoded.height as usize, src_format);
                src_frame.data = decoded.data;

                let mut dst_frame = PxVideoFrame::new(
                    decoded.width as usize,
                    decoded.height as usize,
                    PxFormat::RGB24,
                );

                if let Some(ref conv) = converter {
                    conv.convert(&src_frame, &mut dst_frame)
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

/// Main decode loop - runs in separate thread
fn decode_loop(shared: Arc<PlaybackShared>, path: PathBuf, width: u32, height: u32) {
    tracing::info!("Decode thread started for {:?}", path);

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let result = match ext.as_str() {
        "mp4" | "m4v" | "mov" => decode_mp4(shared.clone(), &path, width, height),
        "mkv" | "webm" => decode_mkv(shared.clone(), &path, width, height),
        "avi" => decode_avi(shared.clone(), &path, width, height),
        "ts" | "mts" | "m2ts" => decode_ts(shared.clone(), &path, width, height),
        _ => Err(format!("Unsupported container: {}", ext)),
    };

    if let Err(e) = result {
        tracing::error!("Decode failed: {}", e);
    }

    tracing::info!("Decode thread finished");
}

/// MKV decoding using MkvDemuxer + hw_decode + pixel_convert
fn decode_mkv(
    shared: Arc<PlaybackShared>,
    path: &PathBuf,
    width: u32,
    height: u32,
) -> Result<(), String> {
    use std::fs::File;
    use std::io::BufReader;

    let mut parser = MkvParser::new();
    let info = parser.parse(path)?;

    let file = File::open(path).map_err(|e| format!("Open error: {}", e))?;
    let reader = BufReader::new(file);

    let mut demuxer = MkvDemuxer::new(reader, info.clone())?;

    let video_track_num = match demuxer.video_track() {
        Some(t) => t,
        None => {
            tracing::error!("No video track found in MKV");
            return Err("No video track found".into());
        }
    };

    // Get video track info including codec_private
    let (vid_w, vid_h, codec_id, codec_private) = info
        .tracks
        .iter()
        .find_map(|t| {
            if let MkvTrack::Video(v) = t {
                Some((
                    v.pixel_width,
                    v.pixel_height,
                    v.codec_id.clone(),
                    v.codec_private.clone(),
                ))
            } else {
                None
            }
        })
        .unwrap_or((width, height, String::new(), None));

    tracing::info!(
        "MKV demuxer ready: {}x{}, video track {}, codec_private: {} bytes",
        vid_w,
        vid_h,
        video_track_num,
        codec_private.as_ref().map(|d| d.len()).unwrap_or(0)
    );

    // Parse AVCC extradata to get SPS/PPS and NAL length size
    let (sps_pps_data, nal_length_size) = if let Some(ref extra) = codec_private {
        if let Some((data, size)) = parse_avcc_extradata(extra) {
            tracing::info!(
                "Parsed AVCC: {} bytes SPS/PPS, nal_length_size={}",
                data.len(),
                size
            );
            (Some(data), size)
        } else {
            tracing::warn!("Failed to parse AVCC extradata");
            (None, 4)
        }
    } else {
        tracing::warn!("No codec_private data in MKV");
        (None, 4)
    };

    let codec = mkv_codec_to_hwcodec(&codec_id)?;

    // Prefer NVDEC if available
    let preferred = if slain_core::nvdec::nvdec_available() {
        Some(HwDecoderType::Nvdec)
    } else {
        None
    };

    let config = DecoderConfig {
        codec,
        width: vid_w,
        height: vid_h,
        preferred_backend: preferred,
        allow_software_fallback: true,
        extra_data: codec_private.clone(),
    };

    let mut decoder = HwDecoder::new(config)?;
    tracing::info!("MKV decoder created: backend={:?}", decoder.backend());

    // Feed SPS/PPS first if we have it
    let mut sps_pps_sent = false;
    if let Some(ref data) = sps_pps_data {
        tracing::info!("Feeding SPS/PPS ({} bytes) to decoder", data.len());
        match decoder.decode(data, 0) {
            Ok(_) => {
                sps_pps_sent = true;
                tracing::info!("SPS/PPS fed successfully");
            }
            Err(e) => tracing::warn!("SPS/PPS feed error (may be ok): {}", e),
        }
    }

    let mut frame_number: u64 = 0;
    let mut packets_fed: u64 = 0;

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
            let _ = demuxer.seek(target);
            shared.seek_requested.store(false, Ordering::SeqCst);
            shared.frame_queue.lock().clear();
            // Re-send SPS/PPS after seek
            if let Some(ref data) = sps_pps_data {
                let _ = decoder.decode(data, 0);
            }
        }

        match demuxer.read_packet() {
            Some(packet) => {
                if packet.track_number != video_track_num {
                    continue;
                }

                packets_fed += 1;

                // Convert AVCC to Annex B if needed
                let decode_data = if is_annexb(&packet.data) {
                    // Already Annex B (e.g., from TS container)
                    packet.data.clone()
                } else {
                    // Convert from AVCC
                    avcc_to_annexb(&packet.data, nal_length_size)
                };

                if packets_fed <= 5 || packets_fed % 100 == 0 {
                    tracing::info!(
                        "MKV packet {}: {} bytes -> {} bytes (annexb), pts={}",
                        packets_fed,
                        packet.data.len(),
                        decode_data.len(),
                        packet.pts_ms
                    );
                }

                match decoder.decode(&decode_data, packet.pts_ms) {
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

                        let pts_ms = packet.pts_ms.max(0) as u64;
                        shared.current_time_ms.store(pts_ms, Ordering::SeqCst);

                        shared.frame_queue.lock().push_back(RgbFrame {
                            data: dst_frame.data,
                            width: decoded.width,
                            height: decoded.height,
                            pts_ms,
                        });

                        frame_number += 1;
                        if frame_number <= 5 || frame_number % 100 == 0 {
                            tracing::info!(
                                "MKV frame {} decoded: {}x{}",
                                frame_number,
                                decoded.width,
                                decoded.height
                            );
                        }
                    }
                    Ok(None) => {
                        if packets_fed <= 20 {
                            tracing::info!(
                                "MKV decoder buffering packet {} (no output yet)",
                                packets_fed
                            );
                        }
                    }
                    Err(e) => {
                        tracing::error!("MKV decode error on packet {}: {}", packets_fed, e);
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
fn decode_avi(
    shared: Arc<PlaybackShared>,
    path: &PathBuf,
    width: u32,
    height: u32,
) -> Result<(), String> {
    use std::fs::File;
    use std::io::BufReader;

    let file = File::open(path).map_err(|e| format!("Open error: {}", e))?;
    let reader = BufReader::new(file);

    let mut demuxer = AviDemuxer::new(reader)?;

    // Clone the values we need to avoid borrow conflicts
    let (video_stream_index, vid_w, vid_h, codec) = {
        let info = demuxer.info();
        let video_stream = info
            .streams
            .iter()
            .find(|stream| matches!(stream.stream_type, slain_core::avi_demux::StreamType::Video))
            .ok_or_else(|| "No video stream found in AVI".to_string())?;

        let vid_w = video_stream.width.unwrap_or(width);
        let vid_h = video_stream.height.unwrap_or(height);
        let codec = match video_stream.codec {
            slain_core::avi_demux::CodecType::H264 => HwCodec::H264,
            other => {
                return Err(format!("Unsupported AVI codec: {:?}", other));
            }
        };
        (video_stream.index, vid_w, vid_h, codec)
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
                if packet.stream_index != video_stream_index {
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
fn decode_ts(
    shared: Arc<PlaybackShared>,
    path: &PathBuf,
    width: u32,
    height: u32,
) -> Result<(), String> {
    use std::fs::File;
    use std::io::BufReader;

    let file = File::open(path).map_err(|e| format!("Open error: {}", e))?;
    let reader = BufReader::new(file);

    let mut demuxer = TsDemuxer::new(reader)?;

    // Clone the values we need to avoid borrow conflicts
    let (video_pid, codec) = {
        let info = demuxer.info();
        let video_stream = info
            .streams
            .iter()
            .find(|stream| {
                matches!(
                    stream.codec,
                    TsStreamCodec::H264
                        | TsStreamCodec::H265
                        | TsStreamCodec::MPEG2Video
                        | TsStreamCodec::MPEG1Video
                )
            })
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
        (video_stream.pid, codec)
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
                if packet.pid != video_pid {
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
fn decode_mp4(
    shared: Arc<PlaybackShared>,
    path: &PathBuf,
    width: u32,
    height: u32,
) -> Result<(), String> {
    use std::fs::File;
    use std::io::BufReader;

    let file = File::open(path).map_err(|e| format!("Open error: {}", e))?;
    let reader = BufReader::new(file);

    let mut demuxer = Mp4Demuxer::new(reader).map_err(|e| format!("Demux init: {}", e))?;

    let streams = demuxer.streams();
    let (video_idx, video_info) = match streams
        .iter()
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

    let (vid_w, vid_h) = if let Some(vi) = demuxer.video_info(video_idx) {
        (vi.width, vi.height)
    } else {
        (width, height)
    };

    let codec = mp4_codec_to_hwcodec(&video_info.codec)?;
    let config = DecoderConfig {
        codec,
        width: vid_w,
        height: vid_h,
        preferred_backend: None,
        allow_software_fallback: true,
        extra_data: Some(video_info.extra_data.clone()),
    };

    let mut decoder = HwDecoder::new(config)?;

    tracing::info!(
        "MP4 decode ready: {}x{}, backend={:?}",
        vid_w,
        vid_h,
        decoder.backend()
    );

    let mut frame_number: u64 = 0;
    let mut packets_fed: u64 = 0;

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
            let _target = shared.seek_target_ms.load(Ordering::SeqCst);
            shared.seek_requested.store(false, Ordering::SeqCst);
            shared.frame_queue.lock().clear();
        }

        match demuxer.read_packet() {
            Some(packet) => {
                if packet.stream_index != video_idx as u32 {
                    continue;
                }

                packets_fed += 1;
                if packets_fed <= 5 || packets_fed % 100 == 0 {
                    tracing::info!(
                        "Feeding packet {}: {} bytes, pts={}",
                        packets_fed,
                        packet.data.len(),
                        packet.pts
                    );
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
                        if frame_number <= 5 || frame_number % 100 == 0 {
                            tracing::info!(
                                "MP4 frame {} decoded: {}x{}",
                                frame_number,
                                decoded.width,
                                decoded.height
                            );
                        }
                    }
                    Ok(None) => {
                        if packets_fed <= 20 {
                            tracing::info!(
                                "MP4 decoder buffering packet {} (no output yet)",
                                packets_fed
                            );
                        }
                    }
                    Err(e) => {
                        tracing::error!("MP4 decode error on packet {}: {}", packets_fed, e);
                    }
                }

                thread::sleep(Duration::from_millis(16));
            }
            None => {
                tracing::info!("End of file");
                break;
            }
        }
    }

    Ok(())
}
