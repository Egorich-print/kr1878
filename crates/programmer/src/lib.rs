//! kr1878-programmer — реализация `ProgrammerBackend` (LPT / USB bridge / mock / wire).
//!
//! Слой *Hardware* (см. ARCHITECTURE.md): конкретные реализации транспорта и
//! адаптеры к абстракции `kr1878_core::backend::Backend`. Бизнес-логика в
//! `core::ops` остаётся платформо-независимой.

pub mod lpt;
pub mod mock;
pub mod usb;
pub mod wire;

#[cfg(feature = "sim")]
pub mod sim;

pub use lpt::{DEFAULT_LPT_BASE, LptBackend, LptTransport};
pub use mock::MockBackend;
pub use usb::{GeneratorMode, UsbBridge, UsbBridgeBackend};
pub use wire::WireBackend;

#[cfg(feature = "sim")]
pub use sim::SimBackend;

/// Convenience re-export: a [`Backend`] fully in memory.
pub use kr1878_core::backend::Backend;
