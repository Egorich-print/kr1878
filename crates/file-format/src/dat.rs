//! The data EEPROM text file (`.dat`).
//!
//! Format (confirmed from the shipped `EEPROM.DAT` sample):
//! * space-separated two-digit hex bytes, 8 bytes per line;
//! * `;` starts a comment that runs to end of line;
//! * file ends with `0x1A` (EOF marker) which is ignored;
//! * exactly 64 data bytes.

use std::io::Write;

use kr1878_core::memory::DATA_EEPROM_BYTES;
use thiserror::Error;

/// Errors produced while reading/writing `.dat` files.
#[derive(Debug, Error)]
pub enum DatError {
    #[error("dat: expected exactly {expected} data bytes, got {got}")]
    Size { expected: usize, got: usize },
    #[error("dat: I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("dat: invalid hex token '{0}'")]
    InvalidHex(String),
    #[error("dat: expected {expected} bytes, got {got}")]
    ParseSize { expected: usize, got: usize },
}

/// Parse a `.dat` text file into exactly 64 data bytes.
///
/// Rules: two-digit hex bytes separated by whitespace; `;` starts a comment to
/// the end of the line; `0x1A` (^Z) ends the file; non-hex content is skipped.
pub fn parse_dat(text: &str) -> Result<[u8; DATA_EEPROM_BYTES], DatError> {
    let mut out = [0xFFu8; DATA_EEPROM_BYTES];
    let mut idx = 0usize;
    // Cut at the historical EOF marker first.
    let body = text.split('\u{1A}').next().unwrap_or("");
    for line in body.lines() {
        if idx >= DATA_EEPROM_BYTES {
            break;
        }
        let code = line.split(';').next().unwrap_or("");
        for tok in code.split_whitespace() {
            if idx >= DATA_EEPROM_BYTES {
                return Err(DatError::ParseSize { expected: DATA_EEPROM_BYTES, got: idx });
            }
            let v = u8::from_str_radix(tok, 16).map_err(|_| DatError::InvalidHex(tok.into()))?;
            out[idx] = v;
            idx += 1;
        }
    }
    Ok(out)
}

/// Write the data EEPROM content in the historical text format.
pub fn write_dat<W: Write>(writer: &mut W, bytes: &[u8]) -> Result<(), DatError> {
    if bytes.len() != DATA_EEPROM_BYTES {
        return Err(DatError::Size { expected: DATA_EEPROM_BYTES, got: bytes.len() });
    }
    for (i, chunk) in bytes.chunks(8).enumerate() {
        let line: Vec<String> = chunk.iter().map(|b| format!("{b:02x}")).collect();
        writeln!(writer, "{} ;{}...{} Bytes", line.join(" "), i * 8, i * 8 + 7)?;
    }
    // Historical EOF marker.
    writer.write_all(&[0x1A])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_expected_layout() {
        let mut out = Vec::new();
        write_dat(&mut out, &[0xFF; 64]).unwrap();
        let text = String::from_utf8_lossy(&out[..out.len() - 1]); // drop 0x1A
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 8);
        assert!(lines[0].starts_with("ff ff ff ff ff ff ff ff ;0...7 Bytes"));
        assert_eq!(out.last(), Some(&0x1A));
    }

    #[test]
    fn rejects_wrong_size() {
        assert!(write_dat(&mut Vec::new(), &[0u8; 63]).is_err());
        assert!(write_dat(&mut Vec::new(), &[0u8; 65]).is_err());
    }

    #[test]
    fn parses_sample_format() {
        let text = "; test\nff ff ff ff ff ff ff ff\n01 02 ff";
        let bytes = parse_dat(text).unwrap();
        assert_eq!(bytes[0], 0xFF);
        assert_eq!(bytes[8], 0x01);
        assert_eq!(bytes[9], 0x02);
        assert_eq!(bytes[10], 0xFF);
    }

    #[test]
    fn parse_skips_comments() {
        let text = "ff;comment\n0a\n0b\n";
        let bytes = parse_dat(text).unwrap();
        assert_eq!(bytes[0], 0xFF);
        assert_eq!(bytes[1], 0x0A);
        assert_eq!(bytes[2], 0x0B);
        assert_eq!(bytes[3], 0xFF);
    }

    #[test]
    fn parse_rejects_bad_hex() {
        let text = "ff zz";
        assert!(parse_dat(text).is_err());
    }
}