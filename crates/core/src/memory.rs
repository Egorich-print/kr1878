//! Memory model of the KR1878VE1: program memory (EEPROM of commands), data EEPROM
//! and the configuration word.

use crate::error::{Error, Result};

/// Size of the program (command) memory in words. 1K × 16 bits.
pub const PROGRAM_WORDS: usize = 1024;
/// Size of the data EEPROM in bytes (ЭСППЗУ данных, 64 × 8).
pub const DATA_EEPROM_BYTES: usize = 64;
/// Size of the EEPROM command memory image used by the programmer (0x400 words).
pub const PROGRAM_IMAGE_WORDS: usize = 1024;

/// The full programmer-facing image of the microcontroller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryImage {
    /// Program (command) memory, little-endian words.
    pub program: Vec<u16>,
    /// Data EEPROM bytes (64).
    pub data: [u8; DATA_EEPROM_BYTES],
    /// OSC/configuration word (optional).
    pub config: Option<u8>,
}

impl Default for MemoryImage {
    fn default() -> Self {
        MemoryImage {
            program: vec![0xFFFF; PROGRAM_WORDS],
            data: [0xFF; DATA_EEPROM_BYTES],
            config: None,
        }
    }
}

impl MemoryImage {
    /// Create an empty image. Program memory starts filled with `0xFFFF` so that
    /// erased regions are distinguishable in the listing.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create an image from an explicit program body (padded/truncated to the
    /// command-memory size).
    pub fn from_program(program: &[u16]) -> Result<Self> {
        if program.len() > PROGRAM_WORDS {
            return Err(Error::ProgramAddressOutOfRange {
                addr: program.len() as u32,
            });
        }
        let mut img = Self::default();
        img.program[..program.len()].copy_from_slice(program);
        Ok(img)
    }

    /// Read a program word; validates the address.
    pub fn program_word(&self, addr: u16) -> Result<u16> {
        if addr as usize >= self.program.len() {
            return Err(Error::ProgramAddressOutOfRange { addr: addr as u32 });
        }
        Ok(self.program[addr as usize])
    }

    /// Write a program word; validates the address.
    pub fn set_program_word(&mut self, addr: u16, value: u16) -> Result<()> {
        if addr as usize >= self.program.len() {
            return Err(Error::ProgramAddressOutOfRange { addr: addr as u32 });
        }
        self.program[addr as usize] = value;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_image_is_erased() {
        let img = MemoryImage::new();
        assert_eq!(img.program.len(), PROGRAM_WORDS);
        assert_eq!(img.program[0], 0xFFFF);
        assert_eq!(img.data[63], 0xFF);
    }

    #[test]
    fn from_program_pads() {
        let img = MemoryImage::from_program(&[0x1234, 0x5678]).unwrap();
        assert_eq!(img.program_word(0).unwrap(), 0x1234);
        assert_eq!(img.program_word(1).unwrap(), 0x5678);
        assert_eq!(img.program_word(2).unwrap(), 0xFFFF);
    }

    #[test]
    fn program_address_out_of_range() {
        let mut img = MemoryImage::new();
        assert!(img.set_program_word(1023, 1).is_ok());
        assert!(img.set_program_word(1024, 1).is_err());
        assert!(img.program_word(1024).is_err());
    }
}
