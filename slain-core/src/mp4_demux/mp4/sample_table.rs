//! MP4 sample table data structures.

#[derive(Debug, Clone, Default)]
pub struct SampleTable {
    pub sample_sizes: Vec<u32>,
    pub chunk_offsets: Vec<u64>,
    pub sample_to_chunk: Vec<(u32, u32, u32)>, // first_chunk, samples_per_chunk, sample_desc_index
    pub time_to_sample: Vec<(u32, u32)>,       // sample_count, sample_delta
    pub keyframes: Vec<u32>,                   // Sample numbers that are keyframes
    pub composition_offsets: Vec<(u32, i32)>,  // sample_count, offset
}
