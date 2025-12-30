//! MP4 track metadata.

use super::{AudioInfo, SampleTable, StreamInfo, VideoInfo};

#[derive(Debug, Clone)]
pub struct Track {
    pub id: u32,
    pub stream_info: StreamInfo,
    pub video_info: Option<VideoInfo>,
    pub audio_info: Option<AudioInfo>,
    pub timescale: u32,
    pub duration: u64,
    pub sample_table: SampleTable,
    pub current_sample: usize,
}
