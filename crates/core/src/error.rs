//! Error handling for the core crate.

use thiserror::Error;

/// All errors produced by the core crate.
#[derive(Debug, Error)]
pub enum Error {
    #[error("invalid opcode word 0x{0:04X}")]
    InvalidOpcode(u16),

    #[error("program address out of range: 0x{addr:03X}")]
    ProgramAddressOutOfRange { addr: u32 },

    #[error("literal out of range: got {got}, expected {lo}..={hi}")]
    LiteralOutOfRange { got: i64, lo: i64, hi: i64 },

    #[error("{0}")]
    Other(String),
}

/// Convenience alias.
pub type Result<T, E = Error> = std::result::Result<T, E>;
