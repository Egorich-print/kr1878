//! Desktop command layer: business operations exposed to the UI.
//!
//! Mirror of the CLI flow (assemble -> program -> verify -> read) but with
//! values passed as structured payloads instead of file paths/argv, so the
//! Svelte frontend can drive the same core without spawning the binary.
//! Pure and unit-tested; the Tauri wrapper is a thin adapter.
//!
//! Unlike the CLI (which rebuilds the backend per invocation and relies on a
//! `.sim` state file for persistence), the app keeps one live backend in the
//! Tauri-managed state so `write -> verify -> read` target the same device.

use serde::{Deserialize, Serialize};

use kr1878_core::backend::Backend;
use kr1878_core::memory::{MemoryImage, DATA_EEPROM_BYTES};
use kr1878_core::ops::{self, WriteOptions};
use kr1878_programmer::{GeneratorMode, LptBackend, MockBackend, SimBackend, UsbBridgeBackend};

/// Backend identifier accepted from the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BackendKind {
    Mock,
    Sim,
    Lpt,
    Usb,
}

impl BackendKind {
    /// All backends the UI may offer.
    pub const ALL: [BackendKind; 4] = [
        BackendKind::Mock,
        BackendKind::Sim,
        BackendKind::Lpt,
        BackendKind::Usb,
    ];

    pub fn label(self) -> &'static str {
        match self {
            BackendKind::Mock => "Mock (in-memory)",
            BackendKind::Sim => "Simulated chip",
            BackendKind::Lpt => "LPT programmer",
            BackendKind::Usb => "USB bridge programmer",
        }
    }

    /// Stable identifier for the UI (sent back by `list_backends`).
    pub fn id(self) -> &'static str {
        match self {
            BackendKind::Mock => "mock",
            BackendKind::Sim => "sim",
            BackendKind::Lpt => "lpt",
            BackendKind::Usb => "usb",
        }
    }
}

fn make_backend(
    kind: BackendKind,
    sim_state: Option<&str>,
    port: Option<&str>,
) -> Result<Box<dyn Backend>, String> {
    Ok(match kind {
        BackendKind::Mock => Box::new(MockBackend::new()),
        BackendKind::Sim => {
            let backend = match sim_state {
                Some(path) if !path.is_empty() => {
                    SimBackend::load_or_new(std::path::Path::new(path))
                        .map_err(|e| format!("load simulated chip: {e}"))?
                }
                _ => SimBackend::new(),
            };
            Box::new(backend)
        }
        BackendKind::Lpt => Box::new(LptBackend::new()),
        BackendKind::Usb => {
            let port = port.ok_or_else(|| "USB bridge requires a serial port".to_string())?;
            // Default generator configuration, matching the official tool.
            Box::new(
                UsbBridgeBackend::open(port, true, GeneratorMode::QuartzHigh)
                    .map_err(|e| e.to_string())?,
            )
        }
    })
}

/// A connected backend plus its human-readable identity for the UI.
pub struct Connection {
    pub backend: Box<dyn Backend>,
    pub label: String,
}

/// Build the live backend the app will operate on for the whole session.
pub fn connect(
    kind: BackendKind,
    sim_state: Option<&str>,
    port: Option<&str>,
) -> Result<Connection, String> {
    let mut backend = make_backend(kind, sim_state, port)?;
    let id = backend.identify().map_err(|e| e.to_string())?;
    let label = format!("{} ({})", id.name, id.revision.as_deref().unwrap_or("n/a"));
    Ok(Connection { backend, label })
}

/// Assembly result handed back to the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssembleResult {
    pub words: Vec<u16>,
    pub data: Vec<u8>,
    pub config: Option<u8>,
    pub listing: String,
    pub symbols: Vec<String>,
}

/// Assemble `.mic` source text into a program image.
pub fn assemble(source: &str) -> Result<AssembleResult, String> {
    let out = kr1878_assembler::assemble(source).map_err(|e| e.to_string())?;
    Ok(AssembleResult {
        words: out.image.program,
        data: out.image.data.to_vec(),
        config: out.image.config,
        listing: out.listing,
        symbols: out.symbols.globals().map(|(n, _)| n.clone()).collect(),
    })
}

/// Payload specifying a program/verify session.
#[derive(Debug, Clone, Deserialize)]
pub struct SessionSpec {
    pub program: Vec<u16>,
    pub data: Option<Vec<u8>>,
    pub config: Option<u8>,
    pub osc: Option<u8>,
    pub erase: bool,
}

