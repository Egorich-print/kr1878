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
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Assemble { source, out, dat } => cmd_assemble(source, out, dat),
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

fn cmd_assemble(source: PathBuf, out: Option<PathBuf>, dat: Option<PathBuf>) -> Result<()> {
    let text = std::fs::read_to_string(&source)
        .with_context(|| format!("cannot read {}", source.display()))?;
    let result = kr1878_assembler::assemble(&text).context("assembly failed")?;
    let out = out.unwrap_or_else(|| source.with_extension("sav"));
    let words = result.image.program;
    write_sav(&out, &words)?;
    println!("assemble: {} words -> {}", words.len(), out.display());
    if let Some(dat_path) = dat {
        write_dat(&dat_path, &result.image.data)?;
        println!("assemble: data -> {}", dat_path.display());
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

fn load_image(sav: &Path, dat: Option<&Path>) -> Result<MemoryImage> {
    let mut img = match sav.extension().and_then(|e| e.to_str()) {
        Some("mic") => {
            let text = std::fs::read_to_string(sav)
                .with_context(|| format!("cannot read {}", sav.display()))?;
            let result = kr1878_assembler::assemble(&text).context("assembly failed")?;
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
