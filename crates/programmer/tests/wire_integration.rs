//! End-to-end integration through the *real* wire path: `WireBackend` drives
//! `kr1878-protocol`'s device driver over a simulated chip, then a polarity-
//! inverted status transport. No `MockBackend`: every opcode, bit cell and
//! read clock is exercised against the protocol state machine, the same path
//! a physical LPT programmer takes.

use kr1878_core::Backend;
use kr1878_core::MemoryImage;
use kr1878_core::memory::PROGRAM_WORDS;
use kr1878_core::ops;
use kr1878_programmer::WireBackend;
use kr1878_protocol::testutil::{InvertedStatusTransport, SimulatorTransport};
use kr1878_protocol::transport::{STATUS_ACK, STATUS_BUSY};

fn run_program_read_verify(transport: &mut dyn kr1878_protocol::transport::Transport) {
    let mut backend = WireBackend::new(transport);

    let mut img = MemoryImage::new();
    img.program[0] = 0x8123;
    img.program[1] = 0x0000;
    img.program[2] = 0xFFFF;
    img.program[511] = 0xDEAD;
    img.data[0] = 0x00;
    img.data[63] = 0x5A;
    img.config = Some(0x02);

    // Erase -> write -> verify through the protocol layer.
    assert!(ops::program(&mut backend, &img).unwrap());

    // Independent read-back over the same wire path.
    let read = ops::read(&mut backend).unwrap();
    assert_eq!(read.program, img.program);
    assert_eq!(read.data, img.data);
    assert_eq!(read.config, img.config);

    // Corrupted image must fail verification.
    let mut other = img.clone();
    other.program[3] = 0x1234;
    assert!(!ops::verify(&mut backend, &other).unwrap());
    assert!(ops::verify(&mut backend, &img).unwrap());
}

#[test]
fn wire_backend_erase_write_read_verify_roundtrips() {
    let mut sim = SimulatorTransport::default();
    run_program_read_verify(&mut sim);
}

#[test]
fn wire_backend_roundtrips_over_inverted_status_polarity() {
    // LPT hardware inverts BUSY/ACK on the wire; the transport normalises the
    // levels back to active-high before the protocol sees them. The full
    // program/read/verify session must still round-trip through the inverted
    // wire, proving the polarity model matches the decoder's contract.
    let sim = SimulatorTransport::default();
    let mut wire = InvertedStatusTransport::inverted(sim);
    run_program_read_verify(&mut wire);
}

#[test]
fn inverted_wire_presents_normalized_status_to_protocol() {
    // A chip reads ACK high (0x40) while idle; the raw LPT wire carries the
    // inverted levels (ACK low, BUSY high). The normalising transport restores
    // the active-high levels the protocol expects.
    let mut passthrough = InvertedStatusTransport::passthrough(SimulatorTransport::default());
    let mut wire = InvertedStatusTransport::inverted(SimulatorTransport::default());

    let active = passthrough.raw().unwrap();
    let raw = wire.raw().unwrap();
    let norm = wire.normalized().unwrap();

    assert_eq!(active, STATUS_ACK); // clean wire: BUSY low, ACK high
    assert_eq!(raw, STATUS_BUSY); // inverted wire: BUSY high, ACK low
    assert_eq!(norm, active); // decoded levels are back to active-high
}

#[test]
fn program_without_data_phase_preserves_data_eeprom() {
    use kr1878_core::ops::WriteOptions;

    let mut sim = SimulatorTransport::default();
    let mut backend = WireBackend::new(&mut sim);
    let mut img = MemoryImage::new();
    img.program[0] = 0xA5A5;
    img.data = [0x5A; 64];
    img.config = Some(0x01);

    // First pass writes a full image (including data EEPROM).
    let full = WriteOptions::default();
    assert!(ops::program_with(&mut backend, &img, &full).unwrap());

    // Second pass selects program+config only (`data: false`). The data EEPROM
    // must survive untouched even though the whole image is written again.
    let opts = WriteOptions {
        erase: true,
        program: true,
        data: false,
        config: true,
    };
    assert!(!opts.data);
    assert!(ops::program_with(&mut backend, &img, &opts).unwrap());

    let read = ops::read(&mut backend).unwrap();
    assert_eq!(read.program[0], 0xA5A5);
    assert_eq!(read.config, Some(0x01));
    assert_eq!(read.data, img.data); // untouched by this pass
    assert_eq!(read.program.len(), PROGRAM_WORDS);
}

#[test]
fn identify_reports_device_name() {
    let mut sim = SimulatorTransport::default();
    let mut backend = WireBackend::new(&mut sim);
    let id = backend.identify().unwrap();
    assert_eq!(id.name, "KR1878VE1");
}
