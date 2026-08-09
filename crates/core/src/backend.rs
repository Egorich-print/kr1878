//! Abstract programmer backend trait. Hardware implementations live in the
//! `kr1878-programmer` crate; the core depends only on this trait so that the
//! business logic stays platform-agnostic and testable.

use crate::{
    error::Result,
    memory::{DATA_EEPROM_BYTES, MemoryImage},
};

/// Identity data reported by [`Backend::identify`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceId {
    pub name: String,
    pub revision: Option<String>,
}

/// A driver for the KR1878VE1 programming hardware. Kept intentionally close to
/// the operations the original toolchain exposes, so behaviour is preserved.
pub trait Backend {
    /// Return the connected device identity (may fail when not present).
    fn identify(&mut self) -> Result<DeviceId>;

    /// Erase the program (command) EEPROM.
    fn erase(&mut self) -> Result<()>;

    /// Write program memory words (16-bit). Full image expected.
    fn write_program(&mut self, words: &[u16]) -> Result<()>;

    /// Write data EEPROM bytes.
    fn write_data(&mut self, bytes: &[u8; DATA_EEPROM_BYTES]) -> Result<()>;

    /// Read the whole program memory back.
    fn read_program(&mut self, out: &mut [u16]) -> Result<()>;

    /// Read the data EEPROM back.
    fn read_data(&mut self) -> Result<[u8; DATA_EEPROM_BYTES]>;

    /// Program + verify in one call, mirroring the historical workflow.
    fn program(&mut self, image: &MemoryImage) -> Result<bool>;

    /// Write the OSC/config byte (channel `0x01`). Default: no-op for
    /// backends without a config channel.
    fn write_config(&mut self, _value: u8) -> Result<()> {
        Ok(())
    }

    /// Read the OSC/config byte back. Returns `None` when the backend has no
    /// config channel (default).
    fn read_config(&mut self) -> Result<Option<u8>> {
        Ok(None)
    }
}
