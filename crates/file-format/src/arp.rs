//! The programmer configuration block `e03prg.arp` (32 bytes).
//!
//! ```text
//! 0x00  magic u32 (LE) = 0x693C5A39        ("39 5A 3C 69" on disk)
//! 0x04  program_write   u32  (0 = enabled, 0xFFFFFFFF = disabled)
//! 0x08  config_write    u32
//! 0x0C  config2         u32
//! 0x10  data_write      u32  (data EEPROM)
//! 0x14  erase_option    u32 (0x01 = extended/erase mode)
//! ```

use thiserror::Error;

/// Size of the `.arp` block in bytes.
pub const ARP_SIZE: usize = 32;
/// Magic identifying a valid `.arp` block.
pub const ARP_MAGIC: u32 = 0x693C_5A39;

/// Parsed programmer configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArpConfig {
    pub program_write: bool,
    pub config_write: bool,
    pub config2: bool,
    pub data_write: bool,
    pub erase_option: bool,
}

/// Errors produced while parsing `.arp`.
#[derive(Debug, Error)]
pub enum ArpError {
    #[error("arp: bad magic (expected 0x{expected:08X}, got 0x{got:08X})")]
    BadMagic { expected: u32, got: u32 },
    #[error("arp: expected {expected} bytes, got {got}")]
    Size { expected: usize, got: usize },
}

/// Parse a 32-byte `.arp` block.
pub fn parse_arp(bytes: &[u8]) -> Result<ArpConfig, ArpError> {
    if bytes.len() < ARP_SIZE {
        return Err(ArpError::Size { expected: ARP_SIZE, got: bytes.len() });
    }
    let magic = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
    if magic != ARP_MAGIC {
        return Err(ArpError::BadMagic { expected: ARP_MAGIC, got: magic });
    }
    let dword = |off: usize| u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap());
    Ok(ArpConfig {
        program_write: dword(4) == 0,
        config_write: dword(8) == 0,
        config2: dword(12) == 0,
        data_write: dword(16) == 0,
        erase_option: dword(20) == 1,
    })
}

/// Serialize an [`ArpConfig`] back into `[u8; 32]` (little-endian).
pub fn write_arp(cfg: &ArpConfig) -> [u8; ARP_SIZE] {
    let mut out = [0u8; ARP_SIZE];
    out[0..4].copy_from_slice(&ARP_MAGIC.to_le_bytes());
    let dword = |on: bool| if on { 0u32 } else { 0xFFFF_FFFF };
    out[4..8].copy_from_slice(&dword(cfg.program_write).to_le_bytes());
    out[8..12].copy_from_slice(&dword(cfg.config_write).to_le_bytes());
    out[12..16].copy_from_slice(&dword(cfg.config2).to_le_bytes());
    out[16..20].copy_from_slice(&dword(cfg.data_write).to_le_bytes());
    out[20..24].copy_from_slice(&(cfg.erase_option as u32).to_le_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact bytes shipped with the original tool.
    const SAMPLE: [u8; 32] = [
        0x39, 0x5A, 0x3C, 0x69, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF,
        0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00,
    ];

    #[test]
    fn parses_shipped_sample() {
        let c = parse_arp(&SAMPLE).unwrap();
        assert!(c.program_write);
        assert!(c.config_write);
        assert!(!c.config2);
        assert!(!c.data_write);
        assert!(!c.erase_option);
    }

    #[test]
    fn bad_magic_rejected() {
        let mut b = SAMPLE;
        b[0] = 0x00;
        assert!(matches!(parse_arp(&b), Err(ArpError::BadMagic { .. })));
    }

    #[test]
    fn roundtrip() {
        let c = ArpConfig {
            program_write: false,
            config_write: true,
            config2: false,
            data_write: true,
            erase_option: true,
        };
        let bytes = write_arp(&c);
        assert_eq!(parse_arp(&bytes).unwrap(), c);
    }
}