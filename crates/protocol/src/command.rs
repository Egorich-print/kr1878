//! Command-phase helpers: the erase/command pulse and the idle/off sequence.

use crate::error::Result;
use crate::transport::{Transport, CONTROL_CMD, CONTROL_IDLE};

/// Data-register value that begins a command/erase phase.
pub const DATA_ERASE_START: u8 = 0x38;
/// Data-register value that ends the pulse.
pub const DATA_ERASE_END: u8 = 0x3F;

/// Drive the command/erase pulse: assert the start data pattern and toggle the
/// command line (C3) `toggles` times, then release to the end pattern.
///
/// Mirrors `docs/HARDWARE_PROTOCOL.md` §2.6.
pub fn command_pulse(t: &mut dyn Transport, toggles: u32) -> Result<()> {
    t.write_control(CONTROL_IDLE)?;
    t.write_data(DATA_ERASE_START)?;
    for _ in 0..toggles {
        t.write_control(CONTROL_IDLE & !CONTROL_CMD)?;
        t.write_control(CONTROL_IDLE | CONTROL_CMD)?;
    }
    t.write_data(DATA_ERASE_END)?;
    Ok(())
}

/// Default number of C3 toggles used by the original erase pulse.
pub const DEFAULT_ERASE_TOGGLES: u32 = 20;

/// Return the wire to the idle (off) state: control zeroed, data zeroed.
pub fn idle_off(t: &mut dyn Transport) -> Result<()> {
    t.write_control(0)?;
    t.write_data(0)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::RecordingTransport;

    #[test]
    fn pulse_toggles_command_line() {
        let mut t = RecordingTransport::new();
        command_pulse(&mut t, 3).unwrap();
        // 1 idle control write + 2 control writes per toggle.
        assert_eq!(t.control_events.len(), 1 + 2 * 3);
        assert_eq!(t.data_events.first(), Some(&DATA_ERASE_START));
        assert_eq!(t.data_events.last(), Some(&DATA_ERASE_END));
    }

    #[test]
    fn idle_off_zeroes_registers() {
        let mut t = RecordingTransport::new();
        idle_off(&mut t).unwrap();
        assert_eq!(*t.control_events.last().unwrap(), 0);
        assert_eq!(*t.data_events.last().unwrap(), 0);
    }
}