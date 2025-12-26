// FFMPEG MINIMAL - Only What SLAIN Needs
//
// FFmpeg is 1.5 million lines of C. SLAIN needs maybe 5% of it.
//
// WHAT A VIDEO PLAYER ACTUALLY NEEDS:
// ════════════════════════════════════════════════════════════════════════════
//
// 1. CONTAINER DEMUXING (libavformat)
//    - MKV  → Already have mkv.rs (1540 lines)
//    - MP4  → Need to write
//    - AVI  → Need to write  
//    - WebM → MKV variant, already covered
//    - TS   → Need for IPTV
//
// 2. VIDEO DECODING (libavcodec)
//    - H.264/AVC  → Use hardware (NVDEC/VAAPI) or dav1d-style decoder
//    - H.265/HEVC → Use hardware or write decoder
//    - VP9        → Use hardware or libvpx wrapper
//    - AV1        → Use dav1d (already Rust-friendly)
//
// 3. AUDIO DECODING (libavcodec)
//    - Already using Symphonia in audio.rs
//    - Covers: MP3, AAC, FLAC, Vorbis, WAV, ALAC
//    - Need: AC3, DTS, TrueHD for Blu-ray
//
// 4. SUBTITLE PARSING
//    - Already have subtitles.rs
//    - Covers: SRT, ASS/SSA, VTT
//    - Need: PGS (Blu-ray), VobSub (DVD)
//
// 5. PIXEL FORMAT CONVERSION (libswscale)
//    - YUV to RGB conversion
//    - Scaling/resizing
//
// 6. AUDIO RESAMPLING (libswresample)
//    - Sample rate conversion
//    - Channel mixing
//
// ════════════════════════════════════════════════════════════════════════════
// THIS FILE: Container demuxing for MP4/AVI/TS
// ════════════════════════════════════════════════════════════════════════════

use std::io::{Read, Seek, SeekFrom};
use std::collections::HashMap;
use serde::{Deserialize, Serialize};

