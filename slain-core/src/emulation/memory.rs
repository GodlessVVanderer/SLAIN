//! Memory subsystems for various platforms

/// Generic RAM block
#[derive(Clone)]
pub struct Ram {
    data: Vec<u8>,
    mask: usize,
}

impl Ram {
    pub fn new(size: usize) -> Self {
        Self {
            data: vec![0; size],
            mask: size - 1,
        }
    }

    pub fn read(&self, addr: u16) -> u8 {
        self.data[addr as usize & self.mask]
    }

    pub fn write(&mut self, addr: u16, val: u8) {
        self.data[addr as usize & self.mask] = val;
    }

    pub fn read32(&self, addr: u32) -> u8 {
        self.data[addr as usize & self.mask]
    }

    pub fn write32(&mut self, addr: u32, val: u8) {
        self.data[addr as usize & self.mask] = val;
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.data
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.data
    }

    pub fn clear(&mut self) {
        self.data.fill(0);
    }
}

/// Generic ROM block
pub struct Rom {
    data: Vec<u8>,
}

impl Rom {
    pub fn new(data: Vec<u8>) -> Self {
        Self { data }
    }

    pub fn from_slice(data: &[u8]) -> Self {
        Self { data: data.to_vec() }
    }

    pub fn read(&self, addr: usize) -> u8 {
        self.data.get(addr).copied().unwrap_or(0xFF)
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.data
    }
}

/// Large RAM for 32-bit systems (Dreamcast/Atomiswave)
pub struct Ram32 {
    data: Vec<u8>,
}

impl Ram32 {
    pub fn new(size: usize) -> Self {
        Self {
            data: vec![0; size],
        }
    }

    pub fn read8(&self, addr: u32) -> u8 {
        self.data.get(addr as usize).copied().unwrap_or(0)
    }

    pub fn read16(&self, addr: u32) -> u16 {
        let addr = addr as usize;
        if addr + 1 < self.data.len() {
            u16::from_le_bytes([self.data[addr], self.data[addr + 1]])
        } else {
            0
        }
    }

    pub fn read32(&self, addr: u32) -> u32 {
        let addr = addr as usize;
        if addr + 3 < self.data.len() {
            u32::from_le_bytes([
                self.data[addr],
                self.data[addr + 1],
                self.data[addr + 2],
                self.data[addr + 3],
            ])
        } else {
            0
        }
    }

    pub fn read64(&self, addr: u32) -> u64 {
        let addr = addr as usize;
        if addr + 7 < self.data.len() {
            u64::from_le_bytes([
                self.data[addr],
                self.data[addr + 1],
                self.data[addr + 2],
                self.data[addr + 3],
                self.data[addr + 4],
                self.data[addr + 5],
                self.data[addr + 6],
                self.data[addr + 7],
            ])
        } else {
            0
        }
    }

    pub fn write8(&mut self, addr: u32, val: u8) {
        if let Some(byte) = self.data.get_mut(addr as usize) {
            *byte = val;
        }
    }

    pub fn write16(&mut self, addr: u32, val: u16) {
        let bytes = val.to_le_bytes();
        let addr = addr as usize;
        if addr + 1 < self.data.len() {
            self.data[addr] = bytes[0];
            self.data[addr + 1] = bytes[1];
        }
    }

    pub fn write32(&mut self, addr: u32, val: u32) {
        let bytes = val.to_le_bytes();
        let addr = addr as usize;
        if addr + 3 < self.data.len() {
            self.data[addr] = bytes[0];
            self.data[addr + 1] = bytes[1];
            self.data[addr + 2] = bytes[2];
            self.data[addr + 3] = bytes[3];
        }
    }

    pub fn write64(&mut self, addr: u32, val: u64) {
        let bytes = val.to_le_bytes();
        let addr = addr as usize;
        if addr + 7 < self.data.len() {
            for (i, byte) in bytes.iter().enumerate() {
                self.data[addr + i] = *byte;
            }
        }
    }

    pub fn load(&mut self, offset: usize, data: &[u8]) {
        let end = (offset + data.len()).min(self.data.len());
        let len = end - offset;
        self.data[offset..end].copy_from_slice(&data[..len]);
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.data
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.data
    }

    pub fn clear(&mut self) {
        self.data.fill(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ram() {
        let mut ram = Ram::new(0x800);
        ram.write(0x100, 0x42);
        assert_eq!(ram.read(0x100), 0x42);
        // Test mirroring
        assert_eq!(ram.read(0x100), ram.read(0x900 & 0x7FF));
    }

    #[test]
    fn test_ram32() {
        let mut ram = Ram32::new(0x10000);
        ram.write32(0x100, 0x12345678);
        assert_eq!(ram.read32(0x100), 0x12345678);
        assert_eq!(ram.read16(0x100), 0x5678);
        assert_eq!(ram.read8(0x100), 0x78);
    }
}
