//! File-format readers/writers for the KR1878VE1 toolchain.
//!
//! * [`sav`]  — program (command) memory image.
//! * [`dat`]  — data EEPROM text file.
//! * [`arp`]  — programmer configuration block.

pub mod arp;
pub mod dat;
pub mod sav;

pub use arp::{ArpConfig, parse_arp, write_arp};
pub use dat::write_dat;
pub use sav::{read_sav, write_sav};
