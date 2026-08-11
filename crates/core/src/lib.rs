//! Software model of the KR1878VE1 reproduced from `docs/REVERSE_ENGINEERING.md`.
//!
//! The crate owns the single source of truth for the instruction set,
//! the memory model and the business-level operations (read / write / verify /
//! assemble pipelines). It is platform-agnostic and must not depend on any
//! hardware or OS feature.

pub mod backend;
pub mod cpu;
pub mod crc;
pub mod error;
pub mod isa;
pub mod memory;
pub mod ops;

pub use backend::{Backend, DeviceId};
pub use cpu::Cpu;
pub use error::{Error, Result};
pub use isa::Instruction;
pub use memory::MemoryImage;
