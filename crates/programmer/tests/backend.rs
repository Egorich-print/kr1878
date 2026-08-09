//! End-to-end backend integration: business ops over `MockBackend`.

use kr1878_core::memory::MemoryImage;
use kr1878_core::ops;
use kr1878_programmer::MockBackend;

#[test]
fn program_then_read_roundtrips() {
    let mut backend = MockBackend::new();
    let mut img = MemoryImage::new();
    img.program[0] = 0x8123;
    img.program[511] = 0xDEAD;
    img.data[63] = 0x5A;

    assert!(ops::program(&mut backend, &img).unwrap());
    let read = ops::read(&mut backend).unwrap();
    assert_eq!(read.program, img.program);
    assert_eq!(read.data, img.data);
}

#[test]
fn verify_reports_mismatch_and_match() {
    let mut backend = MockBackend::new();
    let mut img = MemoryImage::new();
    img.program[4] = 0xF00F;
    ops::program(&mut backend, &img).unwrap();

    // A different image must not verify.
    let mut other = img.clone();
    other.program[4] = 0;
    assert!(!ops::verify(&mut backend, &other).unwrap());
    assert!(ops::verify(&mut backend, &img).unwrap());
}
