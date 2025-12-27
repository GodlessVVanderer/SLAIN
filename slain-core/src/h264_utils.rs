//! H.264 NAL unit utilities
//!
//! Handles conversion between AVCC (length-prefixed) and Annex B (start code) formats.
//! MKV/MP4 use AVCC format, hardware decoders (NVDEC) expect Annex B.

/// Annex B start code (4-byte version)
const ANNEX_B_START_CODE: [u8; 4] = [0x00, 0x00, 0x00, 0x01];

/// Convert AVCC format NAL units to Annex B format
///
/// AVCC: [4-byte length][NAL][4-byte length][NAL]...
/// Annex B: [0x00 0x00 0x00 0x01][NAL][0x00 0x00 0x00 0x01][NAL]...
pub fn avcc_to_annexb(data: &[u8], nal_length_size: usize) -> Vec<u8> {
    if data.is_empty() || nal_length_size == 0 || nal_length_size > 4 {
        return data.to_vec();
    }

    let mut result = Vec::with_capacity(data.len() + 64);
    let mut offset = 0;

    while offset + nal_length_size <= data.len() {
        // Read NAL length (big-endian)
        let nal_len = read_be_uint(&data[offset..], nal_length_size);
        offset += nal_length_size;

        if nal_len == 0 || offset + nal_len > data.len() {
            break;
        }

        // Write start code + NAL data
        result.extend_from_slice(&ANNEX_B_START_CODE);
        result.extend_from_slice(&data[offset..offset + nal_len]);
        offset += nal_len;
    }

    result
}

/// Parse AVCC (avcC) extradata and extract SPS/PPS as Annex B
///
/// Returns the SPS/PPS NALs with start codes, ready to feed to decoder
pub fn parse_avcc_extradata(extradata: &[u8]) -> Option<(Vec<u8>, usize)> {
    if extradata.len() < 7 {
        return None;
    }

    // AVCC format:
    // [0]: version (always 1)
    // [1]: profile
    // [2]: profile compat
    // [3]: level
    // [4]: 0xFC | (nal_length_size - 1)  -> nal_length_size = (byte & 3) + 1
    // [5]: 0xE0 | num_sps
    // Then SPS entries, then PPS entries

    if extradata[0] != 1 {
        return None;
    }

    let nal_length_size = ((extradata[4] & 0x03) + 1) as usize;
    let num_sps = (extradata[5] & 0x1F) as usize;

    let mut result = Vec::with_capacity(extradata.len() + 32);
    let mut offset = 6;

    // Extract SPS
    for _ in 0..num_sps {
        if offset + 2 > extradata.len() {
            return None;
        }
        let sps_len = u16::from_be_bytes([extradata[offset], extradata[offset + 1]]) as usize;
        offset += 2;

        if offset + sps_len > extradata.len() {
            return None;
        }

        result.extend_from_slice(&ANNEX_B_START_CODE);
        result.extend_from_slice(&extradata[offset..offset + sps_len]);
        offset += sps_len;
    }

    // Extract PPS
    if offset >= extradata.len() {
        return Some((result, nal_length_size));
    }

    let num_pps = extradata[offset] as usize;
    offset += 1;

    for _ in 0..num_pps {
        if offset + 2 > extradata.len() {
            break;
        }
        let pps_len = u16::from_be_bytes([extradata[offset], extradata[offset + 1]]) as usize;
        offset += 2;

        if offset + pps_len > extradata.len() {
            break;
        }

        result.extend_from_slice(&ANNEX_B_START_CODE);
        result.extend_from_slice(&extradata[offset..offset + pps_len]);
        offset += pps_len;
    }

    Some((result, nal_length_size))
}

/// Parse HEVC (hvcC) extradata and extract VPS/SPS/PPS as Annex B
pub fn parse_hvcc_extradata(extradata: &[u8]) -> Option<(Vec<u8>, usize)> {
    if extradata.len() < 23 {
        return None;
    }

    // HVCC format is more complex
    // [21]: (lengthSizeMinusOne & 3) -> nal_length_size = (byte & 3) + 1
    // [22]: numOfArrays
    // Then arrays of NAL units

    let nal_length_size = ((extradata[21] & 0x03) + 1) as usize;
    let num_arrays = extradata[22] as usize;

    let mut result = Vec::with_capacity(extradata.len() + 64);
    let mut offset = 23;

    for _ in 0..num_arrays {
        if offset + 3 > extradata.len() {
            break;
        }

        let _nal_type = extradata[offset] & 0x3F;
        offset += 1;

        let num_nalus = u16::from_be_bytes([extradata[offset], extradata[offset + 1]]) as usize;
        offset += 2;

        for _ in 0..num_nalus {
            if offset + 2 > extradata.len() {
                break;
            }
            let nal_len = u16::from_be_bytes([extradata[offset], extradata[offset + 1]]) as usize;
            offset += 2;

            if offset + nal_len > extradata.len() {
                break;
            }

            result.extend_from_slice(&ANNEX_B_START_CODE);
            result.extend_from_slice(&extradata[offset..offset + nal_len]);
            offset += nal_len;
        }
    }

    Some((result, nal_length_size))
}

/// Check if data already has Annex B start codes
pub fn is_annexb(data: &[u8]) -> bool {
    if data.len() < 4 {
        return false;
    }
    // Check for 4-byte or 3-byte start code
    (data[0] == 0 && data[1] == 0 && data[2] == 0 && data[3] == 1)
        || (data[0] == 0 && data[1] == 0 && data[2] == 1)
}

/// Read big-endian unsigned integer of variable size (1-4 bytes)
fn read_be_uint(data: &[u8], size: usize) -> usize {
    let mut val = 0usize;
    for i in 0..size {
        val = (val << 8) | (data[i] as usize);
    }
    val
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_avcc_to_annexb() {
        // 4-byte length prefix: length=5, NAL data = [0x67, 0x42, 0x00, 0x1e, 0x9a]
        let avcc = vec![0x00, 0x00, 0x00, 0x05, 0x67, 0x42, 0x00, 0x1e, 0x9a];
        let annexb = avcc_to_annexb(&avcc, 4);

        assert_eq!(&annexb[0..4], &[0x00, 0x00, 0x00, 0x01]);
        assert_eq!(&annexb[4..], &[0x67, 0x42, 0x00, 0x1e, 0x9a]);
    }

    #[test]
    fn test_is_annexb() {
        assert!(is_annexb(&[0x00, 0x00, 0x00, 0x01, 0x67]));
        assert!(is_annexb(&[0x00, 0x00, 0x01, 0x67]));
        assert!(!is_annexb(&[0x00, 0x00, 0x00, 0x05, 0x67])); // AVCC
    }
}
