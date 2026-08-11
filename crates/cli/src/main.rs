//! `kr1878` — CLI for the KR1878VE1 toolchain.
//!
//! Subcommands:
//!   assemble   `.mic` → `.sav` (+ optional `.dat`)
//!   read       dump device/mock image to `.sav` / `.dat`
//!   write      program a `.sav` (+ `.dat`) image (optionally `--osc`, `--arp`)
//!   verify     compare device against a `.sav` (+ `.dat`) image

use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};

use kr1878_core::backend::Backend;
use kr1878_core::memory::MemoryImage;
use kr1878_core::ops::{self, WriteOptions};
use kr1878_file_format::arp as arpfmt;
use kr1878_file_format::dat as datfmt;
use kr1878_file_format::sav as savfmt;
use kr1878_programmer::{
    GeneratorMode, LptBackend, MockBackend, SimBackend, UsbBridge, UsbBridgeBackend,
};

#[derive(Parser)]
#[command(name = "kr1878", about = "KR1878VE1 toolchain CLI")]
struct Cli {
    /// Extra `#include` search directory (relative to the current working
    /// directory).
    #[arg(long, global = true)]
    include_dir: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum BackendKind {
    /// In-memory backend that skips the protocol layer (test/CI).
    Mock,
    /// Real wire protocol against a simulated chip (no hardware).
    Sim,
    /// Physical parallel-port programmer.
    Lpt,
    /// USB/COM bridge programmer (official `Uprg1878.exe` block protocol).
    Usb,
}

#[derive(Clone, Copy, ValueEnum)]
enum GenKind {
    /// External resistor-capacitor generator.
    Rc,
    /// Internal generator (~50 kHz).
    Internal,
    /// External quartz 500 kHz – 8 MHz.
    QuartzHigh,
    /// External quartz below 500 kHz.
    QuartzLow,
}

impl From<GenKind> for GeneratorMode {
    fn from(k: GenKind) -> Self {
        match k {
            GenKind::Rc => GeneratorMode::Rc,
            GenKind::Internal => GeneratorMode::Internal,
            GenKind::QuartzHigh => GeneratorMode::QuartzHigh,
            GenKind::QuartzLow => GeneratorMode::QuartzLow,
        }
    }
}

#[derive(Subcommand)]
enum Cmd {
    /// Assemble a `.mic` source to a `.sav` image.
    Assemble {
        source: PathBuf,
        /// Output machine image (default: `<source>.sav`).
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// Also write the data EEPROM bytes as a `.dat` hex file.
        #[arg(short, long)]
        dat: Option<PathBuf>,
        /// Also write the assembler listing to this file (`.lst`).
        #[arg(long)]
        lst: Option<PathBuf>,
        /// Also write the symbol table to this file (`.sym`).
        #[arg(long)]
        sym: Option<PathBuf>,
    },
    /// Read the device into `.sav` / `.dat` files.
    Read {
        #[arg(short, long, value_enum, default_value_t = BackendKind::Mock)]
        backend: BackendKind,
        /// Output `.sav` (default: `dump.sav`).
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// Output `.dat` (default: none).
        #[arg(short, long)]
        dat: Option<PathBuf>,
        /// Simulated-device state file (`.sim`); `--backend sim` only.
        #[arg(long)]
        sim_state: Option<PathBuf>,
        /// Serial port for the USB bridge; `--backend usb` only.
        #[arg(long)]
        port: Option<String>,
    },
    /// Write a `.sav` or `.mic` (optionally `.dat`) image to the device.
    Write {
        image: PathBuf,
        #[arg(short, long)]
        dat: Option<PathBuf>,
        #[arg(short = 'b', long, value_enum, default_value_t = BackendKind::Mock)]
        backend: BackendKind,
        /// OSC config byte (original `/c:NNN`).
        #[arg(long)]
        osc: Option<u8>,
        /// Programmer config `e03prg.arp` (phase gates).
        #[arg(long)]
        arp: Option<PathBuf>,
        /// Skip the erase phase.
        #[arg(long)]
        no_erase: bool,
        /// Simulated-device state file (`.sim`); `--backend sim` only.
        #[arg(long)]
        sim_state: Option<PathBuf>,
        /// Serial port for the USB bridge; `--backend usb` only.
        #[arg(long)]
        port: Option<String>,
        /// Generator clock mode for the USB bridge config word
        /// (default: `quartz-high`).
        #[arg(long, value_enum)]
        gen_mode: Option<GenKind>,
        /// Disable the startup-delay bit (RDE) in the USB bridge config word.
        #[arg(long)]
        no_delay: bool,
    },
    /// Verify the device against a `.sav` or `.mic` (optionally `.dat`) image.
    Verify {
        image: PathBuf,
        #[arg(short, long)]
        dat: Option<PathBuf>,
        #[arg(short = 'b', long, value_enum, default_value_t = BackendKind::Mock)]
        backend: BackendKind,
        /// OSC config byte to expect (e.g. `/c:NNN`).
        #[arg(long)]
        osc: Option<u8>,
        /// Program `e03prg.arp` (phase gates).
        #[arg(long)]
        arp: Option<PathBuf>,
        /// Simulated-device state file (`.sim`); `--backend sim` only.
        #[arg(long)]
        sim_state: Option<PathBuf>,
        /// Serial port for the USB bridge; `--backend usb` only.
        #[arg(long)]
        port: Option<String>,
    },
    /// List available serial ports (for the USB bridge).
    Ports,
    /// Execute a `.sav`/`.mic` program on the instruction-accurate CPU
    /// simulator and dump the final state.
    Run {
        image: PathBuf,
        /// Max instructions to execute (default: 1_000_000).
        #[arg(long, default_value_t = 1_000_000)]
        steps: u64,
        /// Dump data memory (all 256 bytes) after execution.
        #[arg(long)]
        dump: bool,
        /// Dump data EEPROM (64 bytes) after execution.
        #[arg(long)]
        eeprom: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    if let Some(dir) = cli.include_dir {
        set_include_dir(dir)?;
    }
    match cli.cmd {
        Cmd::Assemble {
            source,
            out,
            dat,
            lst,
            sym,
        } => cmd_assemble(source, out, dat, lst, sym),
        Cmd::Read {
            backend,
            out,
            dat,
            sim_state,
            port,
        } => cmd_read(backend, out, dat, sim_state, port),
        Cmd::Write {
            image,
            dat,
            backend,
            osc,
            arp,
            no_erase,
            sim_state,
            port,
            gen_mode,
            no_delay,
        } => cmd_write(image, dat, backend, osc, arp, no_erase, sim_state, port, gen_mode, no_delay),
        Cmd::Verify {
            image,
            dat,
            backend,
            osc,
            arp,
            sim_state,
            port,
        } => cmd_verify(image, dat, backend, osc, arp, sim_state, port),
        Cmd::Ports => cmd_ports(),
        Cmd::Run { image, steps, dump, eeprom } => cmd_run(image, steps, dump, eeprom),
    }
}

fn make_backend(kind: BackendKind, state: Option<&Path>) -> Result<Box<dyn Backend>> {
    match kind {
        BackendKind::Mock => Ok(Box::new(MockBackend::new())),
        BackendKind::Lpt => Ok(Box::new(LptBackend::new())),
        BackendKind::Sim => {
            let backend = match state {
                Some(p) => SimBackend::load_or_new(p).context("load simulated chip state")?,
                None => SimBackend::new(),
            };
            Ok(Box::new(backend))
        }
        BackendKind::Usb => bail!("`--backend usb` requires `--port` (see `kr1878 write --help`)"),
    }
}

static INCLUDE_DIR: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

/// Record the `#include` search directory from the global CLI flag.
fn set_include_dir(dir: PathBuf) -> Result<()> {
    if !dir.is_dir() {
        bail!("include dir does not exist: {}", dir.display());
    }
    let _ = INCLUDE_DIR.set(dir);
    Ok(())
}

/// Load an `#include`d file from the configured search directory (or the
/// current working directory as a fallback), used by the assembler loader.
fn load_include(name: &str) -> std::io::Result<String> {
    let dir = INCLUDE_DIR.get();
    let candidate = dir
        .map(|d| d.join(name))
        .unwrap_or_else(|| std::path::PathBuf::from(name));
    std::fs::read_to_string(&candidate).map_err(|e| {
        std::io::Error::new(
            e.kind(),
            format!("cannot load `#include \"{name}\"` from {}: {e}", candidate.display()),
        )
    })
}

/// Assemble `.mic` source text with the CLI `#include` loader enabled.
fn assemble_mic(text: &str) -> anyhow::Result<kr1878_assembler::AssembleOutput> {
    let mut loader = |name: &str| -> kr1878_assembler::error::Result<String> {
        load_include(name).map_err(|e| {
            kr1878_assembler::error::AsmError::new(
                kr1878_assembler::error::Span { file: 0, line: 0 },
                e.to_string(),
            )
        })
    };
    kr1878_assembler::assemble_with_loader(text, Some(&mut loader)).context("assembly failed")
}

fn make_usb(port: Option<&str>, delay: bool, mode: GenKind) -> Result<UsbBridgeBackend> {
    let port = port.context("`--backend usb` requires `--port <serial port>`")?;
    UsbBridgeBackend::open(port, delay, mode.into()).context("open USB bridge")
}

fn load_arp(path: Option<&Path>) -> Result<WriteOptions> {
    match path {
        None => Ok(WriteOptions::default()),
        Some(p) => {
            let bytes = std::fs::read(p).with_context(|| format!("open {}", p.display()))?;
            let cfg = arpfmt::parse_arp(&bytes).context("parse .arp")?;
            Ok(WriteOptions {
                erase: cfg.erase_option,
                program: cfg.program_write,
                data: cfg.data_write,
                config: cfg.config_write,
            })
        }
    }
}

fn cmd_assemble(
    source: PathBuf,
    out: Option<PathBuf>,
    dat: Option<PathBuf>,
    lst: Option<PathBuf>,
    sym: Option<PathBuf>,
) -> Result<()> {
    let text = std::fs::read_to_string(&source)
        .with_context(|| format!("cannot read {}", source.display()))?;
    let result = assemble_mic(&text)?;
    let out = out.unwrap_or_else(|| source.with_extension("sav"));
    let words = result.image.program;
    write_sav(&out, &words)?;
    println!("assemble: {} words -> {}", words.len(), out.display());
    if let Some(dat_path) = dat {
        write_dat(&dat_path, &result.image.data)?;
        println!("assemble: data -> {}", dat_path.display());
    }
    if let Some(lst_path) = lst {
        std::fs::write(&lst_path, &result.listing)
            .with_context(|| format!("write listing {}", lst_path.display()))?;
        println!("assemble: listing -> {}", lst_path.display());
    }
    if let Some(sym_path) = sym {
        let mut text = String::new();
        for (name, v) in result.symbols.globals() {
            let kind = if v.is_label { "label" } else { "const" };
            text.push_str(&format!("{kind:6} {name:<24} = 0x{:03X} ; line {}\n", v.value, v.line));
        }
        std::fs::write(&sym_path, text)
            .with_context(|| format!("write symbols {}", sym_path.display()))?;
        println!("assemble: symbols -> {}", sym_path.display());
    }
    Ok(())
}

fn cmd_read(
    backend: BackendKind,
    out: Option<PathBuf>,
    dat: Option<PathBuf>,
    sim_state: Option<PathBuf>,
    _port: Option<String>,
) -> Result<()> {
    if backend == BackendKind::Usb {
        bail!("USB bridge has no read-back (official protocol is write-only); cannot `read`");
    }
    let mut b = make_backend(backend, sim_state.as_deref())?;
    let id = b.identify()?;
    println!(
        "device: {} ({})",
        id.name,
        id.revision.as_deref().unwrap_or("unknown")
    );
    let img = ops::read(&mut *b)?;
    let out = out.unwrap_or_else(|| PathBuf::from("dump.sav"));
    write_sav(&out, &img.program)?;
    println!("read: {} words -> {}", img.program.len(), out.display());
    if let Some(d) = dat {
        write_dat(&d, &img.data)?;
        println!("read: data -> {}", d.display());
    }
    if let Some(cfg) = img.config {
        println!("read: osc config = 0x{cfg:02X}");
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn cmd_write(
    image: PathBuf,
    dat: Option<PathBuf>,
    backend: BackendKind,
    osc: Option<u8>,
    arp: Option<PathBuf>,
    no_erase: bool,
    sim_state: Option<PathBuf>,
    port: Option<String>,
    gen_mode: Option<GenKind>,
    no_delay: bool,
) -> Result<()> {
    let mut img = load_image(&image, dat.as_deref())?;
    if backend == BackendKind::Usb {
        let mode = gen_mode.unwrap_or(GenKind::QuartzHigh);
        let mut b = make_usb(port.as_deref(), !no_delay, mode)?;
        if let Some(osc) = osc {
            eprintln!("note: USB bridge derives the config word from `--gen`/`--no-delay`, ignoring `--osc {osc:02X}`");
        }
        if no_erase {
            eprintln!("note: USB bridge ignores `--no-erase` (sync block is always sent)");
        }
        // `program()` runs the exact official phase order: sync → data ×2 →
        // program → config → program. No read-back exists; success = all
        // blocks acknowledged.
        if b.program(&img)? {
            println!("programmed (USB bridge, write-only)");
            Ok(())
        } else {
            bail!("program failed on USB bridge")
        }
    } else {
        let mut b = make_backend(backend, sim_state.as_deref())?;
        img.config = osc.or(img.config);
        let mut opts = load_arp(arp.as_deref())?;
        if no_erase {
            opts.erase = false;
        }
        if ops::program_with(&mut *b, &img, &opts)? {
            println!("programmed + verified");
            Ok(())
        } else {
            bail!("verify failed after programming")
        }
    }
}

fn cmd_verify(
    image: PathBuf,
    dat: Option<PathBuf>,
    backend: BackendKind,
    osc: Option<u8>,
    arp: Option<PathBuf>,
    sim_state: Option<PathBuf>,
    _port: Option<String>,
) -> Result<()> {
    if backend == BackendKind::Usb {
        bail!("USB bridge has no read-back (official protocol is write-only); cannot `verify`");
    }
    let mut b = make_backend(backend, sim_state.as_deref())?;
    let mut img = load_image(&image, dat.as_deref())?;
    img.config = osc.or(img.config);
    let opts = load_arp(arp.as_deref())?;
    if ops::verify_with(&mut *b, &img, &opts)? {
        println!("verify OK");
        Ok(())
    } else {
        bail!("verify failed")
    }
}

fn cmd_ports() -> Result<()> {
    let ports = UsbBridge::available_ports();
    if ports.is_empty() {
        println!("no serial ports found");
    } else {
        for p in ports {
            println!("{p}");
        }
    }
    Ok(())
}

fn cmd_run(image: PathBuf, steps: u64, dump: bool, eeprom: bool) -> Result<()> {
    let img = load_image(&image, None)?;
    let mut cpu = kr1878_core::Cpu::new();
    cpu.load_program(&img.program);
    cpu.reset();
    let done = cpu.run(steps).context("simulation failed")?;
    println!(
        "sim: executed {done}/{steps} instructions (pc=0x{:03X}, stopped={})",
        cpu.pc, cpu.stopped
    );
    if cpu.stopped {
        println!("sim: program reached STOP");
    } else if done < steps {
        println!("sim: program ended early (STOP/error)");
    } else {
        eprintln!(
            "sim: step limit reached without STOP — program may be an infinite loop"
        );
    }
    println!("sim: RS = 0x{:02X} (C={} Z={} S={} IE={} OF={} DC={})",
        cpu.rs.to_byte(), cpu.rs.c as u8, cpu.rs.z as u8, cpu.rs.s as u8,
        cpu.rs.ie as u8, cpu.rs.of as u8, cpu.rs.dc as u8);
    println!("sim: port A = 0x{:02X}, port B = 0x{:02X}", cpu.port_a(), cpu.port_b());
    if dump {
        println!("sim: data memory:");
        for row in 0..16 {
            let base = row * 16;
            let bytes: Vec<String> = cpu.data[base..base + 16]
                .iter()
                .map(|b| format!("{b:02X}"))
                .collect();
            println!("  {base:02X}: {}", bytes.join(" "));
        }
    }
    if eeprom {
        let e = cpu.data_eeprom();
        println!("sim: data EEPROM:");
        for row in 0..4 {
            let base = row * 16;
            let bytes: Vec<String> = e[base..base + 16].iter().map(|b| format!("{b:02X}")).collect();
            println!("  C{base:02X}: {}", bytes.join(" "));
        }
    }
    Ok(())
}

fn load_image(sav: &Path, dat: Option<&Path>) -> Result<MemoryImage> {
    let mut img = match sav.extension().and_then(|e| e.to_str()) {
        Some("mic") => {
            let text = std::fs::read_to_string(sav)
                .with_context(|| format!("cannot read {}", sav.display()))?;
            let result = assemble_mic(&text)?;
            result.image
        }
        _ => {
            let mut f =
                BufReader::new(File::open(sav).with_context(|| format!("open {}", sav.display()))?);
            let words = savfmt::read_sav(&mut f)?;
            MemoryImage::from_program(&words)?
        }
    };
    if let Some(d) = dat {
        let text = std::fs::read_to_string(d).with_context(|| format!("open {}", d.display()))?;
        img.data = datfmt::parse_dat(&text)?;
    }
    Ok(img)
}

fn write_sav(path: &Path, words: &[u16]) -> Result<()> {
    let f = File::create(path).with_context(|| format!("create {}", path.display()))?;
    savfmt::write_sav(&mut BufWriter::new(f), words)?;
    Ok(())
}

fn write_dat(path: &Path, bytes: &[u8]) -> Result<()> {
    let f = File::create(path).with_context(|| format!("create {}", path.display()))?;
    datfmt::write_dat(&mut BufWriter::new(f), bytes)?;
    Ok(())
}
