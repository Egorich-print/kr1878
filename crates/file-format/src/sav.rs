//! The `.sav` program-memory image format.
//!
//! **Format:** a raw sequence of little-endian `u16` words (no header). This is the
//! closest available match to the historical toolchain behaviour (see
//! `docs/FILE_FORMATS.md` and `docs/adr/ADR-000-sav-format.md`), where the
//! programmer buffer is fed directly as 16-bit words.

use std::io::{Read, Write};

use kr1878_core::memory::PROGRAM_IMAGE_WORDS;
use thiserror::Error;

/// Errors produced while reading/writing `.sav` files.
#[derive(Debug, Error)]
pub enum SavError {
    #[error("sav: could not read program word at index {index}: {source}")]
    Read {
        index: usize,
        source: std::io::Error,
    },
    #[error("sav: program image larger than program memory ({words} words)")]
    TooLarge { words: usize },
    #[error("sav: I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Read a `.sav` file into a program-word vector.
///
/// Reads until EOF, truncating/padding to [`PROGRAM_IMAGE_WORDS`]. An empty or
/// truncated file yields the missing tail as `0xFFFF` (erased state).
pub fn read_sav<R: Read>(reader: &mut R) -> Result<Vec<u16>, SavError> {
    let mut words: Vec<u16> = Vec::with_capacity(PROGRAM_IMAGE_WORDS);
    let mut buf = [0u8; 2];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        if n < 2 {
            // Odd trailing byte — ignore it (historical leniency).
            break;
        }
        let word = u16::from_le_bytes(buf);
        words.push(word);
        if words.len() >= PROGRAM_IMAGE_WORDS {
            break;
        }
    }
    while words.len() < PROGRAM_IMAGE_WORDS {
        words.push(0xFFFF);
    }
    Ok(words)
}

/// Write a `.sav` image as little-endian `u16` words.
pub fn write_sav<W: Write>(writer: &mut W, words: &[u16]) -> Result<(), SavError> {
    if words.len() > PROGRAM_IMAGE_WORDS {
        return Err(SavError::TooLarge { words: words.len() });
    }
    for (index, &word) in words.iter().enumerate() {
        writer
            .write_all(&word.to_le_bytes())
            .map_err(|source| SavError::Read { index, source })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn roundtrip() {
        let words = vec![0x1234u16, 0xABCD, 0x0001, 0xFFFF];
        let mut out = Cursor::new(Vec::new());
        write_sav(&mut out, &words).unwrap();
        let bytes = out.into_inner();
        assert_eq!(bytes, [0x34, 0x12, 0xCD, 0xAB, 0x01, 0x00, 0xFF, 0xFF]);

        let mut inp = Cursor::new(bytes);
        let got = read_sav(&mut inp).unwrap();
        assert_eq!(&got[..4], &words[..]);
        assert_eq!(got.len(), PROGRAM_IMAGE_WORDS);
        // Tail padded with erased state.
        assert!(got[4..].iter().all(|&w| w == 0xFFFF));
    }

    #[test]
    fn empty_input_pads() {
        let got = read_sav(&mut Cursor::new(vec![])).unwrap();
        assert_eq!(got.len(), PROGRAM_IMAGE_WORDS);
        assert!(got.iter().all(|&w| w == 0xFFFF));
    }

    #[test]
    fn trailing_odd_byte_ignored() {
        let got = read_sav(&mut Cursor::new(vec![0x01u8])).unwrap();
        assert_eq!(got[0], 0xFFFF);
    }

    #[test]
    fn too_large_rejected() {
        let mut out = Cursor::new(Vec::new());
        let big = vec![0u16; PROGRAM_IMAGE_WORDS + 1];
        assert!(write_sav(&mut out, &big).is_err());
    }
}
