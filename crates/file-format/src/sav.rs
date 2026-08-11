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

/// Number of header words the official `Uprg1878.exe` skips in a `.sav` file
/// (`SEEK(F_W, 0x100)` on a `file of Word` → 256 words = 512 bytes).
///
/// The official (Angstrom/TESSA) `.sav` files carry a 256-word header before the
/// 1024 program words; our own `.sav` files are headerless (see `docs/FILE_FORMATS.md`).
pub const UPRG_HEADER_WORDS: usize = 256;

/// Read a `.sav` produced by the official toolchain: skip the 256-word header
/// (via [`UPRG_HEADER_WORDS`]) then read [`PROGRAM_IMAGE_WORDS`] little-endian
/// words. A truncated or empty body is padded with `0xFFFF` (erased state).
///
/// This mirrors `Uprg1878.exe`'s `SEEK(F_W, 0x100)` before the program phase.
pub fn read_sav_uprg<R: Read>(reader: &mut R) -> Result<Vec<u16>, SavError> {
    let mut skipped = 0usize;
    let mut buf = [0u8; 2];
    while skipped < UPRG_HEADER_WORDS {
        let n = reader.read(&mut buf)?;
        if n < 2 {
            return read_sav(reader);
        }
        skipped += 1;
    }
    read_sav(reader)
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

/// Write a `.sav` in the official `Uprg1878.exe` layout: a [`UPRG_HEADER_WORDS`]-
/// word header followed by the program words. The header is written as `0xFFFF`
/// (erased state), mirroring the reserve area the official toolchain skips.
pub fn write_sav_uprg<W: Write>(writer: &mut W, words: &[u16]) -> Result<(), SavError> {
    if words.len() > PROGRAM_IMAGE_WORDS {
        return Err(SavError::TooLarge { words: words.len() });
    }
    for index in 0..UPRG_HEADER_WORDS {
        writer
            .write_all(&0xFFFFu16.to_le_bytes())
            .map_err(|source| SavError::Read { index, source })?;
    }
    write_sav(writer, words)
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

    #[test]
    fn uprg_reader_skips_256_word_header() {
        // Build an official-style file: 256 header words + 3 data words.
        let mut bytes = vec![0u8; UPRG_HEADER_WORDS * 2];
        // Header filled with 0xAAAA.
        for b in bytes.chunks_mut(2) {
            b.copy_from_slice(&0xAAAAu16.to_le_bytes());
        }
        bytes.extend_from_slice(&0x1234u16.to_le_bytes());
        bytes.extend_from_slice(&0x5678u16.to_le_bytes());
        bytes.extend_from_slice(&0x9ABCu16.to_le_bytes());

        let got = read_sav_uprg(&mut Cursor::new(bytes)).unwrap();
        assert_eq!(got.len(), PROGRAM_IMAGE_WORDS);
        assert_eq!(got[0], 0x1234);
        assert_eq!(got[1], 0x5678);
        assert_eq!(got[2], 0x9ABC);
        // The tail after the data words is padded with the erased state.
        assert!(got[3..].iter().all(|&w| w == 0xFFFF));
    }

    #[test]
    fn uprg_reader_short_file_degrades_to_plain_read() {
        // A headerless (our own) file read through the uprg reader: the
        // "header" simply consumes the leading words and the rest is data.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0x1111u16.to_le_bytes());
        bytes.extend_from_slice(&0x2222u16.to_le_bytes());
        let got = read_sav_uprg(&mut Cursor::new(bytes)).unwrap();
        assert_eq!(got[0], 0xFFFF); // header consumed past EOF -> padded
    }

    #[test]
    fn uprg_writer_prepends_header() {
        let words = vec![0x1234u16, 0x5678];
        let mut out = Cursor::new(Vec::new());
        write_sav_uprg(&mut out, &words).unwrap();
        let bytes = out.into_inner();
        assert_eq!(bytes.len(), (UPRG_HEADER_WORDS + words.len()) * 2);
        // The header is erased state.
        for b in bytes[..UPRG_HEADER_WORDS * 2].chunks_exact(2) {
            assert_eq!(b, &0xFFFFu16.to_le_bytes());
        }
        // The body follows at word offset UPRG_HEADER_WORDS.
        let got = read_sav_uprg(&mut Cursor::new(bytes)).unwrap();
        assert_eq!(&got[..2], &words[..]);
    }
}
