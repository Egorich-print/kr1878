//! Transport abstraction: the few platform-level operations the protocol
//! needs from the physical programmer. Any real transport (LPT, USB bridge,
//! a simulated chip) supplies these; everything else in the protocol crate is
//! deterministic and platform-agnostic.
//!
//! The line mapping follows `docs/HARDWARE_PROTOCOL.md` §1:
//!
//! | Control bit | Signal    | Use                  |
//! |-------------|-----------|----------------------|
//! | C0 (0x01)   | STROBE/#  | data bit out         |
//! | C1 (0x02)   | AutoFeed  | write clock          |
//! | C2 (0x04)   | INIT      | read clock           |
//! | C3 (0x08)   | SelectIn  | command / erase pulse|
//!
//! The status register exposes ACK on bit 6 and BUSY on bit 7, *active-high as
//! presented by the transport*. The platform-specific LPT transport is
//! responsible for normalising the hardware's inverted levels into this
//! convention; the bit-bang logic below stays inversion-agnostic.

/// Control register line bit – data output (C0).
pub const CONTROL_DATA: u8 = 0x01;
/// Control register line bit – write clock (C1).
pub const CONTROL_WCLK: u8 = 0x02;
/// Control register line bit – read clock (C2).
pub const CONTROL_RCLK: u8 = 0x04;
/// Control register line bit – command/erase pulse (C3).
pub const CONTROL_CMD: u8 = 0x08;

/// The control register is driven to all-high between words/bits (idle).
pub const CONTROL_IDLE: u8 = 0x0F;

/// Status register: ACK line bit (active high).
pub const STATUS_ACK: u8 = 0x40;
/// Status register: BUSY line bit (active high).
pub const STATUS_BUSY: u8 = 0x80;

/// A platform I/O abstraction over the three programmer registers.
///
/// Each method maps to a single physical write or read; the protocol builds
/// its waveforms from these three primitives plus [`Self::delay`].
pub trait Transport {
    /// Write the control register byte.
    fn write_control(&mut self, byte: u8) -> Result<()>;
    /// Write the data (command) register byte.
    fn write_data(&mut self, byte: u8) -> Result<()>;
    /// Read the status register byte.
    fn read_status(&mut self) -> Result<u8>;

    /// Block for a short, configurable delay. Real transports wait wall-clock
    /// time; tests insert a no-op. The default does nothing.
    fn delay(&mut self, _duration: Duration) -> Result<()> {
        Ok(())
    }
}

use std::time::Duration;

use crate::error::Result;

/// Read the ACK line as a boolean.
pub fn ack(t: &mut dyn Transport) -> Result<bool> {
    Ok(t.read_status()? & STATUS_ACK != 0)
}

/// Read the BUSY line as a boolean.
pub fn busy(t: &mut dyn Transport) -> Result<bool> {
    Ok(t.read_status()? & STATUS_BUSY != 0)
}