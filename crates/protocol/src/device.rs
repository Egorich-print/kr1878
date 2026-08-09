//! Device-level programming state machine.
//!
//! Orchestrates the original toolchain workflow against a [`Transport`]:
//!
//! ```text
//!   idle -> ERASE (command pulse) -> CMD program -> write words
//!       -> verify words -> CMD data -> write bytes -> verify bytes
//! ```
//!
//! See `docs/HARDWARE_PROTOCOL.md` §3. This lives at the protocol layer, so it
//! is fully platform-independent and testable. The `programmer` crate only adds
//! concrete [`Transport`] implementations.

use crate::bits::{read_bits, wait_ack, write_bits, Timing};
use crate::command::{command_pulse, idle_off, DEFAULT_ERASE_TOGGLES};
use crate::error::Result;
use crate::transport::Transport;

/// Command code selecting the program (command) EEPROM for a write phase.
pub const CMD_PROGRAM: u8 = 0x03;
/// Command code selecting the data EEPROM for a write phase.
pub const CMD_DATA: u8 = 0x02;

/// Result of a full programming session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionResult {
    /// Whether both program and data memories verified after write.
    pub verified: bool,
    /// Words read back from program memory.
    pub program_back: Vec<u16>,
    /// Bytes read back from data EEPROM.
    pub data_back: Vec<u8>,
}

/// A driver for the KR1878VE1 programming wire protocol over a [`Transport`].
pub struct Device<'a> {
    pub transport: &'a mut dyn Transport,
    pub timing: Timing,
    //ops_toggles: u32,
}

impl<'a> Device<'a> {
    /// Create a driver for `transport` with default timing.
    pub fn new(transport: &'a mut dyn Transport) -> Self {
        Device { transport, timing: Timing::default() }
    }

    /// Create a driver for `transport` with explicit timing.
    pub fn with_timing(transport: &'a mut dyn Transport, timing: Timing) -> Self {
        Device { transport, timing }
    }

    /// Select which EEPROM memory subsequent read/write ops address.
    fn select(&mut self, code: u8) -> Result<()> {
        wait_ack(self.transport, true)?;
        self.transport.write_data(code)
    }

    /// Erase the currently-selected memory with the erase pulse.
    pub fn erase(&mut self) -> Result<()> {
        command_pulse(self.transport, DEFAULT_ERASE_TOGGLES)
    }

    /// Write `n` 16-bit words to program memory; `n == 0` does nothing.
    pub fn write_words(&mut self, words: &[u16]) -> Result<()> {
        if words.is_empty() {
            return Ok(());
        }
        self.select(CMD_PROGRAM)?;
        for &w in words {
            wait_ack(self.transport, true)?;
            write_bits(self.transport, 16, w, &self.timing)?;
        }
        wait_ack(self.transport, true)?;
        Ok(())
    }

    /// Read `n` 16-bit words back from program memory.
    pub fn read_words(&mut self, count: usize) -> Result<Vec<u16>> {
        self.select(CMD_PROGRAM)?;
        let mut out = Vec::with_capacity(count);
        for _ in 0..count {
            out.push(read_bits(self.transport, 16, &self.timing)?);
        }
        Ok(out)
    }

    /// Write 64 data EEPROM bytes.
    pub fn write_bytes(&mut self, bytes: &[u8]) -> Result<()> {
        if bytes.is_empty() {
            return Ok(());
        }
        self.select(CMD_DATA)?;
        for &b in bytes {
            wait_ack(self.transport, true)?;
            let w = u16::from(b);
            write_bits(self.transport, 8, w, &self.timing)?;
        }
        wait_ack(self.transport, true)?;
        Ok(())
    }

    /// Read `count` data EEPROM bytes.
    pub fn read_bytes(&mut self, count: usize) -> Result<Vec<u8>> {
        self.select(CMD_DATA)?;
        let mut out = Vec::with_capacity(count);
        for _ in 0..count {
            let b = (read_bits(self.transport, 8, &self.timing)? & 0xFF) as u8;
            out.push(b);
        }
        Ok(out)
    }

    /// Run the complete program session: erase, write program, verify program,
    /// write data, verify data. Returns the read-back contents for review.
    pub fn run(
        &mut self,
        program: &[u16],
        data: &[u8],
        expected_config: Option<u8>,
    ) -> Result<SessionResult> {
        let _ = expected_config;
        if self.verify_image_equals(program, data)? {
            // Already in the desired state; skip to avoid an unnecessary erase.
            return Ok(SessionResult { verified: true, program_back: program.to_vec(), data_back: data.to_vec() });
        }

        idle_off(self.transport)?;
        self.erase()?;

        self.write_words(program)?;
        let program_back = self.read_words(program.len())?;
        let program_ok = program_back == program;

        self.write_bytes(data)?;
        let data_back = self.read_bytes(data.len())?;
        let data_ok = data_back == data;

        idle_off(self.transport)?;
        Ok(SessionResult {
            verified: program_ok && data_ok,
            program_back,
            data_back,
        })
    }

    /// Cheap guard: are the memories already equal to `program`/`data`?
    fn verify_image_equals(&mut self, program: &[u16], data: &[u8]) -> Result<bool> {
        let p = self.read_words(program.len())?;
        let d = self.read_bytes(data.len())?;
        Ok(p == program && d == data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::SimulatorTransport;

    #[test]
    fn write_then_read_roundtrips() {
        let mut sim = SimulatorTransport::default();
        let mut dev = Device::new(&mut sim);
        let words = [0x1234u16, 0xABCD, 0x00FF];
        dev.write_words(&words).unwrap();
        let back = dev.read_words(words.len()).unwrap();
        assert_eq!(back, words);
    }

    #[test]
    fn run_programs_and_verifies() {
        let mut sim = SimulatorTransport::default();
        let mut dev = Device::new(&mut sim);
        let program = vec![0x0000u16, 0xFFFF, 0x1234];
        let data = vec![0xAB; 64];
        let res = dev.run(&program, &data, None).unwrap();
        assert!(res.verified);
        assert_eq!(res.program_back, program);
        assert_eq!(res.data_back, data);
    }

    #[test]
    fn run_skips_when_already_equal() {
        let mut sim = SimulatorTransport::default();
        let program = vec![0x5555u16];
        let data = vec![0x11; 64];
        {
            let mut dev = Device::new(&mut sim);
            dev.write_words(&program).unwrap();
            dev.write_bytes(&data).unwrap();
        }
        let mut dev = Device::new(&mut sim);
        let res = dev.run(&program, &data, None).unwrap();
        assert!(res.verified);
    }

    #[test]
    fn run_programs_words_and_bytes_in_separate_memories() {
        let mut sim = SimulatorTransport::default();
        let mut dev = Device::new(&mut sim);
        let words = vec![0x1111u16, 0x2222];
        let data = vec![0x77; 64];
        dev.write_words(&words).unwrap();
        dev.write_bytes(&data).unwrap();
        // Re-select program and confirm the byte write did not clobber words.
        let back = dev.read_words(words.len()).unwrap();
        assert_eq!(back, words);
    }
}
