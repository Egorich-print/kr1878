//! kr1878-protocol — низкоуровневый протокол программатора КР1878ВЕ1.
//!
//! Слой реализует битбанг-примитивы и драйвер устройства, полностью
//! специфичные для прошивки МК, но не зависящие от платформы. Все I/O
//! инкапсулированы в траит [`transport::Transport`]; реальные реализации
//! (LPT/USB/mock) живут в crate `kr1878-programmer`.
//!
//! Архитектурное место: слой *Protocol* (см. ARCHITECTURE.md). Никакой связи
//! с GUI/CLI, никакой зависимости от ОС.

pub mod bits;
pub mod command;
pub mod crc;
pub mod device;
pub mod error;
pub mod transport;

#[cfg(test)]
pub mod testutil;

pub use bits::Timing;
pub use device::{Device, SessionResult, CMD_DATA, CMD_PROGRAM};
pub use error::{Error, Result};
pub use transport::{Transport, CONTROL_IDLE};