//! CRC utilities.
//!
//! The original toolchain computes a per-program 3-byte running-sum checksum
//! when the source ends with `.endc` (see `docs/FILE_FORMATS.md` §1); the
//! result is stored in the body of the program. This module provides that
//! cumulative checksum plus a conventional CRC-16/CCITT, both required to stay
//! format-compatible.

/// 16-bit CRC with the CCITT polynomial `0x1021` (bit-reversed form `0x8408`)
/// and initial value `0xFFFF`, MSB-first.
pub fn crc16_ccitt(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &b in data {
        crc ^= (b as u16) << 8;
        for _ in 0..8 {
            if crc & 0x8000 != 0 {
                crc = (crc << 1) ^ 0x1021;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

/// 8-bit addition with carry propagation, used by the `.endc` cumulative sum.
pub fn add8_with_carry(a: u8, b: u8, carry: &mut u8) -> u8 {
    let sum = a as u16 + b as u16 + *carry as u16;
    *carry = (sum >> 8) as u8 & 1;
    (sum & 0xFF) as u8
}

/// Compute the 3-byte cumulative checksum over a stream of 16-bit words, as the
/// original `.endc` directive does. Returns three bytes (low..high) with carries
/// folded through the accumulation.
pub fn endc_checksum3(words: impl Iterator<Item = u16>) -> [u8; 3] {
    let mut b0: u8 = 0;
    let mut b1: u8 = 0;
    let mut b2: u8 = 0;
    let mut carry: u8 = 0;
    for w in words {
        let lo = (w & 0xFF) as u8;
        let hi = ((w >> 8) & 0xFF) as u8;
        b0 = add8_with_carry(b0, lo, &mut carry);
        b1 = add8_with_carry(b1, hi, &mut carry);
        // propagate any final carry through the third accumulation
        b2 = add8_with_carry(b2, 0, &mut carry);
    }
    [b0, b1, b2]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc16_known_vector() {
        // CRC-16/CCITT-FALSE of "123456789" is 0x29B1.
        assert_eq!(crc16_ccitt(b"123456789"), 0x29B1);
    }

    #[test]
    fn add_with_carry_propagates() {
        let mut carry = 0;
        assert_eq!(add8_with_carry(0xFF, 0x01, &mut carry), 0x00);
        assert_eq!(carry, 1);
        assert_eq!(add8_with_carry(0x00, 0x00, &mut carry), 0x01);
    }

    #[test]
    fn endc_checksum_is_reproducible() {
        let words = [0x0102u16, 0x0304u16, 0x0506u16];
        let a = endc_checksum3(words.iter().copied());
        let b = endc_checksum3(words.iter().copied());
        assert_eq!(a, b);
        assert_eq!(a.len(), 3);
    }
}