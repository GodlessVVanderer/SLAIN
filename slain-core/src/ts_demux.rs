// TS DEMUXER - MPEG Transport Stream Parser
//
// TS is the container for:
// • IPTV streams
// • DVB broadcasts
// • Blu-ray discs (M2TS variant)
// • ATSC broadcasts
//
// Fixed 188-byte packets. Designed for error resilience in broadcast.

use std::io::{Read, Seek, SeekFrom};
use std::collections::HashMap;
use serde::{Deserialize, Serialize};

// ============================================================================
// Constants
// ============================================================================

const TS_PACKET_SIZE: usize = 188;
const TS_SYNC_BYTE: u8 = 0x47;
const M2TS_PACKET_SIZE: usize = 192;  // 4-byte timestamp + 188-byte TS

// PIDs
const PAT_PID: u16 = 0x0000;
const CAT_PID: u16 = 0x0001;
const NULL_PID: u16 = 0x1FFF;

// Stream types
const STREAM_TYPE_MPEG1_VIDEO: u8 = 0x01;
const STREAM_TYPE_MPEG2_VIDEO: u8 = 0x02;
const STREAM_TYPE_MPEG1_AUDIO: u8 = 0x03;
const STREAM_TYPE_MPEG2_AUDIO: u8 = 0x04;
const STREAM_TYPE_AAC: u8 = 0x0F;
const STREAM_TYPE_AAC_LATM: u8 = 0x11;
const STREAM_TYPE_H264: u8 = 0x1B;
const STREAM_TYPE_H265: u8 = 0x24;
const STREAM_TYPE_AC3: u8 = 0x81;
const STREAM_TYPE_DTS: u8 = 0x82;
const STREAM_TYPE_TRUEHD: u8 = 0x83;
const STREAM_TYPE_EAC3: u8 = 0x87;
const STREAM_TYPE_SUBTITLE: u8 = 0x06;

