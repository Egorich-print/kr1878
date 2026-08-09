//! In-memory programmer backend. Used for tests, CI and the `mock` backend.

use kr1878_core::backend::{Backend, DeviceId};
use kr1878_core::error::Result;
use kr1878_core::memory::{DATA_EEPROM_BYTES, PROGRAM_WORDS, MemoryImage};

/// A fully in-memory [`Backend`]; erased cells read back as `0xFFFF`.
#[derive(Debug, Clone)]
pub struct MockBackend {
    image: MemoryImage,
}

impl Default for MockBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl MockBackend {
    pub fn new() -> Self {
        MockBackend { image: MemoryImage::new() }
    }

    pub fn image(&self) -> &MemoryImage {
        &self.image
    }
}

impl Backend for MockBackend {
    fn identify(&mut self) -> Result<DeviceId> {
        Ok(DeviceId { name: "mock (in-memory)".into(), revision: None })
    }

    fn erase(&mut self) -> Result<()> {
        self.image = MemoryImage::new();
        Ok(())
    }

    fn write_program(&mut self, words: &[u16]) -> Result<()> {
        let n = words.len().min(PROGRAM_WORDS);
        self.image.program[..n].copy_from_slice(&words[..n]);
        Ok(())
    }

    fn write_data(&mut self, bytes: &[u8; DATA_EEPROM_BYTES]) -> Result<()> {
        self.image.data = *bytes;
        Ok(())
    }

    fn read_program(&mut self, out: &mut [u16]) -> Result<()> {
        out.copy_from_slice(&self.image.program[..out.len()]);
        Ok(())
    }

    fn read_data(&mut self) -> Result<[u8; DATA_EEPROM_BYTES]> {
        Ok(self.image.data)
    }

    fn program(&mut self, image: &MemoryImage) -> Result<bool> {
        self.image = image.clone();
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_write_and_read() {
        let mut b = MockBackend::new();
        let mut img = MemoryImage::new();
        img.program[5] = 0x1234;
        img.data[0] = 0xAB;
        b.program(&img).unwrap();
        let mut out = vec![0u16; PROGRAM_WORDS];
        b.read_program(&mut out).unwrap();
        assert_eq!(out[5], 0x1234);
        assert_eq!(b.read_data().unwrap()[0], 0xAB);
    }

    #[test]
    fn erase_returns_to_unprogrammed() {
        let mut b = MockBackend::new();
        b.write_program(&[0x1234]).unwrap();
        b.erase().unwrap();
        let mut out = vec![0u16; PROGRAM_WORDS];
        b.read_program(&mut out).unwrap();
        assert_eq!(out[0], 0xFFFF);
    }

    #[test]
    fn write_data_is_full_slice() {
        let mut b = MockBackend::new();
        let bytes = [0x11; DATA_EEPROM_BYTES];
        b.write_data(&bytes).unwrap();
        assert_eq!(b.read_data().unwrap(), bytes);
    }
}
