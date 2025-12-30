//! Minimal MKV/EBML parsing helpers.

use bytes::Buf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Vint {
    pub length: usize,
    pub value: u64,
}

pub fn read_vint<B: Buf>(buf: &mut B) -> Result<Vint, String> {
    if !buf.has_remaining() {
        return Err("Missing vint byte".to_string());
    }

    let first = buf.get_u8();
    let mut mask = 0x80u8;
    let mut length = 1usize;

    while length <= 8 && (first & mask) == 0 {
        mask >>= 1;
        length += 1;
    }

    if length > 8 {
        return Err("Invalid vint length".to_string());
    }

    let mut value = (first & (!mask)) as u64;
    for _ in 1..length {
        if !buf.has_remaining() {
            return Err("Truncated vint".to_string());
        }
        value = (value << 8) | buf.get_u8() as u64;
    }

    Ok(Vint { length, value })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_single_byte_vint() {
        let mut data = &b"\x81"[..];
        let vint = read_vint(&mut data).expect("vint");
        assert_eq!(vint.length, 1);
        assert_eq!(vint.value, 0x01);
    }

    #[test]
    fn reads_two_byte_vint() {
        let mut data = &b"\x40\x7F"[..];
        let vint = read_vint(&mut data).expect("vint");
        assert_eq!(vint.length, 2);
        assert_eq!(vint.value, 0x7F);
    }

    #[test]
    fn fails_on_truncated_vint() {
        let mut data = &b"\x40"[..];
        let err = read_vint(&mut data).unwrap_err();
        assert!(err.contains("Truncated"));
    }
}