// ============================================================================
// Types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TsInfo {
    pub is_m2ts: bool,
    pub programs: Vec<Program>,
    pub streams: Vec<TsStream>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Program {
    pub number: u16,
    pub pmt_pid: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TsStream {
    pub pid: u16,
    pub stream_type: u8,
    pub codec: StreamCodec,
    pub language: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StreamCodec {
    H264,
    H265,
    MPEG2Video,
    MPEG1Video,
    AAC,
    AC3,
    EAC3,
    DTS,
    TrueHD,
    MP3,
    MPEG2Audio,
    Subtitle,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct TsPacket {
    pub pid: u16,
    pub pts: Option<i64>,
    pub dts: Option<i64>,
    pub keyframe: bool,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct PesPacket {
    pub stream_id: u8,
    pub pts: Option<i64>,
    pub dts: Option<i64>,
    pub data: Vec<u8>,
}

// ============================================================================
// TS Packet Header
// ============================================================================

#[derive(Debug, Clone)]
struct TsHeader {
    sync_byte: u8,
    transport_error: bool,
    payload_unit_start: bool,
    transport_priority: bool,
    pid: u16,
    scrambling_control: u8,
    adaptation_field_exists: bool,
    payload_exists: bool,
    continuity_counter: u8,
}

impl TsHeader {
    fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 4 || data[0] != TS_SYNC_BYTE {
            return None;
        }
        
        Some(Self {
            sync_byte: data[0],
            transport_error: (data[1] & 0x80) != 0,
            payload_unit_start: (data[1] & 0x40) != 0,
            transport_priority: (data[1] & 0x20) != 0,
            pid: ((data[1] as u16 & 0x1F) << 8) | data[2] as u16,
            scrambling_control: (data[3] >> 6) & 0x03,
            adaptation_field_exists: (data[3] & 0x20) != 0,
            payload_exists: (data[3] & 0x10) != 0,
            continuity_counter: data[3] & 0x0F,
        })
    }
}

// ============================================================================
// Adaptation Field
// ============================================================================

#[derive(Debug, Clone, Default)]
struct AdaptationField {
    length: u8,
    discontinuity: bool,
    random_access: bool,  // Keyframe indicator
    pcr: Option<i64>,
}

impl AdaptationField {
    fn parse(data: &[u8]) -> Option<Self> {
        if data.is_empty() {
            return None;
        }
        
        let length = data[0];
        if length == 0 || data.len() < length as usize + 1 {
            return Some(Self { length, ..Default::default() });
        }
        
        let flags = data[1];
        let discontinuity = (flags & 0x80) != 0;
        let random_access = (flags & 0x40) != 0;
        let pcr_flag = (flags & 0x10) != 0;
        
        let pcr = if pcr_flag && data.len() >= 8 {
            // PCR is 33 bits base + 9 bits extension
            let base = ((data[2] as i64) << 25)
                     | ((data[3] as i64) << 17)
                     | ((data[4] as i64) << 9)
                     | ((data[5] as i64) << 1)
                     | ((data[6] as i64) >> 7);
            let ext = (((data[6] as i64) & 0x01) << 8) | (data[7] as i64);
            Some(base * 300 + ext)  // Convert to 27MHz ticks
        } else {
            None
        };
        
        Some(Self {
            length,
            discontinuity,
            random_access,
            pcr,
        })
    }
}

// ============================================================================
// PES Parsing
// ============================================================================

fn parse_pes_header(data: &[u8]) -> Option<(PesPacket, usize)> {
    if data.len() < 9 {
        return None;
    }
    
    // Check PES start code: 0x000001
    if data[0] != 0x00 || data[1] != 0x00 || data[2] != 0x01 {
        return None;
    }
    
    let stream_id = data[3];
    let pes_length = ((data[4] as usize) << 8) | data[5] as usize;
    
    // Video/audio streams have extended header
    let (pts, dts, header_len) = if stream_id >= 0xE0 || (stream_id >= 0xC0 && stream_id < 0xE0) {
        if data.len() < 9 {
            return None;
        }
        
        let pts_dts_flags = (data[7] >> 6) & 0x03;
        let header_data_length = data[8] as usize;
        
        let mut pts = None;
        let mut dts = None;
        
        if pts_dts_flags >= 2 && data.len() >= 14 {
            pts = Some(parse_timestamp(&data[9..14]));
        }
        
        if pts_dts_flags == 3 && data.len() >= 19 {
            dts = Some(parse_timestamp(&data[14..19]));
        }
        
        (pts, dts, 9 + header_data_length)
    } else {
        (None, None, 6)
    };
    
    let payload_start = header_len;
    let payload = if pes_length > 0 && data.len() >= payload_start + pes_length - (header_len - 6) {
        data[payload_start..].to_vec()
    } else {
        data[payload_start..].to_vec()
    };
    
    Some((
        PesPacket {
            stream_id,
            pts,
            dts,
            data: payload,
        },
        payload_start,
    ))
}

fn parse_timestamp(data: &[u8]) -> i64 {
    // 33-bit timestamp encoded in 5 bytes
    let ts = (((data[0] as i64) >> 1) & 0x07) << 30
           | ((data[1] as i64) << 22)
           | (((data[2] as i64) >> 1) << 15)
           | ((data[3] as i64) << 7)
           | ((data[4] as i64) >> 1);
    
    // Convert from 90kHz to microseconds
    ts * 1_000_000 / 90_000
}

// ============================================================================
// TS Demuxer
// ============================================================================

pub struct TsDemuxer<R: Read + Seek> {
    reader: R,
    info: TsInfo,
    pid_to_stream: HashMap<u16, usize>,
    pes_buffers: HashMap<u16, Vec<u8>>,
    pes_pts: HashMap<u16, Option<i64>>,
    pes_keyframe: HashMap<u16, bool>,
    packet_size: usize,
}

impl<R: Read + Seek> TsDemuxer<R> {
    pub fn new(mut reader: R) -> Result<Self, String> {
        // Detect packet size (188 for TS, 192 for M2TS)
        let packet_size = detect_packet_size(&mut reader)?;
        reader.seek(SeekFrom::Start(0))
            .map_err(|e| format!("Seek error: {}", e))?;
        
        let mut demuxer = Self {
            reader,
            info: TsInfo {
                is_m2ts: packet_size == M2TS_PACKET_SIZE,
                programs: Vec::new(),
                streams: Vec::new(),
            },
            pid_to_stream: HashMap::new(),
            pes_buffers: HashMap::new(),
            pes_pts: HashMap::new(),
            pes_keyframe: HashMap::new(),
            packet_size,
        };
        
        demuxer.scan_streams()?;
        
        Ok(demuxer)
    }
    
    fn scan_streams(&mut self) -> Result<(), String> {
        // Read first ~1000 packets to find PAT/PMT
        let mut pmt_pids: Vec<u16> = Vec::new();
        let mut packets_read = 0;
        let max_packets = 1000;
        
        let mut packet_buf = vec![0u8; self.packet_size];
        
        while packets_read < max_packets {
            if self.reader.read_exact(&mut packet_buf).is_err() {
                break;
            }
            
            let ts_data = if self.info.is_m2ts {
                &packet_buf[4..]
            } else {
                &packet_buf[..]
            };
            
            let header = match TsHeader::parse(ts_data) {
                Some(h) => h,
                None => {
                    packets_read += 1;
                    continue;
                }
            };
            
            if header.pid == PAT_PID && header.payload_exists && header.payload_unit_start {
                // Parse PAT
                let payload_offset = if header.adaptation_field_exists {
                    5 + ts_data[4] as usize
                } else {
                    4
                };
                
                if payload_offset < ts_data.len() {
                    let payload = &ts_data[payload_offset..];
                    pmt_pids = self.parse_pat(payload);
                }
            } else if pmt_pids.contains(&header.pid) && header.payload_exists && header.payload_unit_start {
                // Parse PMT
                let payload_offset = if header.adaptation_field_exists {
                    5 + ts_data[4] as usize
                } else {
                    4
                };
                
                if payload_offset < ts_data.len() {
                    let payload = &ts_data[payload_offset..];
                    self.parse_pmt(payload);
                }
            }
            
            packets_read += 1;
            
            // Check if we have enough info
            if !self.info.streams.is_empty() && packets_read > 100 {
                break;
            }
        }
        
        // Seek back to start
        self.reader.seek(SeekFrom::Start(0))
            .map_err(|e| format!("Seek error: {}", e))?;
        
        Ok(())
    }
    
    fn parse_pat(&mut self, data: &[u8]) -> Vec<u16> {
        let mut pmt_pids = Vec::new();
        
        if data.len() < 8 {
            return pmt_pids;
        }
        
        // Skip pointer field
        let pointer = data[0] as usize;
        let section = &data[1 + pointer..];
        
        if section.len() < 8 {
            return pmt_pids;
        }
        
        let table_id = section[0];
        if table_id != 0x00 {
            return pmt_pids;
        }
        
        let section_length = (((section[1] as usize) & 0x0F) << 8) | section[2] as usize;
        let section_end = 3 + section_length.min(section.len() - 3);
        
        // Skip header fields
        let mut pos = 8;
        
        while pos + 4 <= section_end.saturating_sub(4) {
            let program_num = ((section[pos] as u16) << 8) | section[pos + 1] as u16;
            let pid = (((section[pos + 2] as u16) & 0x1F) << 8) | section[pos + 3] as u16;
            
            if program_num != 0 {
                self.info.programs.push(Program {
                    number: program_num,
                    pmt_pid: pid,
                });
                pmt_pids.push(pid);
            }
            
            pos += 4;
        }
        
        pmt_pids
    }
    
    fn parse_pmt(&mut self, data: &[u8]) {
        if data.len() < 12 {
            return;
        }
        
        // Skip pointer field
        let pointer = data[0] as usize;
        let section = &data[1 + pointer..];
        
        if section.len() < 12 {
            return;
        }
        
        let table_id = section[0];
        if table_id != 0x02 {
            return;
        }
        
        let section_length = (((section[1] as usize) & 0x0F) << 8) | section[2] as usize;
        let program_info_length = (((section[10] as usize) & 0x0F) << 8) | section[11] as usize;
        
        let mut pos = 12 + program_info_length;
        let section_end = 3 + section_length.min(section.len() - 3);
        
        while pos + 5 <= section_end.saturating_sub(4) {
            let stream_type = section[pos];
            let pid = (((section[pos + 1] as u16) & 0x1F) << 8) | section[pos + 2] as u16;
            let es_info_length = (((section[pos + 3] as usize) & 0x0F) << 8) | section[pos + 4] as usize;
            
            // Parse ES descriptors for language
            let mut language = None;
            if es_info_length > 0 && pos + 5 + es_info_length <= section.len() {
                let descriptors = &section[pos + 5..pos + 5 + es_info_length];
                language = parse_language_descriptor(descriptors);
            }
            
            let codec = match stream_type {
                STREAM_TYPE_H264 => StreamCodec::H264,
                STREAM_TYPE_H265 => StreamCodec::H265,
                STREAM_TYPE_MPEG2_VIDEO => StreamCodec::MPEG2Video,
                STREAM_TYPE_MPEG1_VIDEO => StreamCodec::MPEG1Video,
                STREAM_TYPE_AAC | STREAM_TYPE_AAC_LATM => StreamCodec::AAC,
                STREAM_TYPE_AC3 => StreamCodec::AC3,
                STREAM_TYPE_EAC3 => StreamCodec::EAC3,
                STREAM_TYPE_DTS => StreamCodec::DTS,
                STREAM_TYPE_TRUEHD => StreamCodec::TrueHD,
                STREAM_TYPE_MPEG1_AUDIO => StreamCodec::MP3,
                STREAM_TYPE_MPEG2_AUDIO => StreamCodec::MPEG2Audio,
                STREAM_TYPE_SUBTITLE => StreamCodec::Subtitle,
                _ => StreamCodec::Unknown,
            };
            
            let stream_idx = self.info.streams.len();
            self.info.streams.push(TsStream {
                pid,
                stream_type,
                codec,
                language,
            });
            self.pid_to_stream.insert(pid, stream_idx);
            
            pos += 5 + es_info_length;
        }
    }
    
    /// Get stream info
    pub fn info(&self) -> &TsInfo {
        &self.info
    }
    
    /// Read next packet
    pub fn read_packet(&mut self) -> Option<TsPacket> {
        let mut packet_buf = vec![0u8; self.packet_size];
        
        loop {
            if self.reader.read_exact(&mut packet_buf).is_err() {
                // Flush remaining PES buffers
                return self.flush_pes_buffer();
            }
            
            let ts_data = if self.info.is_m2ts {
                &packet_buf[4..]
            } else {
                &packet_buf[..]
            };
            
            let header = match TsHeader::parse(ts_data) {
                Some(h) => h,
                None => continue,
            };
            
            // Skip null packets and PAT/PMT
            if header.pid == NULL_PID || header.pid == PAT_PID || !self.pid_to_stream.contains_key(&header.pid) {
                continue;
            }
            
            if !header.payload_exists {
                continue;
            }
            
            // Get payload
            let mut payload_offset = 4;
            let mut keyframe = false;
            
            if header.adaptation_field_exists {
                if let Some(af) = AdaptationField::parse(&ts_data[4..]) {
                    payload_offset = 5 + af.length as usize;
                    keyframe = af.random_access;
                }
            }
            
            if payload_offset >= TS_PACKET_SIZE {
                continue;
            }
            
            let payload = &ts_data[payload_offset..];
            
            // Handle PES assembly
            if header.payload_unit_start {
                // Emit previous PES packet if exists
                if let Some(packet) = self.emit_pes(header.pid) {
                    // Start new PES buffer
                    self.pes_buffers.insert(header.pid, payload.to_vec());
                    self.pes_keyframe.insert(header.pid, keyframe);
                    
                    // Parse PES header for PTS
                    if let Some((pes, _)) = parse_pes_header(payload) {
                        self.pes_pts.insert(header.pid, pes.pts);
                    }
                    
                    return Some(packet);
                } else {
                    // Start new PES buffer
                    self.pes_buffers.insert(header.pid, payload.to_vec());
                    self.pes_keyframe.insert(header.pid, keyframe);
                    
                    if let Some((pes, _)) = parse_pes_header(payload) {
                        self.pes_pts.insert(header.pid, pes.pts);
                    }
                }
            } else {
                // Append to existing PES buffer
                if let Some(buffer) = self.pes_buffers.get_mut(&header.pid) {
                    buffer.extend_from_slice(payload);
                    if keyframe {
                        self.pes_keyframe.insert(header.pid, true);
                    }
                }
            }
        }
    }
    
    fn emit_pes(&mut self, pid: u16) -> Option<TsPacket> {
        let buffer = self.pes_buffers.remove(&pid)?;
        let pts = self.pes_pts.remove(&pid).flatten();
        let keyframe = self.pes_keyframe.remove(&pid).unwrap_or(false);
        
        // Parse PES to get actual payload
        let data = if let Some((pes, header_len)) = parse_pes_header(&buffer) {
            buffer[header_len..].to_vec()
        } else {
            buffer
        };
        
        Some(TsPacket {
            pid,
            pts,
            dts: pts,
            keyframe,
            data,
        })
    }
    
    fn flush_pes_buffer(&mut self) -> Option<TsPacket> {
        let pid = *self.pes_buffers.keys().next()?;
        self.emit_pes(pid)
    }
    
    /// Seek to timestamp (microseconds)
    pub fn seek(&mut self, _timestamp_us: i64) -> Result<(), String> {
        // TS doesn't have a seek index - need to scan for keyframes
        // For now, just seek to start
        self.reader.seek(SeekFrom::Start(0))
            .map_err(|e| format!("Seek error: {}", e))?;
        self.pes_buffers.clear();
        self.pes_pts.clear();
        self.pes_keyframe.clear();
        Ok(())
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

fn detect_packet_size<R: Read + Seek>(reader: &mut R) -> Result<usize, String> {
    let mut buf = [0u8; 1024];
    reader.read_exact(&mut buf).map_err(|e| format!("Read error: {}", e))?;
    
    // Look for sync bytes at 188-byte intervals
    let mut ts_score = 0;
    for i in 0..5 {
        let offset = i * 188;
        if offset < buf.len() && buf[offset] == TS_SYNC_BYTE {
            ts_score += 1;
        }
    }
    
    // Look for sync bytes at 192-byte intervals (M2TS)
    let mut m2ts_score = 0;
    for i in 0..5 {
        let offset = i * 192 + 4;  // 4-byte timestamp prefix
        if offset < buf.len() && buf[offset] == TS_SYNC_BYTE {
            m2ts_score += 1;
        }
    }
    
    if m2ts_score > ts_score {
        Ok(M2TS_PACKET_SIZE)
    } else if ts_score >= 3 {
        Ok(TS_PACKET_SIZE)
    } else {
        Err("Not a valid TS/M2TS file".to_string())
    }
}

fn parse_language_descriptor(data: &[u8]) -> Option<String> {
    let mut pos = 0;
    
    while pos + 2 <= data.len() {
        let tag = data[pos];
        let length = data[pos + 1] as usize;
        
        if tag == 0x0A && length >= 3 && pos + 2 + length <= data.len() {
            // ISO 639 language descriptor
            let lang_bytes = &data[pos + 2..pos + 5];
            if lang_bytes.iter().all(|b| b.is_ascii_alphabetic()) {
                return Some(String::from_utf8_lossy(lang_bytes).to_string());
            }
        }
        
        pos += 2 + length;
    }
    
    None
}

// ============================================================================
// Public Rust API
// ============================================================================




pub fn ts_probe(path: String) -> Result<serde_json::Value, String> {
    use std::fs::File;
    
    let file = File::open(&path).map_err(|e| format!("Open error: {}", e))?;
    let demuxer = TsDemuxer::new(file)?;
    
    serde_json::to_value(demuxer.info()).map_err(|e| format!("JSON error: {}", e))
}


pub fn ts_description() -> String {
    r#"
TS DEMUXER - MPEG Transport Stream Parser

The container for broadcast and streaming:
• IPTV streams
• DVB/ATSC broadcasts
• Blu-ray (M2TS variant)
• Live streaming

STRUCTURE:
• Fixed 188-byte packets (192 for M2TS)
• PAT (Program Association Table) at PID 0
• PMT (Program Map Table) lists streams
• PES (Packetized Elementary Stream) carries A/V data

SUPPORTED CODECS:
Video: H.264, H.265, MPEG-2
Audio: AAC, AC3, E-AC3, DTS, TrueHD, MP3
Subtitles: DVB subtitles

FEATURES:
• Auto-detect TS vs M2TS
• PAT/PMT parsing
• PES reassembly
• Language detection
"#.to_string()
}
