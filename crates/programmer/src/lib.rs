//! kr1878-programmer — реализация `ProgrammerBackend` (LPT / mock / wire).
//!
//! Слой *Hardware* (см. ARCHITECTURE.md): конкретные реализации транспорта и
//! адаптеры к абстракции `kr1878_core::backend::Backend`. Бизнес-логика в
//! `core::ops` остаётся платформо-независимой.

pub mod lpt;
pub mod mock;
pub mod wire;

pub use lpt::{DEFAULT_LPT_BASE, LptBackend, LptTransport};
pub use mock::MockBackend;
pub use wire::WireBackend;

/// Convenience re-export: a [`Backend`] fully in memory.
pub use kr1878_core::backend::Backend;
