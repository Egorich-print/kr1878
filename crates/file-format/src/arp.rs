//! The programmer configuration block `e03prg.arp`.
//!
//! Two layouts exist, and the original one is *not* a set of flags.
//!
//! # The original file (66 bytes)
//!
//! Found in the Angstrem distribution next to the programmer (`e03prg.arp`):
//!
//! ```text
//! 0x00  magic u32 (LE) = 0x693C5A39        ("39 5A 3C 69" on disk)
//! 0x04  u32 — meaning unknown
//! 0x08  u32 — meaning unknown
//! 0x0C  u8  — length of the path (including the terminating NUL)
//! 0x0D  the path of the image to program, CP1251, NUL-terminated
//! ...    padding
//! ```
//!
//! So the original file says *which image to program* — it does not select
//! the phases. The checkboxes "Program-write / Config-write / EEPROM-write"
//! belong to the debug environment (DebugMCT), not to this file.
//!
//! # The settings block (32 bytes)
//!
//! Our own reconstruction of the programmer's settings, used when we write a
//! configuration ourselves:
//!
//! ```text
//! 0x00  magic u32 (LE) = 0x693C5A39
//! 0x04  program_write   u32  (0 = enabled, 0xFFFFFFFF = disabled)
//! 0x08  config_write    u32
//! 0x0C  config2         u32
//! 0x10  data_write      u32  (data EEPROM)
//! 0x14  erase_option    u32 (0x01 = extended/erase mode)
//! ```
//!
//! Reading a file of the original layout therefore must not be interpreted as
//! a set of flags: the flags live at offsets where the original file holds a
//! path, and the "config write" flag would silently read as "disabled".

use thiserror::Error;

/// Size of the settings block we write, in bytes.
pub const ARP_SIZE: usize = 32;
/// Size of the original file, in bytes.
pub const ARP_ORIGINAL_SIZE: usize = 66;
/// Magic identifying a valid `.arp` block.
pub const ARP_MAGIC: u32 = 0x693C_5A39;

/// CP1251 for the upper half of the byte range (0x80…0xFF).
const CP1251_HIGH: &str = concat!(
    "АБВГДЕЖЗИЙКЛМНОПРСТУФХЦЧШЩЪЫЬЭЮЯ",
    "абвгдежзийклмнопрстуфхцчшщъыьэюя",
    "АБВГДЕЖЗИЙКЛМНОПРСТУФХЦЧШЩЪЫЬЭЮЯ",
    "абвгдежзийклмнопрстуфхцчшщъыьэюя",
);

/// Decode a byte string as CP1251 (the Windows codepage of the original
/// tools).
fn decode_cp1251(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| {
            if b < 0x80 {
                b as char
            } else {
                CP1251_HIGH
                    .chars()
                    .nth((b - 0x80) as usize)
                    .unwrap_or('\u{FFFD}')
            }
        })
        .collect()
}

/// Parsed programmer configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArpConfig {
    /// The path of the image to program, if the block is of the original
    /// layout.
    pub image_path: Option<String>,
    /// Whether the program phase is enabled (only meaningful for the
    /// settings layout; an original file never disables a phase).
    pub program_write: bool,
    /// Whether the configuration phase is enabled.
    pub config_write: bool,
    /// The second configuration option.
    pub config2: bool,
    /// Whether the data EEPROM phase is enabled.
    pub data_write: bool,
    /// The erase/extended option.
    pub erase_option: bool,
}

/// Errors produced while parsing `.arp`.
#[derive(Debug, Error)]
pub enum ArpError {
    #[error("arp: bad magic (expected 0x{expected:08X}, got 0x{got:08X})")]
    BadMagic { expected: u32, got: u32 },
    #[error("arp: expected at least {expected} bytes, got {got}")]
    Size { expected: usize, got: usize },
}

/// Parse an `.arp` block of either layout.
///
/// An original file (the path of the image) enables every phase — it does not
/// carry the phase flags.
pub fn parse_arp(bytes: &[u8]) -> Result<ArpConfig, ArpError> {
    if bytes.len() < 13 {
        return Err(ArpError::Size {
            expected: 13,
            got: bytes.len(),
        });
    }
    let magic = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
    if magic != ARP_MAGIC {
        return Err(ArpError::BadMagic {
            expected: ARP_MAGIC,
            got: magic,
        });
    }

    if let Some(path) = original_path(bytes) {
        return Ok(ArpConfig {
            image_path: Some(path),
            program_write: true,
            config_write: true,
            config2: false,
            data_write: true,
            erase_option: false,
        });
    }

    if bytes.len() < ARP_SIZE {
        return Err(ArpError::Size {
            expected: ARP_SIZE,
            got: bytes.len(),
        });
    }
    let dword = |off: usize| u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap());
    Ok(ArpConfig {
        image_path: None,
        program_write: dword(4) == 0,
        config_write: dword(8) == 0,
        config2: dword(12) == 0,
        data_write: dword(16) == 0,
        erase_option: dword(20) == 1,
    })
}

