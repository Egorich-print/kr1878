//! Test utilities: a passive recording transport, an inverted-wire wrapper and
//! a full device simulator.
//!
//! The module is compiled for the crate's own tests always, and for downstream
//! crates behind the `testutil` feature (see `Cargo.toml`). Enabling the
//! feature exposes the simulator as a library API so integration tests can
//! drive the real protocol path instead of mocking at the `Backend` layer.

use std::time::Duration;

use crate::device::{CMD_CONFIG, CMD_DATA, CMD_PROGRAM};
use crate::error::Result;
use crate::transport::{
    CONTROL_DATA, CONTROL_IDLE, CONTROL_WCLK, STATUS_ACK, STATUS_BUSY, Transport,
};

/// A transport that records every control/data write and can script status
/// reads. Used to assert exact waveforms in the bit tests.
pub struct RecordingTransport {
    /// Every control byte written, in order.
    pub control_events: Vec<u8>,
    /// Every data byte written, in order.
    pub data_events: Vec<u8>,
    ack: bool,
    busy_script: Vec<bool>,
    busy_pos: usize,
    prev_control: u8,
    c0_before_wclk: Vec<u8>,
}

impl RecordingTransport {
    pub fn new() -> Self {
        RecordingTransport {
            control_events: Vec::new(),
            data_events: Vec::new(),
            ack: true,
            busy_script: Vec::new(),
            busy_pos: 0,
            prev_control: CONTROL_IDLE,
            c0_before_wclk: Vec::new(),
        }
    }

    /// Force the ACK line to a constant level.
    pub fn always_ack(&mut self, ack: bool) {
        self.ack = ack;
    }

    /// Script the BUSY line for a read sequence; each read_status consumes one.
    pub fn script_busy(&mut self, bits: &[bool]) {
        self.busy_script = bits.to_vec();
        self.busy_pos = 0;
    }

    /// The C0 data level at the moment of each rising write-clock (C1) edge.
    pub fn c0_before_wclk_edges(&self) -> Vec<u8> {
        self.c0_before_wclk.clone()
    }
}

impl Default for RecordingTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl Transport for RecordingTransport {
    fn write_control(&mut self, byte: u8) -> Result<()> {
        let prev = self.prev_control;
        let wclk_rose = prev & CONTROL_WCLK == 0 && byte & CONTROL_WCLK != 0;
        self.control_events.push(byte);
        if wclk_rose {
            self.c0_before_wclk.push(byte & CONTROL_DATA);
        }
        self.prev_control = byte;
        Ok(())
    }
    fn write_data(&mut self, byte: u8) -> Result<()> {
        self.data_events.push(byte);
        Ok(())
    }
    fn read_status(&mut self) -> Result<u8> {
        let mut s = 0u8;
        if self.ack {
            s |= STATUS_ACK;
        }
        if self.busy_pos < self.busy_script.len() {
            if self.busy_script[self.busy_pos] {
                s |= STATUS_BUSY;
            }
            self.busy_pos += 1;
        }
        Ok(s)
    }
}

/// A transport adapter that inverts the raw wire polarity of the BUSY (bit7)
/// and ACK (bit6) status lines, exactly as the parallel-port hardware does
/// (see `docs/HARDWARE_PROTOCOL.md` §1). It re-presents the *normalised*
/// active-high status to the protocol, so the bit-bang driver stays polarity-
/// agnostic, while the underlying transport observes the inverted levels.
///
/// Pair this with [`RecordingTransport`] or a real LPT transport to verify
/// that the decoder's un-inverting matches the protocol's active-high levels.
pub struct InvertedStatusTransport<T: Transport> {
    inner: T,
    invert: bool,
}

impl<T: Transport> InvertedStatusTransport<T> {
    /// Wrap `inner` with the polarity inversion enabled.
    pub fn inverted(inner: T) -> Self {
        InvertedStatusTransport {
            inner,
            invert: true,
        }
    }

    /// Wrap `inner` without polarity inversion (pass-through).
    pub fn passthrough(inner: T) -> Self {
        InvertedStatusTransport {
            inner,
            invert: false,
        }
    }

