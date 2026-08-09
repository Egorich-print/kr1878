//! LPT (parallel-port) transport and backend.
//!
//! The original programmer bit-bangs the KR1878VE1 protocol over a parallel
//! port: data/command byte on `0x378`, status on `0x379`, control strobes on
//! `0x37A`. This module implements [`Transport`] over those registers,
//! preserving the wire polarity of the original (see `docs/HARDWARE_PROTOCOL.md`
//! §1–§2.1):
//!
//! * control register is written verbatim (`out 0x37A, byte`);
//! * on the status register, bit7 (BUSY) and bit6 (ACK) are inverted on the
//!   wire and the decoder un-inverts them, matching the protocol's active-high
//!   levels used by the bit-bang driver.
//!
//! Raw port I/O is only available where the OS permits it (Linux/FreeBSD on
//! x86-64 via `ioperm`+`inb`/`outb`). Elsewhere all I/O returns a clear
//! `backend unavailable` error so the crate stays cross-platform.

use std::time::Duration;

use kr1878_core::backend::Backend;
use kr1878_core::memory::MemoryImage;
use kr1878_protocol::error::{Error, Result};
use kr1878_protocol::transport::Transport;

use crate::wire::WireBackend;

/// Parallel-port base address used by the original (0x378 is LPT1).
pub const DEFAULT_LPT_BASE: u16 = 0x378;
/// Status-register offset relative to the base.
const STATUS: u16 = 0x0001;
/// Control-register offset relative to the base.
const CONTROL: u16 = 0x0002;

/// A transport over a parallel port. Construction never touches hardware; the
/// port is opened lazily on the first I/O.
pub struct LptTransport {
    base: u16,
}

impl LptTransport {
    /// Create a transport for the parallel port at `base`.
    pub fn new(base: u16) -> Self {
        LptTransport { base }
    }

    fn out(&self, off: u16, val: u8) -> Result<()> {
        let addr = self
            .base
            .checked_add(off)
            .ok_or_else(|| Error::Transport("LPT address overflow".into()))?;
        write_port(addr, val)
    }

    fn in_status(&self) -> Result<u8> {
        let addr = self
            .base
            .checked_add(STATUS)
            .ok_or_else(|| Error::Transport("LPT address overflow".into()))?;
        read_port(addr)
    }
}

impl Default for LptTransport {
    fn default() -> Self {
        LptTransport::new(DEFAULT_LPT_BASE)
    }
}

/// A ready-to-use [`Backend`] over the parallel port. Owns its transport, so it
/// can be boxed as `dyn Backend` (used by the CLI).
pub struct LptBackend {
    transport: LptTransport,
}

impl LptBackend {
    /// Open the default LPT1 port.
    pub fn new() -> Self {
        LptBackend {
            transport: LptTransport::default(),
        }
    }

    /// Open a specific parallel-port base address.
    pub fn at(base: u16) -> Self {
        LptBackend {
            transport: LptTransport::new(base),
        }
    }
}

impl Default for LptBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl Backend for LptBackend {
    fn identify(&mut self) -> kr1878_core::error::Result<kr1878_core::backend::DeviceId> {
        WireBackend::new(&mut self.transport).identify()
    }

    fn erase(&mut self) -> kr1878_core::error::Result<()> {
        WireBackend::new(&mut self.transport).erase()
    }

    fn write_program(&mut self, words: &[u16]) -> kr1878_core::error::Result<()> {
        WireBackend::new(&mut self.transport).write_program(words)
    }

    fn write_data(&mut self, bytes: &[u8; 64]) -> kr1878_core::error::Result<()> {
        WireBackend::new(&mut self.transport).write_data(bytes)
    }

    fn read_program(&mut self, out: &mut [u16]) -> kr1878_core::error::Result<()> {
        WireBackend::new(&mut self.transport).read_program(out)
    }

    fn read_data(&mut self) -> kr1878_core::error::Result<[u8; 64]> {
        WireBackend::new(&mut self.transport).read_data()
    }

    fn program(&mut self, image: &MemoryImage) -> kr1878_core::error::Result<bool> {
        WireBackend::new(&mut self.transport).program(image)
    }
}

impl Transport for LptTransport {
    fn write_control(&mut self, byte: u8) -> Result<()> {
        self.out(CONTROL, byte)
    }

    fn write_data(&mut self, byte: u8) -> Result<()> {
        self.out(0, byte)
    }

    fn read_status(&mut self) -> Result<u8> {
        let raw = self.in_status()?;
        // Invert BUSY (bit7) and ACK (bit6) as the original decoder does.
        let busy = (!(raw >> 7)) & 1;
        let ack = (!(raw >> 6)) & 1;
        Ok((busy << 7) | (ack << 6))
    }

    fn delay(&mut self, dur: Duration) -> Result<()> {
        std::thread::sleep(dur);
        Ok(())
    }
}

fn write_port(addr: u16, val: u8) -> Result<()> {
    #[cfg(all(
        any(target_os = "linux", target_os = "freebsd"),
        target_arch = "x86_64"
    ))]
    {
        // SAFETY: grants access to the single port address and writes it.
        unsafe {
            libc::ioperm(addr, 1, 1);
            libc::outb(val, addr);
        }
        return Ok(());
    }
    let _ = (addr, val);
    Err(Error::Transport(
        "LPT backend unavailable on this platform/architecture".into(),
    ))
}

fn read_port(addr: u16) -> Result<u8> {
    #[cfg(all(
        any(target_os = "linux", target_os = "freebsd"),
        target_arch = "x86_64"
    ))]
    {
        // SAFETY: grants access to the single port address and reads it.
        unsafe {
            libc::ioperm(addr, 1, 1);
            return Ok(libc::inb(addr));
        }
    }
    let _ = addr;
    Err(Error::Transport(
        "LPT backend unavailable on this platform/architecture".into(),
    ))
}
