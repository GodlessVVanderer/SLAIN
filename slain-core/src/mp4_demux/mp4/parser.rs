//! MP4 box parsing helpers.

use bytes::Buf;
use std::io::Read;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoxHeader {
    pub size: u64,
    pub box_type: [u8; 4],
    pub header_size: u64,
}

pub fn read_box_header<R: Read>(reader: &mut R) -> Result<BoxHeader, String> {
    let mut header = [0u8; 8];
    reader
        .read_exact(&mut header)
        .map_err(|e| format!("Read error: {e}"))?;
    let mut cursor = &header[..];
    let size = cursor.get_u32() as u64;
    let mut box_type = [0u8; 4];
    cursor.copy_to_slice(&mut box_type);

    let (size, header_size) = if size == 1 {
        let mut ext = [0u8; 8];
        reader
            .read_exact(&mut ext)
            .map_err(|e| format!("Read error: {e}"))?;
        let mut ext_cursor = &ext[..];
        let ext_size = ext_cursor.get_u64();
        if ext_size < 16 {
            return Err("Invalid extended box size".to_string());
        }
        (ext_size, 16)
    } else if size == 0 {
        (0, 8)
    } else {
        if size < 8 {
            return Err("Invalid box size".to_string());
        }
        (size, 8)
    };

    Ok(BoxHeader {
        size,
        box_type,
        header_size,
    })
}

pub fn read_u8<R: Read>(reader: &mut R) -> Result<u8, String> {
    let mut buf = [0u8; 1];
    reader
        .read_exact(&mut buf)
        .map_err(|e| format!("Read error: {e}"))?;
    let mut cursor = &buf[..];
    Ok(cursor.get_u8())
}

pub fn read_u16<R: Read>(reader: &mut R) -> Result<u16, String> {
    let mut buf = [0u8; 2];
    reader
        .read_exact(&mut buf)
        .map_err(|e| format!("Read error: {e}"))?;
    let mut cursor = &buf[..];
    Ok(cursor.get_u16())
}

pub fn read_u32<R: Read>(reader: &mut R) -> Result<u32, String> {
    let mut buf = [0u8; 4];
    reader
        .read_exact(&mut buf)
        .map_err(|e| format!("Read error: {e}"))?;
    let mut cursor = &buf[..];
    Ok(cursor.get_u32())
}

pub fn read_u64<R: Read>(reader: &mut R) -> Result<u64, String> {
    let mut buf = [0u8; 8];
    reader
        .read_exact(&mut buf)
        .map_err(|e| format!("Read error: {e}"))?;
    let mut cursor = &buf[..];
    Ok(cursor.get_u64())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_box_header() {
        let data = b"\x00\x00\x00\x10ftyp";
        let mut cursor = &data[..];
        let header = read_box_header(&mut cursor).expect("header");
        assert_eq!(header.size, 16);
        assert_eq!(header.box_type, *b"ftyp");
        assert_eq!(header.header_size, 8);
    }

    #[test]
    fn parses_extended_box_header() {
        let data = b"\x00\x00\x00\x01mdat\x00\x00\x00\x00\x00\x00\x00\x20";
        let mut cursor = &data[..];
        let header = read_box_header(&mut cursor).expect("header");
        assert_eq!(header.size, 32);
        assert_eq!(header.box_type, *b"mdat");
        assert_eq!(header.header_size, 16);
    }

    #[test]
    fn rejects_too_small_box() {
        let data = b"\x00\x00\x00\x07free";
        let mut cursor = &data[..];
        let err = read_box_header(&mut cursor).unwrap_err();
        assert!(err.contains("Invalid box size"));
    }

    #[test]
    fn allows_zero_sized_box() {
        let data = b"\x00\x00\x00\x00mdat";
        let mut cursor = &data[..];
        let header = read_box_header(&mut cursor).expect("header");
        assert_eq!(header.size, 0);
        assert_eq!(header.box_type, *b"mdat");
    }
}
