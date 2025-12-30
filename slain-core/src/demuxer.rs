//! Universal demuxer facade for supported containers.

use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use crate::avi_demux::{AviDemuxer, AviPacket};
use crate::mkv::{MkvDemuxer, MkvInfo, MkvPacket, MkvParser};
use crate::mp4_demux::{Mp4Demuxer, Packet as Mp4Packet};
use crate::ts_demux::{TsDemuxer, TsPacket};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerKind {
    Mkv,
    Mp4,
    Avi,
    Ts,
}

#[derive(Debug, Clone)]
pub struct UniversalPacket {
    pub stream_index: u32,
    pub pts_us: Option<i64>,
    pub dts_us: Option<i64>,
    pub keyframe: bool,
    pub data: Vec<u8>,
}

pub enum UniversalDemuxer {
    Mkv(MkvDemuxer<BufReader<File>>, MkvInfo),
    Mp4(Mp4Demuxer<BufReader<File>>),
    Avi(AviDemuxer<BufReader<File>>),
    Ts(TsDemuxer<BufReader<File>>),
}

impl UniversalDemuxer {
    pub fn open(path: &Path) -> Result<Self, String> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        match ext.as_str() {
            "mkv" | "webm" => {
                let mut parser = MkvParser::new();
                let info = parser.parse(path)?;
                let file = File::open(path).map_err(|e| format!("Open error: {}", e))?;
                let reader = BufReader::new(file);
                let demuxer = MkvDemuxer::new(reader, info.clone())?;
                Ok(Self::Mkv(demuxer, info))
            }
            "mp4" | "m4v" | "mov" => {
                let file = File::open(path).map_err(|e| format!("Open error: {}", e))?;
                let reader = BufReader::new(file);
                let demuxer = Mp4Demuxer::new(reader).map_err(|e| format!("Demux init: {}", e))?;
                Ok(Self::Mp4(demuxer))
            }
            "avi" => {
                let file = File::open(path).map_err(|e| format!("Open error: {}", e))?;
                let reader = BufReader::new(file);
                let demuxer = AviDemuxer::new(reader)?;
                Ok(Self::Avi(demuxer))
            }
            "ts" | "mts" | "m2ts" => {
                let file = File::open(path).map_err(|e| format!("Open error: {}", e))?;
                let reader = BufReader::new(file);
                let demuxer = TsDemuxer::new(reader)?;
                Ok(Self::Ts(demuxer))
            }
            other => Err(format!("Unsupported container: {}", other)),
        }
    }

    pub fn container(&self) -> ContainerKind {
        match self {
            UniversalDemuxer::Mkv(_, _) => ContainerKind::Mkv,
            UniversalDemuxer::Mp4(_) => ContainerKind::Mp4,
            UniversalDemuxer::Avi(_) => ContainerKind::Avi,
            UniversalDemuxer::Ts(_) => ContainerKind::Ts,
        }
    }

    pub fn mkv_info(&self) -> Option<&MkvInfo> {
        match self {
            UniversalDemuxer::Mkv(_, info) => Some(info),
            _ => None,
        }
    }

    pub fn read_packet(&mut self) -> Option<UniversalPacket> {
        match self {
            UniversalDemuxer::Mkv(demuxer, _) => demuxer.read_packet().map(map_mkv_packet),
            UniversalDemuxer::Mp4(demuxer) => demuxer.read_packet().map(map_mp4_packet),
            UniversalDemuxer::Avi(demuxer) => demuxer.read_packet().map(map_avi_packet),
            UniversalDemuxer::Ts(demuxer) => demuxer.read_packet().map(map_ts_packet),
        }
    }
}

fn map_mkv_packet(packet: MkvPacket) -> UniversalPacket {
    let stream_index = u32::try_from(packet.track_number).unwrap_or(0);
    let pts_us = packet.pts_ms.saturating_mul(1_000);
    UniversalPacket {
        stream_index,
        pts_us: Some(pts_us),
        dts_us: Some(pts_us),
        keyframe: packet.keyframe,
        data: packet.data,
    }
}

fn map_mp4_packet(packet: Mp4Packet) -> UniversalPacket {
    UniversalPacket {
        stream_index: packet.stream_index,
        pts_us: Some(packet.pts),
        dts_us: Some(packet.dts),
        keyframe: packet.keyframe,
        data: packet.data,
    }
}

fn map_avi_packet(packet: AviPacket) -> UniversalPacket {
    UniversalPacket {
        stream_index: packet.stream_index,
        pts_us: Some(packet.pts),
        dts_us: Some(packet.dts),
        keyframe: packet.keyframe,
        data: packet.data,
    }
}

fn map_ts_packet(packet: TsPacket) -> UniversalPacket {
    UniversalPacket {
        stream_index: packet.pid as u32,
        pts_us: packet.pts,
        dts_us: packet.dts,
        keyframe: packet.keyframe,
        data: packet.data,
    }
}
