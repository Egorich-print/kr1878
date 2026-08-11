//! The `.sim` device-state snapshot format.
//!
//! Stores the full microcontroller state (program words, data EEPROM, optional
//! OSC/config byte) in a compact binary form, so a simulated device can survive
//! across separate CLI invocations. This is the canonical persistence format
//! for the `--backend sim` device only; physical backends do not use it.
//!
//! **Format:** `b"KRSI"` magic, `0x01` version, little-endian `u16[1024]`
//! program words, `u8[64]` data, config byte (`0xFF` = none, else the value).

use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::Path;

use kr1878_core::memory::{MemoryImage, PROGRAM_WORDS};
use thiserror::Error;

const MAGIC: &[u8; 4] = b"KRSI";
const VERSION: u8 = 0x01;
/// Sentinel in the config slot meaning "no config programmed".
pub const CONFIG_NONE: u8 = 0xFF;

/// Errors produced while reading/writing `.sim` snapshots.
#[derive(Debug, Error)]
pub enum SimError {
    #[error("sim: not a KR1878 snapshot (bad magic)")]
    BadMagic,
    #[error("sim: unsupported snapshot version {0} (expected {VERSION})")]
    BadVersion(u8),
    #[error("sim: I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Read a device-state snapshot into a [`MemoryImage`].
pub fn read_sim<R: Read>(reader: &mut R) -> Result<MemoryImage, SimError> {
    let mut magic = [0u8; 4];
    reader.read_exact(&mut magic)?;
    if &magic != MAGIC {
        return Err(SimError::BadMagic);
    }
    let mut version = [0u8; 1];
    reader.read_exact(&mut version)?;
    if version[0] != VERSION {
        return Err(SimError::BadVersion(version[0]));
    }

    let mut img = MemoryImage::new();
    img.program.clear();
    for _ in 0..PROGRAM_WORDS {
        let mut w = [0u8; 2];
        reader.read_exact(&mut w)?;
        img.program.push(u16::from_le_bytes(w));
    }
    reader.read_exact(&mut img.data)?;

    let mut cfg = [0u8; 1];
    reader.read_exact(&mut cfg)?;
    img.config = (cfg[0] != CONFIG_NONE).then_some(cfg[0]);
    Ok(img)
}

/// Write a device-state snapshot from a [`MemoryImage`].
pub fn write_sim<W: Write>(writer: &mut W, img: &MemoryImage) -> Result<(), SimError> {
    writer.write_all(MAGIC)?;
    writer.write_all(&[VERSION])?;
    for &word in img.program.iter().take(PROGRAM_WORDS) {
        writer.write_all(&word.to_le_bytes())?;
    }
    for _ in img.program.len()..PROGRAM_WORDS {
        writer.write_all(&0xFFFFu16.to_le_bytes())?;
    }
    writer.write_all(&img.data)?;
    let cfg = img.config.unwrap_or(CONFIG_NONE);
    writer.write_all(&[cfg])?;
    Ok(())
}

/// Write a snapshot to a file, creating or truncating it.
pub fn write_sim_file(path: &Path, img: &MemoryImage) -> Result<(), SimError> {
    let f = File::create(path)?;
    write_sim(&mut BufWriter::new(f), img)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kr1878_core::memory::DATA_EEPROM_BYTES;
    use std::io::Cursor;

    #[test]
    fn roundtrip_full_image() {
        let mut img = MemoryImage::new();
        img.program[0] = 0x1234;
        img.program[1023] = 0xDEAD;
        img.data = [0x5A; DATA_EEPROM_BYTES];
        img.config = Some(0x02);

        let mut out = Cursor::new(Vec::new());
        write_sim(&mut out, &img).unwrap();
        let mut inp = Cursor::new(out.into_inner());
        let got = read_sim(&mut inp).unwrap();
        assert_eq!(got, img);
    }

    #[test]
    fn roundtrip_no_config() {
        let mut img = MemoryImage::new();
        img.program[5] = 0x00FF;
        img.config = None;

        let mut out = Cursor::new(Vec::new());
        write_sim(&mut out, &img).unwrap();
        let mut inp = Cursor::new(out.into_inner());
        let got = read_sim(&mut inp).unwrap();
        assert_eq!(got, img);
    }

    #[test]
    fn rejects_bad_magic() {
        let mut inp = Cursor::new(b"XXXX\x01".to_vec());
        assert!(matches!(read_sim(&mut inp), Err(SimError::BadMagic)));
    }

    #[test]
    fn rejects_bad_version() {
        let mut inp = Cursor::new(b"KRSI\x09".to_vec());
        assert!(matches!(read_sim(&mut inp), Err(SimError::BadVersion(9))));
    }
}
