// Pure Rust Audio Pipeline
// symphonia (decode) + cpal (output)
// No C dependencies - 100% Rust

use std::fs::File;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use ringbuf::traits::Observer;
use symphonia::core::audio::{AudioBufferRef, Signal, SampleBuffer};
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::units::Time;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, Stream, StreamConfig};

use ringbuf::HeapRb;
use ringbuf::traits::{Consumer, Producer, Split};
use serde::{Deserialize, Serialize};


// ============================================================================
// Types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioInfo {
    pub codec: String,
    pub sample_rate: u32,
    pub channels: u32,
    pub bits_per_sample: Option<u32>,
    pub duration_secs: Option<f64>,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub track_number: Option<u32>,
    pub genre: Option<String>,
    pub year: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioDevice {
    pub name: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlaybackState {
    Stopped,
    Playing,
    Paused,
}

// AudioPlayer state that is Send + Sync
pub struct AudioPlayerState {
    // Playback state
    state: Arc<Mutex<PlaybackState>>,
    playing: Arc<AtomicBool>,
    position_samples: Arc<AtomicU64>,
    sample_rate: Arc<AtomicU64>,

    // Volume (0.0 - 1.0)
    volume: Arc<Mutex<f32>>,
}

// Full player with stream (not Send due to cpal::Stream)
pub struct AudioPlayer {
    inner: AudioPlayerState,

    // Audio stream (not Send/Sync - kept out of global static)
    stream: Option<Stream>,

    // Ring buffer for decoded audio
    producer: Option<ringbuf::HeapProd<f32>>,

    // Decode thread handle
    decode_thread: Option<thread::JoinHandle<()>>,
}

// ============================================================================
// Audio Info Extraction
// ============================================================================

pub fn get_audio_info<P: AsRef<Path>>(path: P) -> Result<AudioInfo, String> {
    let path = path.as_ref();
    
    let file = File::open(path)
        .map_err(|e| format!("Failed to open file: {}", e))?;
    
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    
    let meta_opts = MetadataOptions::default();
    let fmt_opts = FormatOptions::default();
    
    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &fmt_opts, &meta_opts)
        .map_err(|e| format!("Failed to probe file: {}", e))?;
    
    let mut format = probed.format;
    
    // Find first audio track
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or("No audio track found")?;
    
    let params = &track.codec_params;
    
    let codec = symphonia::default::get_codecs()
        .get_codec(params.codec)
        .map(|c| c.short_name.to_string())
        .unwrap_or_else(|| format!("Unknown ({:?})", params.codec));
    
    let sample_rate = params.sample_rate.unwrap_or(44100);
    let channels = params.channels.map(|c| c.count() as u32).unwrap_or(2);
    let bits_per_sample = params.bits_per_sample;
    
    // Calculate duration
    let duration_secs = if let Some(n_frames) = params.n_frames {
        Some(n_frames as f64 / sample_rate as f64)
    } else {
        None
    };
    
    // Extract metadata
    let mut title = None;
    let mut artist = None;
    let mut album = None;
    let mut track_number = None;
    let mut genre = None;
    let mut year = None;
    
    if let Some(metadata) = format.metadata().current() {
        for tag in metadata.tags() {
            match tag.std_key {
                Some(symphonia::core::meta::StandardTagKey::TrackTitle) => {
                    title = Some(tag.value.to_string());
                }
                Some(symphonia::core::meta::StandardTagKey::Artist) => {
                    artist = Some(tag.value.to_string());
                }
                Some(symphonia::core::meta::StandardTagKey::Album) => {
                    album = Some(tag.value.to_string());
                }
                Some(symphonia::core::meta::StandardTagKey::TrackNumber) => {
                    track_number = tag.value.to_string().parse().ok();
                }
                Some(symphonia::core::meta::StandardTagKey::Genre) => {
                    genre = Some(tag.value.to_string());
                }
                Some(symphonia::core::meta::StandardTagKey::Date) => {
                    // Try to parse year from date
                    let date_str = tag.value.to_string();
                    if let Some(y) = date_str.split('-').next() {
                        year = y.parse().ok();
                    }
                }
                _ => {}
            }
        }
    }
    
    Ok(AudioInfo {
        codec,
        sample_rate,
        channels,
        bits_per_sample,
        duration_secs,
        title,
        artist,
        album,
        track_number,
        genre,
        year,
    })
}

