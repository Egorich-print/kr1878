//! Bit-level wire primitives: `write_bits` / `read_bits`.
//!
//! Waveforms follow `docs/HARDWARE_PROTOCOL.md` §2.4–§2.5, with the pin
//! mapping from §1 (write clock = C1, read clock = C2).

use crate::error::{Error, Result};
use crate::transport::{CONTROL_DATA, CONTROL_IDLE, CONTROL_RCLK, CONTROL_WCLK, Transport};

/// How many handshake attempts are made before giving up on ACK.
const ACK_ATTEMPTS: u32 = 10_000;

/// Timing parameters (nanoseconds) for the bit-bang.
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    /// Settle time after placing data on C0 before clocking.
    pub data_settle_ns: u32,
    /// Half-period of the clock lines.
    pub clock_half_ns: u32,
}

impl Default for Timing {
    fn default() -> Self {
        // Modest conservative values; the original used loop-based delays.
        Timing {
            data_settle_ns: 1,
            clock_half_ns: 1,
        }
    }
}

/// Wait until ACK equals `expected`, polling up to `ACK_ATTEMPTS` times.
pub fn wait_ack(t: &mut dyn Transport, expected: bool) -> Result<()> {
    for _ in 0..ACK_ATTEMPTS {
        if ack_line(t)? == expected {
            return Ok(());
        }
    }
    Err(Error::NotReady)
}

/// Current ACK level with a single status read.
fn ack_line(t: &mut dyn Transport) -> Result<bool> {
    Ok(t.read_status()? & crate::transport::STATUS_ACK != 0)
}

/// Transmit `n` bits of `value`, most-significant bit first.
///
/// Each bit: lower the write clock, place the data level on C0, then raise
/// the write clock (C1) to latch it. The read clock (C2) is never toggled
/// here, keeping the write clock as the only edge source. The line returns to
/// idle afterwards.
pub fn write_bits(t: &mut dyn Transport, n: u8, value: u16, timing: &Timing) -> Result<()> {
    if n == 0 || n > 16 {
        return Err(Error::Library(format!("invalid bit count {n}")));
    }
    wait_ack(t, true)?;
    for i in (0..n).rev() {
        let bit = ((value >> i) & 1) as u8;
        // Lower the write clock first; the loop's first control write drives
        // the whole control state. No leading idle write is emitted: from an
        // all-off (idle_off) state a leading IDLE would synthesize a WCLK edge.
        let mut out = CONTROL_IDLE & !CONTROL_WCLK;
        // Place the data level on C0 while the clock is low.
        out &= !CONTROL_DATA;
        out |= if bit != 0 { CONTROL_DATA } else { 0 };
        t.write_control(out)?;
        t.delay(Duration::from_nanos(timing.data_settle_ns.into()))?;
        // Rising edge latches the C0 level.
        out |= CONTROL_WCLK;
        t.write_control(out)?;
        t.delay(Duration::from_nanos(timing.clock_half_ns.into()))?;
    }
    t.write_control(CONTROL_IDLE)?;
    Ok(())
}

/// Receive `n` bits into a `u16`, most-significant bit first.
///
/// Each bit: pulse the read clock (C2), then sample BUSY. The write clock is
/// never toggled here, so sampling drives the only read-clock edge source. The
/// line returns to idle afterwards.
pub fn read_bits(t: &mut dyn Transport, n: u8, timing: &Timing) -> Result<u16> {
    if n == 0 || n > 16 {
        return Err(Error::Library(format!("invalid bit count {n}")));
    }
    let mut value: u16 = 0;
    for i in (0..n).rev() {
        // Lower the read clock, then raise it to clock out the next bit.
        let out = CONTROL_IDLE & !CONTROL_RCLK;
        t.write_control(out)?;
        t.delay(Duration::from_nanos(timing.clock_half_ns.into()))?;
        t.write_control(out | CONTROL_RCLK)?;
        t.delay(Duration::from_nanos(timing.clock_half_ns.into()))?;
        let bit = busy_line(t)?;
        value |= (bit as u16) << i;
    }
    t.write_control(CONTROL_IDLE)?;
    Ok(value)
}

/// Current BUSY level with a single status read.
fn busy_line(t: &mut dyn Transport) -> Result<bool> {
    Ok(t.read_status()? & crate::transport::STATUS_BUSY != 0)
}

use std::time::Duration;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::RecordingTransport;

    #[test]
    fn write_bits_emits_msb_first() {
        let mut t = RecordingTransport::new();
        write_bits(&mut t, 8, 0b1010_0000, &Timing::default()).unwrap();
        // Rising edges of C1 clock the C0 level; collect the C0 value just
        // before each C1 edge.
        let data_bits: Vec<u8> = t.c0_before_wclk_edges();
        assert_eq!(data_bits, vec![1, 0, 1, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn write_bits_rejects_bad_width() {
        let mut t = RecordingTransport::new();
        assert!(write_bits(&mut t, 0, 0, &Timing::default()).is_err());
        assert!(write_bits(&mut t, 17, 0, &Timing::default()).is_err());
    }

    #[test]
    fn read_bits_decodes_busy_stream() {
        let mut t = RecordingTransport::new();
        t.script_busy(&[true, false, true, false, false, false, false, false]);
        t.always_ack(true);
        let v = read_bits(&mut t, 8, &Timing::default()).unwrap();
        assert_eq!(v, 0b1010_0000);
    }

    #[test]
    fn wait_ack_times_out() {
        let mut t = RecordingTransport::new();
        t.always_ack(false);
        assert!(matches!(wait_ack(&mut t, true), Err(Error::NotReady)));
    }

    #[test]
    fn wait_ack_returns_on_match() {
        let mut t = RecordingTransport::new();
        t.always_ack(true);
        assert!(wait_ack(&mut t, true).is_ok());
    }
}
