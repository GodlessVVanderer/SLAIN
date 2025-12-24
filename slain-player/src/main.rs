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
use parking_lot::Mutex;

// Import from our core library - NOT rewriting
use slain_core::mkv::{MkvParser, MkvInfo, MkvTrack, MkvDemuxer, MkvPacket};
use slain_core::mp4_demux::{Packet, mp4::Mp4Demuxer};
use slain_core::audio::AudioPlayer;
use slain_core::hw_decode::{find_best_decoder, available_decoders, HwCodec, HwDecoder, HwDecoderType, DecodedFrame, DecoderConfig};
use slain_core::pixel_convert::{PixelConverter, VideoFrame as PxVideoFrame, PixelFormat as PxFormat, ColorSpace};
use slain_core::bandwidth::{window_monitor, AttentionState};
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

fn main() -> Result<()> {
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
        Box::new(|cc| Ok(Box::new(SlainApp::new(cc)))),
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
    
    // Pipeline selection
    pipeline: PipelineKind,
    pipeline_manager: Option<PipelineManager>,
    
    // Display
    video_texture: Option<TextureHandle>,
    frame_width: u32,
    frame_height: u32,
    last_frame_time: Instant,
    
    // UI state
    show_osd: bool,
    is_fullscreen: bool,
    show_settings: bool,
    
    // Stats
    fps: f32,
    frame_count: u64,
    dropped_frames: u32,
    decoder_name: String,
}

impl SlainApp {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
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
            pipeline: PipelineKind::SoftwareOnly,
            pipeline_manager: None, // Lazy init on file load
            video_texture: None,
            frame_width: 1920,
            frame_height: 1080,
            last_frame_time: Instant::now(),
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
    
