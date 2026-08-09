//! Error type for the low-level programming protocol.

use thiserror::Error;

/// A protocol-level failure. These arise from the wire interaction, not from
/// the rest of the toolchain, so they are kept independent of the core crate.
#[derive(Debug, Error)]
pub enum Error {
    /// The microcontroller did not raise ACK within the retry budget.
    #[error("microcontroller not ready (handshake timeout)")]
    NotReady,

    /// The transport raised an I/O fault while driving a line.
    #[error("transport I/O error: {0}")]
    Transport(String),

    /// A protocol-layout helper failed.
    #[error("protocol error: {0}")]
    Library(String),
}

impl Error {
    /// Convenience constructor wrapping an underlying I/O error string.
    pub fn transport(msg: impl Into<String>) -> Self {
        Error::Transport(msg.into())
    }
}

/// Boolean result alias for protocol helpers.
pub type Result<T> = std::result::Result<T, Error>;