    /// The raw status as the wire would deliver it (inversion applied).
    pub fn raw(&mut self) -> Result<u8> {
        let s = self.inner.read_status()?;
        Ok(if self.invert {
            s ^ (STATUS_BUSY | STATUS_ACK)
        } else {
            s
        })
    }

    /// The normalised status the transport presents to the protocol.
    pub fn normalized(&mut self) -> Result<u8> {
        let s = self.raw()?;
        Ok(if self.invert {
            s ^ (STATUS_BUSY | STATUS_ACK)
        } else {
            s
        })
    }
}

impl<T: Transport> Transport for InvertedStatusTransport<T> {
    fn write_control(&mut self, byte: u8) -> Result<()> {
        self.inner.write_control(byte)
    }

    fn write_data(&mut self, byte: u8) -> Result<()> {
        self.inner.write_data(byte)
    }

    fn read_status(&mut self) -> Result<u8> {
        self.normalized()
    }

    fn delay(&mut self, duration: Duration) -> Result<()> {
        self.inner.delay(duration)
    }
}

/// A runnable simulator of the wire protocol.
///
/// Bits written on the write clock are stored per-memory; when the driver
/// later pulses the read clock and samples BUSY, it replays those same bits.
/// Writes and reads therefore agree, so program+verify round-trips succeed.
pub struct SimulatorTransport {
    program_bits: Vec<bool>,
    data_bits: Vec<bool>,
    config_bits: Vec<bool>,
    last_code: u8,
    read_ptr: usize,
    read_level: bool,
    prev_control: u8,
}

impl SimulatorTransport {
    pub fn program_bits(&self) -> &[bool] {
        &self.program_bits
    }
    pub fn data_bits(&self) -> &[bool] {
        &self.data_bits
    }
    pub fn config_bits(&self) -> &[bool] {
        &self.config_bits
    }
}

impl Default for SimulatorTransport {
    fn default() -> Self {
        SimulatorTransport {
            program_bits: Vec::new(),
            data_bits: Vec::new(),
            config_bits: Vec::new(),
            last_code: 0,
            read_ptr: 0,
            read_level: false,
            prev_control: CONTROL_IDLE,
        }
    }
}

impl Transport for SimulatorTransport {
    fn write_data(&mut self, byte: u8) -> Result<()> {
        self.last_code = byte;
        // Selecting a memory resets its internal read counter.
        self.read_ptr = 0;
        Ok(())
    }

    fn write_control(&mut self, byte: u8) -> Result<()> {
        let prev = self.prev_control;
        // Write clock rising edge: capture the data line (C0) into memory,
        // but only while a write command address is selected. Erase/command
        // pulses (unselected states) therefore do not latch.
        if prev & CONTROL_WCLK == 0 && byte & CONTROL_WCLK != 0 {
            let bit = byte & CONTROL_DATA != 0;
            match self.last_code {
                CMD_DATA => self.data_bits.push(bit),
                CMD_PROGRAM => self.program_bits.push(bit),
                CMD_CONFIG => self.config_bits.push(bit),
                _ => {}
            }
        }
        // Read clock rising edge: present the next stored bit on BUSY.
        let rclk_rose = prev & crate::transport::CONTROL_RCLK == 0
            && byte & crate::transport::CONTROL_RCLK != 0;
        if rclk_rose {
            let store = self.store();
            if self.read_ptr < store.len() {
                self.read_level = store[self.read_ptr];
            } else {
                self.read_level = false;
            }
            self.read_ptr += 1;
        }
        self.prev_control = byte;
        Ok(())
    }

    fn read_status(&mut self) -> Result<u8> {
        let mut s = STATUS_ACK;
        if self.read_level {
            s |= STATUS_BUSY;
        }
        Ok(s)
    }
}

impl SimulatorTransport {
    fn store(&self) -> &[bool] {
        match self.last_code {
            CMD_DATA => &self.data_bits,
            CMD_CONFIG => &self.config_bits,
            _ => &self.program_bits,
        }
    }
}