// ============================================================================
// Audio Device Enumeration
// ============================================================================

pub fn list_audio_devices() -> Result<Vec<AudioDevice>, String> {
    let host = cpal::default_host();
    let default_device = host.default_output_device();
    let default_name = default_device.as_ref().and_then(|d| d.name().ok());
    
    let devices = host.output_devices()
        .map_err(|e| format!("Failed to enumerate devices: {}", e))?;
    
    let mut result = Vec::new();
    for device in devices {
        if let Ok(name) = device.name() {
            let is_default = default_name.as_ref().map(|d| d == &name).unwrap_or(false);
            result.push(AudioDevice { name, is_default });
        }
    }
    
    Ok(result)
}

pub fn get_default_device() -> Result<Device, String> {
    cpal::default_host()
        .default_output_device()
        .ok_or_else(|| "No default output device found".to_string())
}

// ============================================================================
// Audio Player Implementation
// ============================================================================

impl AudioPlayer {
    pub fn new() -> Self {
        Self {
            inner: AudioPlayerState {
                state: Arc::new(Mutex::new(PlaybackState::Stopped)),
                playing: Arc::new(AtomicBool::new(false)),
                position_samples: Arc::new(AtomicU64::new(0)),
                sample_rate: Arc::new(AtomicU64::new(44100)),
                volume: Arc::new(Mutex::new(1.0)),
            },
            stream: None,
            producer: None,
            decode_thread: None,
        }
    }
    
    pub fn play_file<P: AsRef<Path>>(&mut self, path: P) -> Result<(), String> {
        self.stop();

        let path = path.as_ref().to_path_buf();

        // Get audio info first
        let info = get_audio_info(&path)?;
        self.inner.sample_rate.store(info.sample_rate as u64, Ordering::SeqCst);

        // Set up output device
        let device = get_default_device()?;
        let config = device.default_output_config()
            .map_err(|e| format!("Failed to get output config: {}", e))?;

        // Create ring buffer (2 seconds of audio)
        let buffer_size = info.sample_rate as usize * info.channels as usize * 2;
        let ring = HeapRb::<f32>::new(buffer_size);
        let (producer, consumer) = ring.split();

        self.producer = Some(producer);

        // Create output stream
        let stream = self.create_output_stream(device, config, consumer)?;
        stream.play().map_err(|e| format!("Failed to start stream: {}", e))?;
        self.stream = Some(stream);

        // Start decode thread
        let playing = self.inner.playing.clone();
        let position = self.inner.position_samples.clone();
        let mut producer = self.producer.take().unwrap();

        playing.store(true, Ordering::SeqCst);

        let decode_handle = thread::spawn(move || {
            if let Err(e) = decode_audio_to_buffer(path, &mut producer, &playing, &position) {
                eprintln!("Decode error: {}", e);
            }
        });

        self.decode_thread = Some(decode_handle);
        *self.inner.state.lock().unwrap() = PlaybackState::Playing;

        Ok(())
    }
    
