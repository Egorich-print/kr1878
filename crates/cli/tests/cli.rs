//! CLI integration tests: run the compiled `kr1878` binary end-to-end.
//!
//! These exercise the real process boundary (arg parsing, exit codes, file
//! I/O, backend construction) including the fully simulated device with
//! durable `.sim` state.

use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_kr1878");

fn tmp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "kr1878_cli_test_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run(args: &[&str]) -> std::process::Output {
    Command::new(BIN).args(args).output().unwrap()
}

const DEMO_MIC: &str = "\
movl %a1,24h
addl %a1,12h
addl %a1,55h
";

fn write(path: &Path, content: &str) {
    std::fs::write(path, content).unwrap();
}

#[test]
fn assemble_creates_sav() {
    let dir = tmp_dir();
    let mic = dir.join("demo.mic");
    let sav = dir.join("demo.sav");
    write(&mic, DEMO_MIC);

    let out = run(&[
        "assemble",
        mic.to_str().unwrap(),
        "-o",
        sav.to_str().unwrap(),
    ]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(sav.exists());
    let words = std::fs::read(&sav).unwrap();
    // 1024 16-bit words, little-endian.
    assert_eq!(words.len(), 1024 * 2);
}

#[test]
fn assemble_to_mic_write_verify_roundtrips_across_processes() {
    let dir = tmp_dir();
    let mic = dir.join("prog.mic");
    let state = dir.join("chip.sim");
    write(&mic, DEMO_MIC);

    // Write through the real protocol path; persists the chip to chip.sim.
    let w = run(&[
        "write",
        mic.to_str().unwrap(),
        "--backend",
        "sim",
        "--sim-state",
        state.to_str().unwrap(),
        "--osc",
        "3",
    ]);
    assert!(
        w.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&w.stderr)
    );
    assert!(state.exists());

    // A separate process with the same state file must verify against the
    // same image (proves the chip survived across invocations).
    let v = run(&[
        "verify",
        mic.to_str().unwrap(),
        "--backend",
        "sim",
        "--sim-state",
        state.to_str().unwrap(),
        "--osc",
        "3",
    ]);
    assert!(
        v.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&v.stderr)
    );

    // A separate process with a different config expectation must fail.
    let bad = run(&[
        "verify",
        mic.to_str().unwrap(),
        "--backend",
        "sim",
        "--sim-state",
        state.to_str().unwrap(),
        "--osc",
        "9",
    ]);
    assert!(!bad.status.success());
}

#[test]
fn read_dumps_persisted_program_and_config() {
    let dir = tmp_dir();
    let mic = dir.join("prog.mic");
    let state = dir.join("chip.sim");
    let dump = dir.join("dump.sav");
    write(&mic, DEMO_MIC);

    let w = run(&[
        "write",
        mic.to_str().unwrap(),
        "--backend",
        "sim",
        "--sim-state",
        state.to_str().unwrap(),
        "--osc",
        "5",
    ]);
    assert!(w.status.success());

    let r = run(&[
        "read",
        "--backend",
        "sim",
        "--sim-state",
        state.to_str().unwrap(),
        "--out",
        dump.to_str().unwrap(),
        "--dat",
        dir.join("dump.dat").to_str().unwrap(),
    ]);
    assert!(
        r.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&r.stderr)
    );
    let stdout = String::from_utf8_lossy(&r.stdout);
    assert!(stdout.contains("osc config = 0x05"), "stdout: {stdout}");
    assert!(dump.exists());
}

#[test]
fn usb_read_reports_no_readback() {
    let out = run(&["read", "--backend", "usb"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("no read-back"), "stderr: {stderr}");
}

#[test]
fn usb_verify_reports_no_readback() {
    let out = run(&["verify", "x.sav", "--backend", "usb"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("no read-back"), "stderr: {stderr}");
}

#[test]
fn usb_write_without_port_reports_port_required() {
    let dir = tmp_dir();
    let mic = dir.join("demo.mic");
    write(&mic, DEMO_MIC);
    let out = run(&["write", mic.to_str().unwrap(), "--backend", "usb"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("requires `--port"),
        "stderr: {stderr}"
    );
}

#[test]
fn ports_command_lists_or_reports_none() {
    let out = run(&["ports"]);
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    // Either a list of ports or a "no ports" message; never an error.
    assert!(
        stdout.contains("no serial ports") || stdout.contains("/dev/") || stdout.contains("COM"),
        "stdout: {stdout}"
    );
}
