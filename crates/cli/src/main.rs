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
use kr1878_programmer::{LptBackend, MockBackend, SimBackend};

#[derive(Parser)]
#[command(name = "kr1878", about = "KR1878VE1 toolchain CLI")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Clone, Copy, ValueEnum)]
enum BackendKind {
    /// In-memory backend that skips the protocol layer (test/CI).
    Mock,
    /// Real wire protocol against a simulated chip (no hardware).
    Sim,
    /// Physical parallel-port programmer.
    Lpt,
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
    },
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Assemble { source, out, dat } => cmd_assemble(source, out, dat),
        Cmd::Read {
            backend,
            out,
            dat,
            sim_state,
        } => cmd_read(backend, out, dat, sim_state),
        Cmd::Write {
            image,
            dat,
            backend,
            osc,
            arp,
            no_erase,
            sim_state,
        } => cmd_write(image, dat, backend, osc, arp, no_erase, sim_state),
        Cmd::Verify {
            image,
            dat,
            backend,
            osc,
            arp,
            sim_state,
        } => cmd_verify(image, dat, backend, osc, arp, sim_state),
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
    }
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
) -> Result<()> {
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

fn cmd_write(
    image: PathBuf,
    dat: Option<PathBuf>,
    backend: BackendKind,
    osc: Option<u8>,
    arp: Option<PathBuf>,
    no_erase: bool,
    sim_state: Option<PathBuf>,
) -> Result<()> {
    let mut b = make_backend(backend, sim_state.as_deref())?;
    let mut img = load_image(&image, dat.as_deref())?;
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

fn cmd_verify(
    image: PathBuf,
    dat: Option<PathBuf>,
    backend: BackendKind,
    osc: Option<u8>,
    arp: Option<PathBuf>,
    sim_state: Option<PathBuf>,
) -> Result<()> {
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