    fn create_output_stream(
        &self,
        device: Device,
        config: cpal::SupportedStreamConfig,
        mut consumer: ringbuf::HeapCons<f32>,
    ) -> Result<Stream, String> {
        let volume = self.inner.volume.clone();
        let sample_format = config.sample_format();
        let config: StreamConfig = config.into();
        
        let err_fn = |err| eprintln!("Audio stream error: {}", err);
        
        let stream = match sample_format {
            SampleFormat::F32 => device.build_output_stream(
                &config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let vol = *volume.lock().unwrap();
                    for sample in data.iter_mut() {
                        *sample = consumer.try_pop().unwrap_or(0.0) * vol;
                    }
                },
                err_fn,
                None,
            ),
            SampleFormat::I16 => device.build_output_stream(
                &config,
                move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                    let vol = *volume.lock().unwrap();
                    for sample in data.iter_mut() {
                        let s = consumer.try_pop().unwrap_or(0.0) * vol;
                        *sample = (s * i16::MAX as f32) as i16;
                    }
                },
                err_fn,
                None,
            ),
            SampleFormat::U16 => device.build_output_stream(
                &config,
                move |data: &mut [u16], _: &cpal::OutputCallbackInfo| {
                    let vol = *volume.lock().unwrap();
                    for sample in data.iter_mut() {
                        let s = consumer.try_pop().unwrap_or(0.0) * vol;
                        *sample = ((s + 1.0) * 0.5 * u16::MAX as f32) as u16;
                    }
                },
                err_fn,
                None,
            ),
            _ => return Err("Unsupported sample format".to_string()),
        };
        
        stream.map_err(|e| format!("Failed to build stream: {}", e))
    }
    
    pub fn pause(&mut self) {
        self.inner.playing.store(false, Ordering::SeqCst);
        if let Some(stream) = &self.stream {
            let _ = stream.pause();
        }
        *self.inner.state.lock().unwrap() = PlaybackState::Paused;
    }

    pub fn resume(&mut self) {
        self.inner.playing.store(true, Ordering::SeqCst);
        if let Some(stream) = &self.stream {
            let _ = stream.play();
        }
        *self.inner.state.lock().unwrap() = PlaybackState::Playing;
    }

    pub fn stop(&mut self) {
        self.inner.playing.store(false, Ordering::SeqCst);

        // Stop stream
        if let Some(stream) = self.stream.take() {
            drop(stream);
        }

        // Wait for decode thread
        if let Some(handle) = self.decode_thread.take() {
            let _ = handle.join();
        }

        self.producer = None;
        self.inner.position_samples.store(0, Ordering::SeqCst);
        *self.inner.state.lock().unwrap() = PlaybackState::Stopped;
    }

    pub fn set_volume(&mut self, volume: f32) {
        *self.inner.volume.lock().unwrap() = volume.clamp(0.0, 1.0);
    }

    pub fn get_volume(&self) -> f32 {
        *self.inner.volume.lock().unwrap()
    }

    pub fn get_position_secs(&self) -> f64 {
        let samples = self.inner.position_samples.load(Ordering::SeqCst);
        let rate = self.inner.sample_rate.load(Ordering::SeqCst);
        if rate > 0 {
            samples as f64 / rate as f64
        } else {
            0.0
        }
    }

    pub fn get_state(&self) -> PlaybackState {
        *self.inner.state.lock().unwrap()
    }

    pub fn is_playing(&self) -> bool {
        self.inner.playing.load(Ordering::SeqCst)
    }
}

impl Drop for AudioPlayer {
    fn drop(&mut self) {
        self.stop();
    }
}

// ============================================================================
// Decode Thread
// ============================================================================

fn decode_audio_to_buffer(
    path: std::path::PathBuf,
    producer: &mut ringbuf::HeapProd<f32>,
    playing: &AtomicBool,
    position: &AtomicU64,
) -> Result<(), String> {
    let file = File::open(&path)
        .map_err(|e| format!("Failed to open file: {}", e))?;
    
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    
    let meta_opts = MetadataOptions::default();
    let fmt_opts = FormatOptions::default();
    
    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &fmt_opts, &meta_opts)
        .map_err(|e| format!("Failed to probe file: {}", e))?;
    
    let mut format = probed.format;
    
    // Find audio track
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or("No audio track found")?;
    
    let track_id = track.id;
    let dec_opts = DecoderOptions::default();
    
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &dec_opts)
        .map_err(|e| format!("Failed to create decoder: {}", e))?;
    
    // Get channel count - default to stereo if not specified in codec params
    let channels = decoder.codec_params().channels
        .map(|c| c.count())
        .unwrap_or(2);  // Default to stereo
    
    // Decode loop
    let mut sample_buf: Option<SampleBuffer<f32>> = None;
    let mut samples_decoded: u64 = 0;
    
    loop {
        // Check if we should stop
        if !playing.load(Ordering::SeqCst) {
            // Wait a bit when paused instead of busy-looping
            thread::sleep(Duration::from_millis(50));
            continue;
        }
        
        // Read next packet
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                break; // End of file
            }
            Err(e) => {
                eprintln!("Packet read error: {}", e);
                break;
            }
        };
        
        // Skip packets from other tracks
        if packet.track_id() != track_id {
            continue;
        }
        
        // Decode packet
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(SymphoniaError::DecodeError(e)) => {
                eprintln!("Decode error: {}", e);
                continue;
            }
            Err(e) => {
                eprintln!("Fatal decode error: {}", e);
                break;
            }
        };
        
        // Convert to f32 samples
        if sample_buf.is_none() {
            let spec = *decoded.spec();
            let duration = decoded.capacity() as u64;
            sample_buf = Some(SampleBuffer::new(duration, spec));
        }
        
        if let Some(buf) = &mut sample_buf {
            buf.copy_interleaved_ref(decoded);
            
            let samples = buf.samples();
            
            // Push to ring buffer
            for &sample in samples {
                // Busy wait if buffer is full
                while producer.is_full() {
                    if !playing.load(Ordering::SeqCst) {
                        return Ok(());
                    }
                    thread::sleep(Duration::from_micros(100));
                }
                let _ = producer.try_push(sample);
            }
            
            samples_decoded += (samples.len() / channels) as u64;
            position.store(samples_decoded, Ordering::SeqCst);
        }
    }
    
    Ok(())
}