    /// Open a media file using slain-core parsers
    fn open_file(&mut self, path: PathBuf) {
        tracing::info!("Opening: {:?}", path);
        
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
                        let codec = match v.codec_id.as_str() {
                            "V_MPEG4/ISO/AVC" => Some(HwCodec::H264),
                            "V_MPEGH/ISO/HEVC" => Some(HwCodec::H265),
                            "V_VP9" => Some(HwCodec::VP9),
                            "V_AV1" => Some(HwCodec::AV1),
                            _ => None,
                        };
                        
                        if let Some(c) = codec {
                            if let Some(dec) = find_best_decoder(c) {
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
                
                self.decode_thread = Some(thread::spawn(move || {
                    decode_loop(shared, video_path, width, height);
                }));
                
                // Start audio playback - lazy init audio player
                if self.audio_player.is_none() {
                    self.audio_player = Some(AudioPlayer::new());
                }
                if let Some(ref mut player) = self.audio_player {
                    if let Err(e) = player.play_file(path) {
                        tracing::warn!("Audio failed: {}", e);
                    }
                }
                
                self.playback_state = PlaybackState::Ready;
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
        
        // Use MP4 demuxer from slain-core
        tracing::info!("Opening MP4: {:?}", path);
        // TODO: Wire up mp4_demux module fully
        
        // For now, try audio
        if self.audio_player.is_none() {
            self.audio_player = Some(AudioPlayer::new());
        }
        if let Some(ref mut player) = self.audio_player {
            if let Err(e) = player.play_file(path) {
                tracing::warn!("Audio failed: {}", e);
            }
        }
        
        // MP4 stub - set ready for now
        self.playback_state = PlaybackState::Ready;
    }
    
    fn open_avi(&mut self, path: &PathBuf) {
        self.playback_state = PlaybackState::Loading;
        
        tracing::info!("Opening AVI: {:?}", path);
        // TODO: Wire up avi_demux module
        
        if self.audio_player.is_none() {
            self.audio_player = Some(AudioPlayer::new());
        }
        if let Some(ref mut player) = self.audio_player {
            if let Err(e) = player.play_file(path) {
                tracing::warn!("Audio failed: {}", e);
            }
        }
        
        self.playback_state = PlaybackState::Ready;
    }
    
    fn open_ts(&mut self, path: &PathBuf) {
        self.playback_state = PlaybackState::Loading;
        
        tracing::info!("Opening TS: {:?}", path);
        // TODO: Wire up ts_demux module
        
        if self.audio_player.is_none() {
            self.audio_player = Some(AudioPlayer::new());
        }
        if let Some(ref mut player) = self.audio_player {
            if let Err(e) = player.play_file(path) {
                tracing::warn!("Audio failed: {}", e);
            }
        }
        
        self.playback_state = PlaybackState::Ready;
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
        
        // TODO: Seek in audio
    }
    
    fn set_volume(&mut self, vol: f32) {
        self.volume = vol.clamp(0.0, 1.0);
        // TODO: Set audio volume
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
            
            // Sync state from shared
            self.current_time_ms = self.shared.current_time_ms.load(Ordering::Relaxed);
        }
        
        // Pull frame from queue and upload to texture
        if let Some(frame) = self.shared.frame_queue.lock().pop_front() {
            // Calculate FPS
            let now = Instant::now();
            let delta = now.duration_since(self.last_frame_time);
            if delta.as_secs_f32() > 0.0 {
                self.fps = 1.0 / delta.as_secs_f32();
            }
            self.last_frame_time = now;
            self.frame_count += 1;
            
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
        
        // Menu bar
        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
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
                    ui.add(egui::Slider::new(&mut self.volume, 0.0..=1.0).show_value(false));
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
        
        // Bottom controls
        egui::TopBottomPanel::bottom("controls")
            .min_height(60.0)
            .show(ctx, |ui| {
                ui.add_space(8.0);
                
                // Seek bar
                let mut time_sec = self.current_time_ms as f64 / 1000.0;
                let duration_sec = self.duration_ms as f64 / 1000.0;
                
                let slider = egui::Slider::new(&mut time_sec, 0.0..=duration_sec.max(1.0))
                    .show_value(false)
                    .trailing_fill(true);
                    
                if ui.add(slider).changed() {
                    self.seek((time_sec * 1000.0) as u64);
                }
                
                ui.horizontal(|ui| {
                    // Play/Pause
                    let icon = if self.is_playing() { "⏸" } else { "▶" };
                    if ui.button(egui::RichText::new(icon).size(20.0)).clicked() {
                        self.toggle_play();
                    }
                    
                    // Stop
                    if ui.button(egui::RichText::new("⏹").size(20.0)).clicked() {
                        self.playback_state = PlaybackState::Ready;
                        self.shared.is_playing.store(false, Ordering::SeqCst);
                        self.current_time_ms = 0;
                    }
                    
                    // Time display
                    ui.label(format!(
                        "{} / {}",
                        format_time(self.current_time_ms),
                        format_time(self.duration_ms)
                    ));
                    
                    // Right side controls
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // Fullscreen
                        if ui.button(egui::RichText::new("⛶").size(18.0)).clicked() {
                            self.toggle_fullscreen(ctx);
                        }
                        
                        // Volume slider
                        ui.add(
                            egui::Slider::new(&mut self.volume, 0.0..=1.0)
                                .show_value(false)
                                .fixed_decimals(0)
                        );
                        
                        // Volume icon
                        let vol_icon = if self.volume == 0.0 { "🔇" } 
                            else if self.volume < 0.5 { "🔉" } 
                            else { "🔊" };
                        ui.label(vol_icon);
                    });
                });
                
                ui.add_space(4.0);
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
// Decode Thread
// ============================================================================

/// Main decode loop - runs in separate thread
/// Reads packets from demuxer → decodes → converts to RGB → pushes to queue
fn decode_loop(shared: Arc<PlaybackShared>, path: PathBuf, width: u32, height: u32) {
    tracing::info!("Decode thread started for {:?}", path);
    
    let ext = path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    
    let result = match ext.as_str() {
        "mp4" | "m4v" | "mov" => decode_mp4(shared.clone(), &path, width, height),
        "mkv" | "webm" => decode_mkv(shared.clone(), &path, width, height),
        _ => {
            tracing::info!("Unknown format {:?}, using test pattern", ext);
            Ok(()) // Will fall through to test pattern
        }
    };
    
    if let Err(e) = result {
        tracing::error!("Decode failed: {}, falling back to test pattern", e);
        decode_test_pattern(shared, width, height);
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
    let (vid_w, vid_h) = info.tracks.iter()
        .find_map(|t| {
            if let MkvTrack::Video(v) = t {
                Some((v.pixel_width, v.pixel_height))
            } else {
                None
            }
        })
        .unwrap_or((width, height));
    
    tracing::info!("MKV demuxer ready: {}x{}, video track {}", vid_w, vid_h, video_track_num);
    
    // Create hardware decoder (tries NVDEC → AMF → VAAPI → Software)
    let config = DecoderConfig {
        codec: HwCodec::H264, // TODO: Detect from codec_id
        width: vid_w,
        height: vid_h,
        preferred_backend: Some(HwDecoderType::Software),
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
                tracing::info!("End of MKV file");
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
    let config = DecoderConfig {
        codec: HwCodec::H264, // Assume H.264 for now
        width: vid_w,
        height: vid_h,
        preferred_backend: Some(HwDecoderType::Software),
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
                        
                        // Calculate PTS in milliseconds
                        let pts_ms = if packet.pts > 0 {
                            (packet.pts as u64 * 1000) / 90000 // Assume 90kHz timescale
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
