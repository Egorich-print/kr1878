//! `kr1878` — CLI for the KR1878VE1 toolchain.
//!
//! Subcommands:
//!   assemble   `.mic` → `.sav` (+ optional `.dat`)
//!   read       dump device/mock image to `.sav` / `.dat`
//!   write      program a `.sav` (+ `.dat`) image
//!   verify     compare device against a `.sav` (+ `.dat`) image

use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};

use kr1878_core::backend::Backend;
use kr1878_core::memory::MemoryImage;
use kr1878_core::ops;
use kr1878_file_format::dat as datfmt;
use kr1878_file_format::sav as savfmt;
use kr1878_programmer::{LptBackend, MockBackend};

#[derive(Parser)]
#[command(name = "kr1878", about = "KR1878VE1 toolchain CLI")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Clone, Copy, ValueEnum)]
enum BackendKind {
    Mock,
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
    },
    /// Write a `.sav` (optionally `.dat`) image to the device.
    Write {
        image: PathBuf,
        #[arg(short, long)]
        dat: Option<PathBuf>,
        #[arg(short = 'b', value_enum, default_value_t = BackendKind::Mock)]
        backend: BackendKind,
    },
    /// Verify the device against a `.sav` (optionally `.dat`) image.
    Verify {
        image: PathBuf,
        #[arg(short, long)]
        dat: Option<PathBuf>,
        #[arg(short = 'b', value_enum, default_value_t = BackendKind::Mock)]
        backend: BackendKind,
    },
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Assemble { source, out, dat } => cmd_assemble(source, out, dat),
        Cmd::Read { backend, out, dat } => cmd_read(backend, out, dat),
        Cmd::Write { image, dat, backend } => cmd_write(image, dat, backend),
        Cmd::Verify { image, dat, backend } => cmd_verify(image, dat, backend),
    }
}

fn make_backend(kind: BackendKind) -> Box<dyn Backend> {
    match kind {
        BackendKind::Mock => Box::new(MockBackend::new()),
        BackendKind::Lpt => Box::new(LptBackend::new()),
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

fn cmd_read(backend: BackendKind, out: Option<PathBuf>, dat: Option<PathBuf>) -> Result<()> {
    let mut b = make_backend(backend);
    let id = b.identify()?;
    println!("device: {} ({})", id.name, id.revision.as_deref().unwrap_or("unknown"));
    let img = ops::read(&mut *b)?;
    let out = out.unwrap_or_else(|| PathBuf::from("dump.sav"));
    write_sav(&out, &img.program)?;
    println!("read: {} words -> {}", img.program.len(), out.display());
    if let Some(d) = dat {
        write_dat(&d, &img.data)?;
        println!("read: data -> {}", d.display());
    }
    Ok(())
}

fn cmd_write(image: PathBuf, dat: Option<PathBuf>, backend: BackendKind) -> Result<()> {
    let mut b = make_backend(backend);
    let img = load_image(&image, dat.as_deref())?;
    if ops::program(&mut *b, &img)? {
        println!("programmed + verified");
        Ok(())
    } else {
        bail!("verify failed after programming")
    }
}

fn cmd_verify(image: PathBuf, dat: Option<PathBuf>, backend: BackendKind) -> Result<()> {
    let mut b = make_backend(backend);
    let img = load_image(&image, dat.as_deref())?;
    if ops::verify(&mut *b, &img)? {
        println!("verify OK");
        Ok(())
    } else {
        bail!("verify failed")
    }
}

fn load_image(sav: &Path, dat: Option<&Path>) -> Result<MemoryImage> {
    let mut f = BufReader::new(File::open(sav).with_context(|| format!("open {}", sav.display()))?);
    let words = savfmt::read_sav(&mut f)?;
    let mut img = MemoryImage::from_program(&words)?;
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