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
//! Raw port I/O is only available where the OS permits it: Linux on x86-64,
//! where `ioperm(2)` grants access to the port and the transfer itself is the
//! `out dx, al` / `in al, dx` instruction. The `libc` crate exposes `ioperm`
//! but **not** `inb`/`outb` — those are x86 instructions, not libc calls — so
//! the transfer is emitted as inline assembly.
//! Elsewhere all I/O returns a clear `backend unavailable` error so the crate
//! stays cross-platform.

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

    fn write_config(&mut self, value: u8) -> kr1878_core::error::Result<()> {
        WireBackend::new(&mut self.transport).write_config(value)
    }

    fn read_config(&mut self) -> kr1878_core::error::Result<Option<u8>> {
        WireBackend::new(&mut self.transport).read_config()
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

/// Raw port-I/O primitives for Linux/x86-64.
///
/// `libc` provides `ioperm()` on Linux but has no `inb()`/`outb()`: those are
/// x86 instructions (`out dx, al` / `in al, dx`), not libc functions — which
/// is why the crate could never compile with them. Emitting the instructions
/// directly is also what the original bit-bang driver did.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod port_io {
    /// Grant this process access to the single I/O port `addr`.
    ///
    /// Returns `false` when the kernel refuses: without `CAP_SYS_RAWIO` (or
    /// root) the following `out`/`in` would fault the process.
    ///
    /// # Safety
    ///
    /// The caller must only touch ports it is allowed to access.
    pub unsafe fn grant(addr: u16) -> bool {
        // The explicit `unsafe` block is required even inside an `unsafe fn`
        // since edition 2024 (`unsafe_op_in_unsafe_fn`).
        unsafe { libc::ioperm(addr as libc::c_ulong, 1, 1) == 0 }
    }

    /// Write `val` to the I/O port `addr`.
    ///
    /// # Safety
    ///
    /// `grant(addr)` must have succeeded for this address.
    pub unsafe fn out(addr: u16, val: u8) {
        // `out dx, al`: AL -> port addressed by DX. The instruction touches
        // neither the stack nor the flags, hence `nostack`/`preserves_flags`.
        // The explicit `unsafe` block is required even inside an `unsafe fn`
        // since edition 2024 (`unsafe_op_in_unsafe_fn`).
        unsafe {
            core::arch::asm!(
                "out dx, al",
                in("dx") addr,
                in("al") val,
                options(nostack, preserves_flags),
            );
        }
    }

    /// Read a byte from the I/O port `addr`.
    ///
    /// # Safety
    ///
    /// `grant(addr)` must have succeeded for this address.
    pub unsafe fn in_byte(addr: u16) -> u8 {
        let mut val: u8 = 0;
        // `in al, dx`: port addressed by DX -> AL. The explicit `unsafe` block
        // is required even inside an `unsafe fn` since edition 2024
        // (`unsafe_op_in_unsafe_fn`).
        unsafe {
            core::arch::asm!(
                "in al, dx",
                out("al") val,
                in("dx") addr,
                options(nostack, preserves_flags),
            );
        }
        val
    }
}

/// Write `val` to the I/O port `addr`.
///
/// Two mutually exclusive definitions instead of one function with `#[cfg]`
/// blocks inside: the cfg-stripped body then has exactly one tail expression
/// on every platform, so neither `needless_return` nor unreachable-code
/// lints can fire on a target the author does not build on.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn write_port(addr: u16, val: u8) -> Result<()> {
    // SAFETY: `grant` limits the kernel permission to the single port we are
    // about to write, and the caller checked the address for overflow before
    // we got here.
    unsafe {
        if !port_io::grant(addr) {
            return Err(Error::Transport(format!(
                "ioperm(0x{addr:03X}) failed — raw I/O needs CAP_SYS_RAWIO (root)"
            )));
        }
        port_io::out(addr, val);
    }
    Ok(())
}

/// Write `val` to the I/O port `addr` — raw port I/O is unavailable here.
#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
fn write_port(addr: u16, val: u8) -> Result<()> {
    let _ = (addr, val);
    Err(Error::Transport(
        "LPT backend unavailable on this platform/architecture".into(),
    ))
}

/// Read a byte from the I/O port `addr`.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn read_port(addr: u16) -> Result<u8> {
    // SAFETY: `grant` limits the kernel permission to the single port we are
    // about to read, and the caller checked the address for overflow before
    // we got here.
    unsafe {
        if !port_io::grant(addr) {
            return Err(Error::Transport(format!(
                "ioperm(0x{addr:03X}) failed — raw I/O needs CAP_SYS_RAWIO (root)"
            )));
        }
        Ok(port_io::in_byte(addr))
    }
}

/// Read a byte from the I/O port `addr` — raw port I/O is unavailable here.
#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
fn read_port(addr: u16) -> Result<u8> {
    let _ = addr;
    Err(Error::Transport(
        "LPT backend unavailable on this platform/architecture".into(),
    ))
}