/// Recognise the original layout and extract the path of the image.
fn original_path(bytes: &[u8]) -> Option<String> {
    let len = bytes[12] as usize;
    if len == 0 || 13 + len > bytes.len() {
        return None;
    }
    // The length counts the path itself; the terminating NUL follows it.
    let raw = &bytes[13..13 + len];
    let text = raw.strip_suffix(&[0u8]).unwrap_or(raw);
    if text.is_empty() || !text.iter().all(|&b| (0x20..0x7F).contains(&b) || b >= 0x80) {
        return None;
    }
    Some(decode_cp1251(text))
}

/// Serialize an [`ArpConfig`] into the settings block (little-endian).
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

/// Serialize the original layout: the path of the image to program.
pub fn write_arp_with_path(path: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(ARP_ORIGINAL_SIZE);
    out.extend_from_slice(&ARP_MAGIC.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&1u32.to_le_bytes());
    let raw: Vec<u8> = path.chars().map(encode_cp1251).collect();
    out.push(raw.len() as u8);
    out.extend_from_slice(&raw);
    out.push(0);
    out.resize(ARP_ORIGINAL_SIZE, 0xFF);
    out
}

/// Encode a character as CP1251; characters outside the codepage become '?'.
fn encode_cp1251(c: char) -> u8 {
    if c.is_ascii() {
        return c as u8;
    }
    CP1251_HIGH
        .chars()
        .position(|x| x == c)
        .map(|i| (i + 0x80) as u8)
        .unwrap_or(b'?')
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The settings block we used to believe was the original format.
    const SETTINGS: [u8; 32] = [
        0x39, 0x5A, 0x3C, 0x69, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF,
        0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00,
    ];

    /// The original `e03prg.arp` from the Angstrem distribution: the path of
    /// the image to program ("C:\WINDOWS\Рабочий стол\BE1\f.sav" in CP1251).
    const ORIGINAL: [u8; 66] = [
        0x39, 0x5A, 0x3C, 0x69, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x21, 0x43, 0x3A,
        0x5C, 0x57, 0x49, 0x4E, 0x44, 0x4F, 0x57, 0x53, 0x5C, 0xD0, 0xE0, 0xE1, 0xEE, 0xF7, 0xE8,
        0xE9, 0x20, 0xF1, 0xF2, 0xEE, 0xEB, 0x5C, 0x42, 0x45, 0x31, 0x5C, 0x66, 0x2E, 0x73, 0x61,
        0x76, 0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
        0xFF, 0xFF, 0x01, 0x00, 0x00, 0x00,
    ];

    #[test]
    fn parses_the_settings_block() {
        let c = parse_arp(&SETTINGS).unwrap();
        assert_eq!(c.image_path, None);
        assert!(c.program_write);
        assert!(c.config_write);
        assert!(!c.config2);
        assert!(!c.data_write);
        assert!(!c.erase_option);
    }

    #[test]
    fn parses_the_original_file() {
        let c = parse_arp(&ORIGINAL).unwrap();
        assert_eq!(
            c.image_path.as_deref(),
            Some(
                "C:\\WINDOWS\\\u{420}\u{430}\u{431}\u{43E}\u{447}\u{438}\u{439} \u{441}\u{442}\u{43E}\u{43B}\\BE1\\f.sav"
            )
        );
        // An original file never disables a phase.
        assert!(c.program_write);
        assert!(c.config_write);
        assert!(c.data_write);
    }

    #[test]
    fn original_file_round_trip() {
        let path = "C:\\WINDOWS\\\u{420}\u{430}\u{431}\u{43E}\u{447}\u{438}\u{439} \u{441}\u{442}\u{43E}\u{43B}\\BE1\\f.sav";
        let bytes = write_arp_with_path(path);
        assert_eq!(bytes.len(), ARP_ORIGINAL_SIZE);
        assert_eq!(parse_arp(&bytes).unwrap().image_path.as_deref(), Some(path));
    }

    #[test]
    fn bad_magic_rejected() {
        let mut b = SETTINGS;
        b[0] = 0x00;
        assert!(matches!(parse_arp(&b), Err(ArpError::BadMagic { .. })));
    }

    #[test]
    fn short_file_rejected() {
        assert!(matches!(
            parse_arp(&[0x39, 0x5A]),
            Err(ArpError::Size { .. })
        ));
    }

    #[test]
    fn roundtrip() {
        let c = ArpConfig {
            image_path: None,
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