// ============================================================================
// Seek Support
// ============================================================================

pub fn seek_audio<P: AsRef<Path>>(
    path: P,
    seek_time_secs: f64,
) -> Result<u64, String> {
    let path = path.as_ref();
    
    let file = File::open(path)
        .map_err(|e| format!("Failed to open file: {}", e))?;
    
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    
    let meta_opts = MetadataOptions::default();
    let fmt_opts = FormatOptions::default();
    
    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &fmt_opts, &meta_opts)
        .map_err(|e| format!("Failed to probe file: {}", e))?;
    
    let mut format = probed.format;
    
    let seek_to = SeekTo::Time {
        time: Time::from(seek_time_secs),
        track_id: None,
    };
    
    let seeked_to = format.seek(SeekMode::Coarse, seek_to)
        .map_err(|e| format!("Seek failed: {}", e))?;
    
    Ok(seeked_to.actual_ts)
}

// ============================================================================
// Global Player Instance
// ============================================================================

use std::cell::RefCell;

// Use thread-local for AudioPlayer since cpal::Stream is not Send/Sync
thread_local! {
    static AUDIO_PLAYER: RefCell<AudioPlayer> = RefCell::new(AudioPlayer::new());
}

// ============================================================================
// Public API
// ============================================================================

pub fn audio_get_info(path: String) -> Result<AudioInfo, String> {
    get_audio_info(&path)
}

pub fn audio_list_devices() -> Result<Vec<AudioDevice>, String> {
    list_audio_devices()
}


pub fn audio_play(path: String) -> Result<(), String> {
    AUDIO_PLAYER.with(|cell| {
        cell.borrow_mut().play_file(&path)
    })
}

pub fn audio_pause() -> Result<(), String> {
    AUDIO_PLAYER.with(|cell| {
        cell.borrow_mut().pause();
        Ok(())
    })
}

pub fn audio_resume() -> Result<(), String> {
    AUDIO_PLAYER.with(|cell| {
        cell.borrow_mut().resume();
        Ok(())
    })
}

pub fn audio_stop() -> Result<(), String> {
    AUDIO_PLAYER.with(|cell| {
        cell.borrow_mut().stop();
        Ok(())
    })
}

pub fn audio_set_volume(volume: f32) -> Result<(), String> {
    AUDIO_PLAYER.with(|cell| {
        cell.borrow_mut().set_volume(volume);
        Ok(())
    })
}

pub fn audio_get_volume() -> Result<f32, String> {
    AUDIO_PLAYER.with(|cell| {
        Ok(cell.borrow().get_volume())
    })
}

pub fn audio_get_position() -> Result<f64, String> {
    AUDIO_PLAYER.with(|cell| {
        Ok(cell.borrow().get_position_secs())
    })
}

pub fn audio_is_playing() -> Result<bool, String> {
    AUDIO_PLAYER.with(|cell| {
        Ok(cell.borrow().is_playing())
    })
}

// ============================================================================
// Supported Formats
// ============================================================================

pub fn supported_audio_formats() -> Vec<&'static str> {
    vec![
        // Lossless
        "flac",
        "wav",
        "aiff",
        "alac",
        // Lossy
        "mp3",
        "aac",
        "m4a",
        "ogg",
        "opus",
        "vorbis",
        "wma",
        // Containers
        "mkv",
        "mka",
        "webm",
        "mp4",
    ]
}


pub fn audio_supported_formats() -> Vec<&'static str> {
    supported_audio_formats()
}