/// Program the device and (optionally) verify. Returns `true` on success.
pub fn program(backend: &mut dyn Backend, spec: &SessionSpec) -> Result<bool, String> {
    let mut img = build_image(spec)?;
    if let Some(cfg) = spec.osc {
        img.config = Some(cfg);
    }
    let opts = WriteOptions {
        erase: spec.erase,
        program: true,
        data: true,
        config: true,
    };
    ops::program_with(backend, &img, &opts).map_err(|e| e.to_string())
}

/// Program the device using the backend's own phase sequence (used by the USB
/// bridge, whose protocol has no read-back: `program()` runs the exact official
/// block sequence and returns `true` when every block is acknowledged).
pub fn program_device(backend: &mut dyn Backend, spec: &SessionSpec) -> Result<bool, String> {
    let img = build_image(spec)?;
    backend.program(&img).map_err(|e| e.to_string())
}

/// Verify the device against the session image.
pub fn verify(backend: &mut dyn Backend, spec: &SessionSpec) -> Result<bool, String> {
    let img = build_image(spec)?;
    let opts = WriteOptions {
        erase: false,
        program: true,
        data: true,
        config: true,
    };
    ops::verify_with(backend, &img, &opts).map_err(|e| e.to_string())
}

/// Read the device state (program words + data + config).
pub fn read(backend: &mut dyn Backend) -> Result<ImageContents, String> {
    let img = ops::read(backend).map_err(|e| e.to_string())?;
    Ok(ImageContents {
        program: img.program,
        data: img.data.to_vec(),
        config: img.config,
    })
}

/// A read-back image summary for the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageContents {
    pub program: Vec<u16>,
    pub data: Vec<u8>,
    pub config: Option<u8>,
}

fn build_image(spec: &SessionSpec) -> Result<MemoryImage, String> {
    let mut img = MemoryImage::new();
    let n = spec.program.len().min(img.program.len());
    img.program[..n].copy_from_slice(&spec.program[..n]);
    if let Some(bytes) = &spec.data {
        let n = bytes.len().min(DATA_EEPROM_BYTES);
        img.data[..n].copy_from_slice(&bytes[..n]);
    }
    img.config = spec.config;
    Ok(img)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn demo_spec() -> SessionSpec {
        SessionSpec {
            program: vec![0x8123u16, 0x0000, 0xFFFF],
            data: None,
            config: None,
            osc: None,
            erase: true,
        }
    }

    #[test]
    fn assemble_runs_demo_program() {
        let src = "movl %a1,24h\naddl %a1,12h\naddl %a1,55h\n";
        let res = assemble(src).unwrap();
        assert!(!res.words.is_empty());
        assert_eq!(res.words.len(), 1024);
        assert!(!res.listing.is_empty());
    }

    #[test]
    fn assemble_rejects_garbage() {
        assert!(assemble("not an instruction <<<>>").is_err());
    }

    #[test]
    fn program_on_mock_roundtrips_when_verify_skipped() {
        let mut backend = MockBackend::new();
        let spec = demo_spec();
        assert!(program(&mut backend, &spec).unwrap());
        let img = read(&mut backend).unwrap();
        assert_eq!(img.program[0], spec.program[0]);
    }

    #[test]
    fn program_mismatch_fails_verify() {
        let mut backend = MockBackend::new();
        let mut spec = demo_spec();
        assert!(program(&mut backend, &spec).unwrap());
        spec.program[0] = 0;
        assert!(!verify(&mut backend, &spec).unwrap());
    }

    #[test]
    fn sim_backend_survives_across_ops_via_shared_backend() {
        // The app keeps one SimBackend for the whole session; a fresh one
        // (as in the CLI per-invocation model) would lose state.
        let mut boxed = make_backend(BackendKind::Sim, None, None).unwrap();
        let backend: &mut dyn Backend = &mut *boxed;
        let mut spec = demo_spec();
        spec.osc = Some(0x02);
        assert!(program(backend, &spec).unwrap());
        let img = read(backend).unwrap();
        assert_eq!(img.program[0], 0x8123);
        assert_eq!(img.config, Some(0x02));
    }

    #[test]
    fn mock_read_returns_device_name() {
        let mut backend = make_backend(BackendKind::Mock, None, None).unwrap();
        assert!(backend.identify().unwrap().name.contains("mock"));
    }

    #[test]
    fn connect_reports_backend_identity() {
        let mock = connect(BackendKind::Mock, None, None).unwrap();
        assert!(mock.label.to_lowercase().contains("mock"));
        assert!(connect(BackendKind::Sim, None, None).is_ok());
    }
}
