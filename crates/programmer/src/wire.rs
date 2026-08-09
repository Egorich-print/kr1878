//! Adapter that turns the protocol-layer [`Device`] into a core [`Backend`].
//!
//! This wires the hardware-independent protocol driver to the business layer:
//! a caller supplies a concrete [`Transport`] (LPT, mock wire, …) and this
//! [`WireBackend`] exposes the full [`Backend`] surface using that transport.

use kr1878_core::backend::{Backend, DeviceId};
use kr1878_core::error::{Error, Result};
use kr1878_core::memory::{DATA_EEPROM_BYTES, MemoryImage};
use kr1878_protocol::{Device, Timing, Transport};

/// A [`Backend`] implemented over a live protocol [`Device`].
pub struct WireBackend<'a> {
    device: Device<'a>,
}

impl<'a> WireBackend<'a> {
    /// Build a backend driving `transport` with the default timing.
    pub fn new(transport: &'a mut dyn Transport) -> Self {
        WireBackend {
            device: Device::new(transport),
        }
    }

    /// Build a backend driving `transport` with explicit timing.
    pub fn with_timing(transport: &'a mut dyn Transport, timing: Timing) -> Self {
        WireBackend {
            device: Device::with_timing(transport, timing),
        }
    }
}

impl Backend for WireBackend<'_> {
    fn identify(&mut self) -> Result<DeviceId> {
        Ok(DeviceId {
            name: "KR1878VE1".into(),
            revision: None,
        })
    }

    fn erase(&mut self) -> Result<()> {
        self.device.erase().map_err(|e| Error::Other(e.to_string()))
    }

    fn write_program(&mut self, words: &[u16]) -> Result<()> {
        self.device
            .write_words(words)
            .map_err(|e| Error::Other(e.to_string()))
    }

    fn write_data(&mut self, bytes: &[u8; DATA_EEPROM_BYTES]) -> Result<()> {
        self.device
            .write_bytes(bytes)
            .map_err(|e| Error::Other(e.to_string()))
    }

    fn read_program(&mut self, out: &mut [u16]) -> Result<()> {
        let words = self
            .device
            .read_words(out.len())
            .map_err(|e| Error::Other(e.to_string()))?;
        out.copy_from_slice(&words);
        Ok(())
    }

    fn read_data(&mut self) -> Result<[u8; DATA_EEPROM_BYTES]> {
        let bytes = self
            .device
            .read_bytes(DATA_EEPROM_BYTES)
            .map_err(|e| Error::Other(e.to_string()))?;
        let mut out = [0u8; DATA_EEPROM_BYTES];
        out.copy_from_slice(&bytes);
        Ok(out)
    }

    fn program(&mut self, image: &MemoryImage) -> Result<bool> {
        let res = self
            .device
            .run(&image.program, &image.data, image.config)
            .map_err(|e| Error::Other(e.to_string()))?;
        Ok(res.verified)
    }
}
