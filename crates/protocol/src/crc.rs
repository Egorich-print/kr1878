//! CRC utilities, re-exported from `kr1878-core` where they live.
//!
//! The `.endc` cumulative checksum and the CRC-16/CCITT are part of the
//! file-format / toolchain behaviour, so they belong to the core crate;
//! `kr1878-protocol` re-exports them for compatibility.

pub use kr1878_core::crc::{add8_with_carry, crc16_ccitt, endc_checksum3};
