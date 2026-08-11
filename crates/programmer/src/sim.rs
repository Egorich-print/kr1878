//! Fully simulated device backend. No hardware attached: the real protocol
//! path (`WireBackend` -> `protocol::Device` -> `SimulatorTransport`) is run
//! against an in-memory chip model, so the exact waveforms the LPT bit-bang
//! would produce are exercised end-to-end. Used by the CLI (`--backend sim`),
//! CI and demos where a physical programmer is unavailable.
//!
//! Contrast with [`crate::mock::MockBackend`]: that one stores a whole
//! `MemoryImage` and skips the protocol layer entirely; this one goes through
//! every bit cell, clock edge and read-back.
//!
//! An optional `.sim` state file makes the chip durable across separate CLI
//! invocations: every mutating operation persists the full state.

use std::path::Path;

use kr1878_core::backend::{Backend, DeviceId};
use kr1878_core::error::Result;
use kr1878_core::memory::{DATA_EEPROM_BYTES, MemoryImage};
use kr1878_file_format::sim as simfmt;
use kr1878_protocol::testutil::SimulatorTransport;

use crate::wire::WireBackend;

/// A [`Backend`] backed by the wire-protocol simulator.
pub struct SimBackend {
    transport: SimulatorTransport,
    state_file: Option<std::path::PathBuf>,
}

impl Default for SimBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl SimBackend {
    /// Create a simulator with an empty (erased) chip.
    pub fn new() -> Self {
        SimBackend {
            transport: SimulatorTransport::default(),
            state_file: None,
        }
    }

    /// Load a chip from a `.sim` snapshot; a missing file yields an erased
    /// chip. The chip is bound to `path` and re-persisted on every mutation.
    pub fn load_or_new(path: &Path) -> Result<Self> {
        let transport = read_snapshot(path)?;
        Ok(SimBackend {
            transport,
            state_file: Some(path.to_path_buf()),
        })
    }

    /// Inspect the raw bit-level state the simulator recorded. Primarily for
    /// tests/debugging: words are bubbled back little-endian like the wire.
    pub fn program_bits(&self) -> &[bool] {
        self.transport.program_bits()
    }

    /// Dump the current chip state as a memory image (what a `read` sees).
    pub fn read_image(&mut self) -> Result<MemoryImage> {
        let mut img = MemoryImage::new();
        let mut prog = vec![0u16; kr1878_core::memory::PROGRAM_WORDS];
        let mut wire = WireBackend::new(&mut self.transport);
        wire.read_program(&mut prog)?;
        img.program = prog;
        img.data = wire.read_data()?;
        img.config = wire.read_config()?;
        Ok(img)
    }

    fn persist(&mut self) -> Result<()> {
        let Some(path) = self.state_file.clone() else {
            return Ok(());
        };
        let img = self.read_image()?;
        simfmt::write_sim_file(&path, &img)
            .map_err(|e| kr1878_core::error::Error::Other(e.to_string()))
    }
}

/// Read a `.sim` snapshot into a transport, replaying the words through the
/// real write path; a missing file yields an erased (empty) transport.
fn read_snapshot(path: &Path) -> Result<SimulatorTransport> {
    if !path.exists() {
        return Ok(SimulatorTransport::default());
    }
    let bytes = std::fs::read(path).map_err(|e| kr1878_core::error::Error::Other(e.to_string()))?;
    let img = simfmt::read_sim(&mut &bytes[..])
        .map_err(|e| kr1878_core::error::Error::Other(e.to_string()))?;
    let mut backend = SimBackend::new();
    backend.write_program(&img.program)?;
    backend.write_data(&img.data)?;
    if let Some(cfg) = img.config {
        backend.write_config(cfg)?;
    }
    Ok(backend.transport)
}

impl Backend for SimBackend {
    fn identify(&mut self) -> Result<DeviceId> {
        WireBackend::new(&mut self.transport).identify()
    }

    fn erase(&mut self) -> Result<()> {
        let r = WireBackend::new(&mut self.transport).erase();
        self.persist()?;
        r
    }

    fn write_program(&mut self, words: &[u16]) -> Result<()> {
        let r = WireBackend::new(&mut self.transport).write_program(words);
        self.persist()?;
        r
    }

    fn write_data(&mut self, bytes: &[u8; DATA_EEPROM_BYTES]) -> Result<()> {
        let r = WireBackend::new(&mut self.transport).write_data(bytes);
        self.persist()?;
        r
    }

    fn read_program(&mut self, out: &mut [u16]) -> Result<()> {
        WireBackend::new(&mut self.transport).read_program(out)
    }

    fn read_data(&mut self) -> Result<[u8; DATA_EEPROM_BYTES]> {
        WireBackend::new(&mut self.transport).read_data()
    }

    fn program(&mut self, image: &MemoryImage) -> Result<bool> {
        let r = WireBackend::new(&mut self.transport).program(image);
        self.persist()?;
        r
    }

    fn write_config(&mut self, value: u8) -> Result<()> {
        let r = WireBackend::new(&mut self.transport).write_config(value);
        self.persist()?;
        r
    }

    fn read_config(&mut self) -> Result<Option<u8>> {
        WireBackend::new(&mut self.transport).read_config()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(path: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("kr1878_sim_test_{}_{}", std::process::id(), path))
    }

    #[test]
    fn program_roundtrips_through_real_protocol_path() {
        let mut backend = SimBackend::new();
        let mut img = MemoryImage::new();
        img.program[0] = 0x8123;
        img.program[511] = 0xDEAD;
        img.data[0] = 0x00;
        img.data[63] = 0x5A;
        img.config = Some(0x02);

        assert!(backend.program(&img).unwrap());

        // read_image reads back through the wire: identical to what was set.
        let back = backend.read_image().unwrap();
        assert_eq!(back.program, img.program);
        assert_eq!(back.data, img.data);
        assert_eq!(back.config, Some(0x02));
    }

    #[test]
    fn persistence_survives_reload_from_snapshot() {
        let path = tmp("persist.sim");
        let _ = std::fs::remove_file(&path);

        {
            let mut backend = SimBackend::load_or_new(&path).unwrap();
            let img = MemoryImage::from_program(&[0x1234, 0x5678]).unwrap();
            assert!(backend.program(&img).unwrap());
        }

        // A fresh backend bound to the same file must see the same image.
        let mut re = SimBackend::load_or_new(&path).unwrap();
        let back = re.read_image().unwrap();
        assert_eq!(back.program[0], 0x1234);
        assert_eq!(back.program[1], 0x5678);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn missing_snapshot_yields_erased_chip() {
        let path = tmp("missing.sim");
        let _ = std::fs::remove_file(&path);
        let mut backend = SimBackend::load_or_new(&path).unwrap();
        let img = backend.read_image().unwrap();
        // A never-written chip reads all-zero bits through the wire path
        // (unlike MockBackend, which defaults to 0xFFFF at the memory layer).
        assert_eq!(img.program[0], 0x0000);
        assert_eq!(img.config, Some(0x00));
        let _ = std::fs::remove_file(&path);
    }
}