// ============================================================================
// Common Types (shared across containers)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CodecType {
    Video,
    Audio,
    Subtitle,
    Data,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VideoCodec {
    H264,
    H265,
    VP8,
    VP9,
    AV1,
    MPEG2,
    MPEG4,
    VC1,
    Theora,
    Unknown(u32),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AudioCodec {
    AAC,
    MP3,
    AC3,
    EAC3,
    DTS,
    DTSHD,
    TrueHD,
    FLAC,
    Vorbis,
    Opus,
    PCM,
    ALAC,
    Unknown(u32),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SubtitleCodec {
    SRT,
    ASS,
    VTT,
    PGS,      // Blu-ray
    VobSub,   // DVD
    DVBSub,
    Unknown(u32),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamInfo {
    pub index: u32,
    pub codec_type: CodecType,
    pub codec: CodecId,
    pub language: Option<String>,
    pub title: Option<String>,
    pub default: bool,
    pub forced: bool,
    pub extra_data: Vec<u8>,  // Codec-specific init data (SPS/PPS for H.264, etc)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CodecId {
    Video(VideoCodec),
    Audio(AudioCodec),
    Subtitle(SubtitleCodec),
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoInfo {
    pub width: u32,
    pub height: u32,
    pub fps_num: u32,
    pub fps_den: u32,
    pub pixel_format: PixelFormat,
    pub bit_depth: u8,
    pub color_space: ColorSpace,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum PixelFormat {
    YUV420P,
    YUV420P10,
    YUV422P,
    YUV444P,
    NV12,
    RGB24,
    RGBA,
    Unknown,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ColorSpace {
    BT601,
    BT709,
    BT2020,
    SRGB,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioInfo {
    pub sample_rate: u32,
    pub channels: u8,
    pub channel_layout: ChannelLayout,
    pub bits_per_sample: u8,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ChannelLayout {
    Mono,
    Stereo,
    Surround51,
    Surround71,
    Unknown(u8),
}

/// A packet of compressed data from the container
#[derive(Debug, Clone)]
pub struct Packet {
    pub stream_index: u32,
    pub pts: i64,           // Presentation timestamp
    pub dts: i64,           // Decode timestamp
    pub duration: i64,
    pub keyframe: bool,
    pub data: Vec<u8>,
}

// ============================================================================
// MP4/MOV Demuxer
// ============================================================================

pub mod mp4 {
    use super::*;

    /// MP4 atom/box types
    const FTYP: u32 = 0x66747970;  // ftyp
    const MOOV: u32 = 0x6D6F6F76;  // moov
    const MVHD: u32 = 0x6D766864;  // mvhd
    const TRAK: u32 = 0x7472616B;  // trak
    const TKHD: u32 = 0x746B6864;  // tkhd
    const MDIA: u32 = 0x6D646961;  // mdia
    const MDHD: u32 = 0x6D646864;  // mdhd
    const HDLR: u32 = 0x68646C72;  // hdlr
    const MINF: u32 = 0x6D696E66;  // minf
    const STBL: u32 = 0x7374626C;  // stbl
    const STSD: u32 = 0x73747364;  // stsd
    const STTS: u32 = 0x73747473;  // stts
    const STSC: u32 = 0x73747363;  // stsc
    const STSZ: u32 = 0x7374737A;  // stsz
    const STCO: u32 = 0x7374636F;  // stco
    const CO64: u32 = 0x636F3634;  // co64
    const STSS: u32 = 0x73747373;  // stss (keyframes)
    const CTTS: u32 = 0x63747473;  // ctts (composition time)
    const MDAT: u32 = 0x6D646174;  // mdat
    const EDTS: u32 = 0x65647473;  // edts
    const ELST: u32 = 0x656C7374;  // elst

    // Video codec atoms
    const AVC1: u32 = 0x61766331;  // avc1 (H.264)
    const HVC1: u32 = 0x68766331;  // hvc1 (HEVC)
    const HEV1: u32 = 0x68657631;  // hev1 (HEVC)
    const VP09: u32 = 0x76703039;  // vp09 (VP9)
    const AV01: u32 = 0x61763031;  // av01 (AV1)

    // Audio codec atoms
    const MP4A: u32 = 0x6D703461;  // mp4a (AAC)
    const AC3_: u32 = 0x61632D33;  // ac-3
    const EC3_: u32 = 0x65632D33;  // ec-3
    const FLAC: u32 = 0x664C6143;  // fLaC
    const OPUS: u32 = 0x4F707573;  // Opus

    #[derive(Debug)]
    pub struct Mp4Demuxer<R: Read + Seek> {
        reader: R,
        duration: u64,
        timescale: u32,
        tracks: Vec<Track>,
        mdat_offset: u64,
        mdat_size: u64,
    }

    #[derive(Debug, Clone)]
    struct Track {
        id: u32,
        stream_info: StreamInfo,
        video_info: Option<VideoInfo>,
        audio_info: Option<AudioInfo>,
        timescale: u32,
        duration: u64,
        sample_table: SampleTable,
        current_sample: usize,
    }

    #[derive(Debug, Clone, Default)]
    struct SampleTable {
        sample_sizes: Vec<u32>,
        chunk_offsets: Vec<u64>,
        sample_to_chunk: Vec<(u32, u32, u32)>,  // first_chunk, samples_per_chunk, sample_desc_index
        time_to_sample: Vec<(u32, u32)>,         // sample_count, sample_delta
        keyframes: Vec<u32>,                      // Sample numbers that are keyframes
        composition_offsets: Vec<(u32, i32)>,    // sample_count, offset
    }

    impl<R: Read + Seek> Mp4Demuxer<R> {
        pub fn new(reader: R) -> Result<Self, String> {
            let mut demuxer = Self {
                reader,
                duration: 0,
                timescale: 1000,
                tracks: Vec::new(),
                mdat_offset: 0,
                mdat_size: 0,
            };
            demuxer.parse_atoms()?;
            Ok(demuxer)
        }

        fn parse_atoms(&mut self) -> Result<(), String> {
            let file_size = self.reader.seek(SeekFrom::End(0))
                .map_err(|e| format!("Seek error: {}", e))?;
            self.reader.seek(SeekFrom::Start(0))
                .map_err(|e| format!("Seek error: {}", e))?;

            let mut pos = 0u64;
            while pos < file_size {
                let (size, atom_type) = self.read_atom_header()?;
                
                match atom_type {
                    FTYP => {
                        // File type - just skip for now
                        self.skip_bytes(size - 8)?;
                    }
                    MOOV => {
                        self.parse_moov(size - 8)?;
                    }
                    MDAT => {
                        self.mdat_offset = pos + 8;
                        self.mdat_size = size - 8;
                        self.skip_bytes(size - 8)?;
                    }
                    _ => {
                        self.skip_bytes(size - 8)?;
                    }
                }
                
                pos += size;
            }

            Ok(())
        }

        fn read_atom_header(&mut self) -> Result<(u64, u32), String> {
            let mut buf = [0u8; 8];
            self.reader.read_exact(&mut buf)
                .map_err(|e| format!("Read error: {}", e))?;
            
            let size = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]) as u64;
            let atom_type = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]);

            let actual_size = if size == 1 {
                // 64-bit size
                let mut buf64 = [0u8; 8];
                self.reader.read_exact(&mut buf64)
                    .map_err(|e| format!("Read error: {}", e))?;
                u64::from_be_bytes(buf64)
            } else if size == 0 {
                // Extends to end of file
                let current = self.reader.stream_position()
                    .map_err(|e| format!("Position error: {}", e))?;
                let end = self.reader.seek(SeekFrom::End(0))
                    .map_err(|e| format!("Seek error: {}", e))?;
                self.reader.seek(SeekFrom::Start(current))
                    .map_err(|e| format!("Seek error: {}", e))?;
                end - current + 8
            } else {
                size
            };

            Ok((actual_size, atom_type))
        }

        fn skip_bytes(&mut self, n: u64) -> Result<(), String> {
            self.reader.seek(SeekFrom::Current(n as i64))
                .map_err(|e| format!("Seek error: {}", e))?;
            Ok(())
        }

        fn read_u8(&mut self) -> Result<u8, String> {
            let mut buf = [0u8; 1];
            self.reader.read_exact(&mut buf)
                .map_err(|e| format!("Read error: {}", e))?;
            Ok(buf[0])
        }

        fn read_u16(&mut self) -> Result<u16, String> {
            let mut buf = [0u8; 2];
            self.reader.read_exact(&mut buf)
                .map_err(|e| format!("Read error: {}", e))?;
            Ok(u16::from_be_bytes(buf))
        }

        fn read_u32(&mut self) -> Result<u32, String> {
            let mut buf = [0u8; 4];
            self.reader.read_exact(&mut buf)
                .map_err(|e| format!("Read error: {}", e))?;
            Ok(u32::from_be_bytes(buf))
        }

        fn read_u64(&mut self) -> Result<u64, String> {
            let mut buf = [0u8; 8];
            self.reader.read_exact(&mut buf)
                .map_err(|e| format!("Read error: {}", e))?;
            Ok(u64::from_be_bytes(buf))
        }

        fn parse_moov(&mut self, size: u64) -> Result<(), String> {
            let end_pos = self.reader.stream_position()
                .map_err(|e| format!("Position error: {}", e))? + size;

            while self.reader.stream_position().unwrap_or(end_pos) < end_pos {
                let (atom_size, atom_type) = self.read_atom_header()?;
                
                match atom_type {
                    MVHD => self.parse_mvhd(atom_size - 8)?,
                    TRAK => self.parse_trak(atom_size - 8)?,
                    _ => self.skip_bytes(atom_size - 8)?,
                }
            }

            Ok(())
        }

        fn parse_mvhd(&mut self, size: u64) -> Result<(), String> {
            let version = self.read_u8()?;
            self.skip_bytes(3)?; // flags

            if version == 1 {
                self.skip_bytes(8)?;  // creation_time
                self.skip_bytes(8)?;  // modification_time
                self.timescale = self.read_u32()?;
                self.duration = self.read_u64()?;
            } else {
                self.skip_bytes(4)?;  // creation_time
                self.skip_bytes(4)?;  // modification_time
                self.timescale = self.read_u32()?;
                self.duration = self.read_u32()? as u64;
            }

            // Skip rest
            let remaining = if version == 1 { size - 28 } else { size - 16 };
            self.skip_bytes(remaining)?;

            Ok(())
        }

        fn parse_trak(&mut self, size: u64) -> Result<(), String> {
            let end_pos = self.reader.stream_position()
                .map_err(|e| format!("Position error: {}", e))? + size;

            let mut track = Track {
                id: self.tracks.len() as u32,
                stream_info: StreamInfo {
                    index: self.tracks.len() as u32,
                    codec_type: CodecType::Unknown,
                    codec: CodecId::Unknown,
                    language: None,
                    title: None,
                    default: false,
                    forced: false,
                    extra_data: Vec::new(),
                },
                video_info: None,
                audio_info: None,
                timescale: 1000,
                duration: 0,
                sample_table: SampleTable::default(),
                current_sample: 0,
            };

            while self.reader.stream_position().unwrap_or(end_pos) < end_pos {
                let (atom_size, atom_type) = self.read_atom_header()?;
                
                match atom_type {
                    TKHD => self.parse_tkhd(&mut track, atom_size - 8)?,
                    MDIA => self.parse_mdia(&mut track, atom_size - 8)?,
                    _ => self.skip_bytes(atom_size - 8)?,
                }
            }

            self.tracks.push(track);
            Ok(())
        }

        fn parse_tkhd(&mut self, track: &mut Track, size: u64) -> Result<(), String> {
            let version = self.read_u8()?;
            let flags = {
                let mut buf = [0u8; 3];
                self.reader.read_exact(&mut buf).map_err(|e| format!("Read error: {}", e))?;
                u32::from_be_bytes([0, buf[0], buf[1], buf[2]])
            };

            track.stream_info.default = (flags & 0x01) != 0;

            if version == 1 {
                self.skip_bytes(8)?;  // creation_time
                self.skip_bytes(8)?;  // modification_time
                track.id = self.read_u32()?;
                self.skip_bytes(4)?;  // reserved
                track.duration = self.read_u64()?;
            } else {
                self.skip_bytes(4)?;
                self.skip_bytes(4)?;
                track.id = self.read_u32()?;
                self.skip_bytes(4)?;
                track.duration = self.read_u32()? as u64;
            }

            let remaining = if version == 1 { size - 32 } else { size - 20 };
            self.skip_bytes(remaining)?;

            Ok(())
        }

        fn parse_mdia(&mut self, track: &mut Track, size: u64) -> Result<(), String> {
            let end_pos = self.reader.stream_position()
                .map_err(|e| format!("Position error: {}", e))? + size;

            while self.reader.stream_position().unwrap_or(end_pos) < end_pos {
                let (atom_size, atom_type) = self.read_atom_header()?;
                
                match atom_type {
                    MDHD => self.parse_mdhd(track, atom_size - 8)?,
                    HDLR => self.parse_hdlr(track, atom_size - 8)?,
                    MINF => self.parse_minf(track, atom_size - 8)?,
                    _ => self.skip_bytes(atom_size - 8)?,
                }
            }

            Ok(())
        }

        fn parse_mdhd(&mut self, track: &mut Track, size: u64) -> Result<(), String> {
            let version = self.read_u8()?;
            self.skip_bytes(3)?;

            if version == 1 {
                self.skip_bytes(8)?;
                self.skip_bytes(8)?;
                track.timescale = self.read_u32()?;
                self.skip_bytes(8)?;
            } else {
                self.skip_bytes(4)?;
                self.skip_bytes(4)?;
                track.timescale = self.read_u32()?;
                self.skip_bytes(4)?;
            }

            // Language (packed ISO-639-2)
            let lang = self.read_u16()?;
            let c1 = ((lang >> 10) & 0x1F) as u8 + 0x60;
            let c2 = ((lang >> 5) & 0x1F) as u8 + 0x60;
            let c3 = (lang & 0x1F) as u8 + 0x60;
            track.stream_info.language = Some(format!("{}{}{}", c1 as char, c2 as char, c3 as char));

            let remaining = if version == 1 { size - 26 } else { size - 18 };
            self.skip_bytes(remaining)?;

            Ok(())
        }

        fn parse_hdlr(&mut self, track: &mut Track, size: u64) -> Result<(), String> {
            self.skip_bytes(4)?;  // version + flags
            self.skip_bytes(4)?;  // pre_defined

            let handler_type = self.read_u32()?;
            track.stream_info.codec_type = match handler_type {
                0x76696465 => CodecType::Video,  // 'vide'
                0x736F756E => CodecType::Audio,  // 'soun'
                0x74657874 => CodecType::Subtitle, // 'text'
                0x73756274 => CodecType::Subtitle, // 'subt'
                _ => CodecType::Unknown,
            };

            self.skip_bytes(size - 8)?;
            Ok(())
        }

        fn parse_minf(&mut self, track: &mut Track, size: u64) -> Result<(), String> {
            let end_pos = self.reader.stream_position()
                .map_err(|e| format!("Position error: {}", e))? + size;

            while self.reader.stream_position().unwrap_or(end_pos) < end_pos {
                let (atom_size, atom_type) = self.read_atom_header()?;
                
                if atom_type == STBL {
                    self.parse_stbl(track, atom_size - 8)?;
                } else {
                    self.skip_bytes(atom_size - 8)?;
                }
            }

            Ok(())
        }

        fn parse_stbl(&mut self, track: &mut Track, size: u64) -> Result<(), String> {
            let end_pos = self.reader.stream_position()
                .map_err(|e| format!("Position error: {}", e))? + size;

            while self.reader.stream_position().unwrap_or(end_pos) < end_pos {
                let (atom_size, atom_type) = self.read_atom_header()?;
                
                match atom_type {
                    STSD => self.parse_stsd(track, atom_size - 8)?,
                    STTS => self.parse_stts(track, atom_size - 8)?,
                    STSC => self.parse_stsc(track, atom_size - 8)?,
                    STSZ => self.parse_stsz(track, atom_size - 8)?,
                    STCO => self.parse_stco(track, atom_size - 8)?,
                    CO64 => self.parse_co64(track, atom_size - 8)?,
                    STSS => self.parse_stss(track, atom_size - 8)?,
                    CTTS => self.parse_ctts(track, atom_size - 8)?,
                    _ => self.skip_bytes(atom_size - 8)?,
                }
            }

            Ok(())
        }

        fn parse_stsd(&mut self, track: &mut Track, size: u64) -> Result<(), String> {
            self.skip_bytes(4)?;  // version + flags
            let entry_count = self.read_u32()?;

            if entry_count > 0 {
                let (entry_size, codec_fourcc) = self.read_atom_header()?;
                
                track.stream_info.codec = match codec_fourcc {
                    AVC1 => CodecId::Video(VideoCodec::H264),
                    HVC1 | HEV1 => CodecId::Video(VideoCodec::H265),
                    VP09 => CodecId::Video(VideoCodec::VP9),
                    AV01 => CodecId::Video(VideoCodec::AV1),
                    MP4A => CodecId::Audio(AudioCodec::AAC),
                    AC3_ => CodecId::Audio(AudioCodec::AC3),
                    EC3_ => CodecId::Audio(AudioCodec::EAC3),
                    FLAC => CodecId::Audio(AudioCodec::FLAC),
                    OPUS => CodecId::Audio(AudioCodec::Opus),
                    _ => CodecId::Unknown,
                };

                // Parse video/audio specific info
                match track.stream_info.codec_type {
                    CodecType::Video => {
                        self.skip_bytes(6)?;  // reserved
                        self.skip_bytes(2)?;  // data_reference_index
                        self.skip_bytes(16)?; // pre_defined, reserved
                        let width = self.read_u16()?;
                        let height = self.read_u16()?;
                        self.skip_bytes(50)?; // rest of visual sample entry

                        track.video_info = Some(VideoInfo {
                            width: width as u32,
                            height: height as u32,
                            fps_num: 24000,
                            fps_den: 1001,
                            pixel_format: PixelFormat::YUV420P,
                            bit_depth: 8,
                            color_space: ColorSpace::BT709,
                        });

                        // Parse avcC/hvcC for extra_data
                        let remaining = entry_size as i64 - 8 - 78;
                        if remaining > 8 {
                            // Look for codec config
                            let start = self.reader.stream_position().unwrap_or(0);
                            let end = start + remaining as u64;
                            
                            while self.reader.stream_position().unwrap_or(end) < end {
                                if let Ok((cfg_size, cfg_type)) = self.read_atom_header() {
                                    if cfg_type == 0x61766343 || cfg_type == 0x68766343 {
                                        // avcC or hvcC
                                        let data_size = (cfg_size - 8) as usize;
                                        let mut data = vec![0u8; data_size];
                                        self.reader.read_exact(&mut data).ok();
                                        track.stream_info.extra_data = data;
                                    } else {
                                        self.skip_bytes(cfg_size - 8).ok();
                                    }
                                } else {
                                    break;
                                }
                            }
                        }
                    }
                    CodecType::Audio => {
                        self.skip_bytes(6)?;  // reserved
                        self.skip_bytes(2)?;  // data_reference_index
                        self.skip_bytes(8)?;  // reserved
                        let channels = self.read_u16()?;
                        let bits = self.read_u16()?;
                        self.skip_bytes(4)?;  // pre_defined, reserved
                        let sample_rate = self.read_u32()? >> 16;

                        track.audio_info = Some(AudioInfo {
                            sample_rate,
                            channels: channels as u8,
                            channel_layout: match channels {
                                1 => ChannelLayout::Mono,
                                2 => ChannelLayout::Stereo,
                                6 => ChannelLayout::Surround51,
                                8 => ChannelLayout::Surround71,
                                n => ChannelLayout::Unknown(n as u8),
                            },
                            bits_per_sample: bits as u8,
                        });
                    }
                    _ => {}
                }

                // Skip any remaining bytes in stsd
                let read_so_far = 8 + 78;  // Approximate
                if entry_size > read_so_far {
                    self.skip_bytes(entry_size - read_so_far).ok();
                }
            }

            Ok(())
        }

        fn parse_stts(&mut self, track: &mut Track, _size: u64) -> Result<(), String> {
            self.skip_bytes(4)?;
            let entry_count = self.read_u32()?;

            for _ in 0..entry_count {
                let sample_count = self.read_u32()?;
                let sample_delta = self.read_u32()?;
                track.sample_table.time_to_sample.push((sample_count, sample_delta));
            }

            Ok(())
        }

        fn parse_stsc(&mut self, track: &mut Track, _size: u64) -> Result<(), String> {
            self.skip_bytes(4)?;
            let entry_count = self.read_u32()?;

            for _ in 0..entry_count {
                let first_chunk = self.read_u32()?;
                let samples_per_chunk = self.read_u32()?;
                let sample_desc_index = self.read_u32()?;
                track.sample_table.sample_to_chunk.push((first_chunk, samples_per_chunk, sample_desc_index));
            }

            Ok(())
        }

        fn parse_stsz(&mut self, track: &mut Track, _size: u64) -> Result<(), String> {
            self.skip_bytes(4)?;
            let sample_size = self.read_u32()?;
            let sample_count = self.read_u32()?;

            if sample_size == 0 {
                for _ in 0..sample_count {
                    track.sample_table.sample_sizes.push(self.read_u32()?);
                }
            } else {
                track.sample_table.sample_sizes = vec![sample_size; sample_count as usize];
            }

            Ok(())
        }

        fn parse_stco(&mut self, track: &mut Track, _size: u64) -> Result<(), String> {
            self.skip_bytes(4)?;
            let entry_count = self.read_u32()?;

            for _ in 0..entry_count {
                track.sample_table.chunk_offsets.push(self.read_u32()? as u64);
            }

            Ok(())
        }

        fn parse_co64(&mut self, track: &mut Track, _size: u64) -> Result<(), String> {
            self.skip_bytes(4)?;
            let entry_count = self.read_u32()?;

            for _ in 0..entry_count {
                track.sample_table.chunk_offsets.push(self.read_u64()?);
            }

            Ok(())
        }

        fn parse_stss(&mut self, track: &mut Track, _size: u64) -> Result<(), String> {
            self.skip_bytes(4)?;
            let entry_count = self.read_u32()?;

            for _ in 0..entry_count {
                track.sample_table.keyframes.push(self.read_u32()?);
            }

            Ok(())
        }

        fn parse_ctts(&mut self, track: &mut Track, _size: u64) -> Result<(), String> {
            let version = self.read_u8()?;
            self.skip_bytes(3)?;
            let entry_count = self.read_u32()?;

            for _ in 0..entry_count {
                let sample_count = self.read_u32()?;
                let offset = if version == 1 {
                    self.read_u32()? as i32
                } else {
                    self.read_u32()? as i32
                };
                track.sample_table.composition_offsets.push((sample_count, offset));
            }

            Ok(())
        }

        /// Get stream info for all tracks
        pub fn streams(&self) -> Vec<StreamInfo> {
            self.tracks.iter().map(|t| t.stream_info.clone()).collect()
        }

        /// Get video info for video tracks
        pub fn video_info(&self, track_index: usize) -> Option<VideoInfo> {
            self.tracks.get(track_index).and_then(|t| t.video_info.clone())
        }

        /// Get audio info for audio tracks
        pub fn audio_info(&self, track_index: usize) -> Option<AudioInfo> {
            self.tracks.get(track_index).and_then(|t| t.audio_info.clone())
        }

        /// Read next packet
        pub fn read_packet(&mut self) -> Option<Packet> {
            // Find track with earliest next sample
            let mut best_track = None;
            let mut best_time = i64::MAX;

            for (idx, track) in self.tracks.iter().enumerate() {
                if track.current_sample < track.sample_table.sample_sizes.len() {
                    let time = self.calculate_sample_time(track, track.current_sample);
                    if time < best_time {
                        best_time = time;
                        best_track = Some(idx);
                    }
                }
            }

            let track_idx = best_track?;
            let track = &self.tracks[track_idx];
            let sample_idx = track.current_sample;

            // Get sample offset
            let offset = self.get_sample_offset(track, sample_idx)?;
            let size = track.sample_table.sample_sizes.get(sample_idx)?.clone();
            
            // Check if keyframe
            let keyframe = if track.sample_table.keyframes.is_empty() {
                true  // No keyframe table = all keyframes (e.g., audio)
            } else {
                track.sample_table.keyframes.contains(&(sample_idx as u32 + 1))
            };

            // Read data
            self.reader.seek(SeekFrom::Start(offset)).ok()?;
            let mut data = vec![0u8; size as usize];
            self.reader.read_exact(&mut data).ok()?;

            // Calculate PTS/DTS
            let pts = best_time;
            let dts = best_time;  // Simplified - should use ctts

            // Advance to next sample
            self.tracks[track_idx].current_sample += 1;

            Some(Packet {
                stream_index: track_idx as u32,
                pts,
                dts,
                duration: 0,
                keyframe,
                data,
            })
        }

        fn calculate_sample_time(&self, track: &Track, sample_idx: usize) -> i64 {
            let mut time = 0i64;
            let mut sample = 0usize;

            for &(count, delta) in &track.sample_table.time_to_sample {
                let count = count as usize;
                if sample + count > sample_idx {
                    time += ((sample_idx - sample) as i64) * (delta as i64);
                    break;
                }
                time += (count as i64) * (delta as i64);
                sample += count;
            }

            // Convert to microseconds
            time * 1_000_000 / (track.timescale as i64)
        }

        fn get_sample_offset(&self, track: &Track, sample_idx: usize) -> Option<u64> {
            let stsc = &track.sample_table.sample_to_chunk;
            let stco = &track.sample_table.chunk_offsets;
            let stsz = &track.sample_table.sample_sizes;

            if stsc.is_empty() || stco.is_empty() {
                return None;
            }

            // Find which chunk this sample is in
            let mut sample = 0usize;

            for i in 0..stsc.len() {
                let first_chunk = (stsc[i].0 - 1) as usize;
                let spc = stsc[i].1 as usize;
                
                let next_first_chunk = if i + 1 < stsc.len() {
                    (stsc[i + 1].0 - 1) as usize
                } else {
                    stco.len()
                };

                for c in first_chunk..next_first_chunk {
                    if sample + spc > sample_idx {
                        // Calculate offset within chunk
                        let chunk_offset = *stco.get(c)?;
                        let sample_in_chunk = sample_idx - sample;
                        
                        let mut offset = chunk_offset;
                        for j in 0..sample_in_chunk {
                            offset += *stsz.get(sample + j)? as u64;
                        }
                        return Some(offset);
                    }
                    sample += spc;
                }
            }

            None
        }

        /// Seek to specific timestamp (microseconds)
        pub fn seek(&mut self, timestamp_us: i64) -> Result<(), String> {
            for track in &mut self.tracks {
                // Find closest keyframe before timestamp
                let target_sample = Self::find_sample_for_time_static(track, timestamp_us);
                
                if !track.sample_table.keyframes.is_empty() {
                    // Find nearest keyframe at or before target
                    let mut best_keyframe = 0usize;
                    for &kf in &track.sample_table.keyframes {
                        if (kf as usize) <= target_sample + 1 {
                            best_keyframe = (kf - 1) as usize;
                        } else {
                            break;
                        }
                    }
                    track.current_sample = best_keyframe;
                } else {
                    track.current_sample = target_sample;
                }
            }

            Ok(())
        }

        fn find_sample_for_time_static(track: &Track, timestamp_us: i64) -> usize {
            let target_time = timestamp_us * (track.timescale as i64) / 1_000_000;
            
            let mut time = 0i64;
            let mut sample = 0usize;

            for &(count, delta) in &track.sample_table.time_to_sample {
                let segment_duration = (count as i64) * (delta as i64);
                if time + segment_duration > target_time {
                    let remaining = target_time - time;
                    sample += (remaining / (delta as i64)) as usize;
                    return sample;
                }
                time += segment_duration;
                sample += count as usize;
            }

            sample
        }

        /// Get duration in microseconds
        pub fn duration_us(&self) -> i64 {
            (self.duration as i64) * 1_000_000 / (self.timescale as i64)
        }
    }
}

// ============================================================================
// Public Rust API
// ============================================================================




pub fn demux_probe_file(path: String) -> Result<serde_json::Value, String> {
    use std::fs::File;
    
    let file = File::open(&path).map_err(|e| format!("Open error: {}", e))?;
    
    // Try MP4 first
    if let Ok(demuxer) = mp4::Mp4Demuxer::new(file) {
        let streams = demuxer.streams();
        return Ok(serde_json::json!({
            "format": "mp4",
            "duration_us": demuxer.duration_us(),
            "streams": streams,
        }));
    }
    
    // TODO: Try MKV, AVI, TS
    
    Err("Unknown format".to_string())
}


pub fn demux_get_streams(path: String) -> Result<Vec<serde_json::Value>, String> {
    use std::fs::File;
    
    let file = File::open(&path).map_err(|e| format!("Open error: {}", e))?;
    let demuxer = mp4::Mp4Demuxer::new(file)?;
    
    Ok(demuxer.streams().into_iter().map(|s| serde_json::to_value(s).unwrap()).collect())
}


pub fn ffmpeg_minimal_description() -> String {
    r#"
FFMPEG MINIMAL - Only What SLAIN Needs

FFmpeg is 1.5 million lines of C.
SLAIN needs maybe 5% of it.

WHAT'S COVERED:
✓ MP4/MOV demuxing (this file)
✓ MKV demuxing (mkv.rs)
✓ Audio decoding (audio.rs via Symphonia)
✓ Subtitle parsing (subtitles.rs)

WHAT'S NEEDED:
• AVI demuxing
• TS demuxing (for IPTV)
• Hardware video decoding hooks (NVDEC/VAAPI)
• Pixel format conversion (YUV→RGB)
• Audio resampling

NO SOFTWARE VIDEO DECODERS:
We use hardware decoding for H.264/H.265/VP9/AV1.
The GPU does the work, not the CPU.
"#.to_string()
}
