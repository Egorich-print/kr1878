//! USB/COM bridge programmer backend, replicating the official `Uprg1878.exe`
//! block protocol (see `docs/HARDWARE_PROTOCOL.md` §7).
//!
//! Unlike the LPT bit-bang path ([`crate::lpt`]), the official modern Angstrom
//! programmer talks to the host over a UART through a USB bridge at
//! **38400-8N1**, exchanging fixed 16-byte blocks where each block is
//! acknowledged with a single `'R'` (0x52) byte. There is **no read-back**: the
//! "verification" passes are re-writes of the same data (phases B/E), so this
//! backend keeps a shadow image and returns it from the read methods.
//!
//! Sequence (see §7.3):
//!   sync/erase block (content ignored) → data EEPROM ×2 (4×16 bytes each)
//!   → program `.sav` words (128×16 bytes, big-endian words, skipping the
//!     256-word `.sav` header) → config word block → program repeat.

use std::time::Duration;

use kr1878_core::backend::{Backend, DeviceId};
use kr1878_core::error::{Error, Result};
use kr1878_core::memory::{DATA_EEPROM_BYTES, MemoryImage, PROGRAM_WORDS};

/// Serial parameters used by the official programmer.
pub const UART_BAUD: u32 = 38_400;

/// Fixed exchange block size in bytes (both directions).
pub const BLOCK_LEN: usize = 16;

/// Acknowledgement byte sent by the programmer after each accepted block.
pub const ACK: u8 = b'R';

/// Default read-drain silence timeout for ordinary blocks (ms).
pub const BLOCK_TIMEOUT_MS: u64 = 80;

/// Read timeout for the (slower) config-word block (ms).
pub const CONFIG_TIMEOUT_MS: u64 = 1_000;

/// Program memory blocks per pass (128 blocks × 16 bytes = 1024 words).
const PROGRAM_BLOCKS: usize = 128;

/// Program words per 16-byte block (8 words × 2 bytes).
const WORDS_PER_BLOCK: usize = 8;

/// Data EEPROM blocks per pass (4 blocks × 16 bytes = 64 bytes).
const DATA_BLOCKS: usize = 4;

/// Generator clock-mode selector (the `Uprg1878.exe` "Генератор" radio group).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeneratorMode {
    /// External resistor-capacitor (RC) generator.
    Rc,
    /// Internal generator (~50 kHz).
    Internal,
    /// External quartz 500 kHz – 8 MHz.
    QuartzHigh,
    /// External quartz below 500 kHz.
    QuartzLow,
}

/// 16-bit configuration word assembled from the generator mode and the
/// startup-delay (RDE) bit. Per the datasheet each logical bit is physically
/// repeated three times; `Uprg1878.exe` uses these masks:
///
/// | Source                       | Bits   | Mask   |
/// |------------------------------|--------|--------|
/// | "Задержка" (RDE, startup delay) | 0–2  | 0x0007 |
/// | RC                           | –      | 0x0000 |
/// | Internal generator           | 6–8    | 0x01C0 |
/// | Quartz >500 kHz              | 3–5    | 0x0038 |
/// | Quartz <500 kHz              | 3–8    | 0x01F8 |
pub fn config_word(delay: bool, generator: GeneratorMode) -> u16 {
    let mut w = if delay { 0x0007 } else { 0x0000 };
    w |= match generator {
        GeneratorMode::Rc => 0x0000,
        GeneratorMode::Internal => 0x01C0,
        GeneratorMode::QuartzHigh => 0x0038,
        GeneratorMode::QuartzLow => 0x01F8,
    };
    w
}

/// Low-level COM bridge: owns the serial port and speaks the 16-byte block
/// exchange (`WRITE block → PURGE → drain until 80 ms silence, last byte 'R'`).
pub struct UsbBridge {
    port: Box<dyn serialport::SerialPort>,
}

impl UsbBridge {
    /// Open `port_name` at 38400-8N1 with a 1000 ms write timeout. The read
    /// timeout is set per block by [`UsbBridge::send_block`].
    pub fn open(port_name: &str) -> Result<Self> {
        let port = serialport::new(port_name, UART_BAUD)
            .data_bits(serialport::DataBits::Eight)
            .stop_bits(serialport::StopBits::One)
            .parity(serialport::Parity::None)
            .flow_control(serialport::FlowControl::None)
            .timeout(Duration::from_millis(BLOCK_TIMEOUT_MS))
            .open()
            .map_err(|e| Error::Other(format!("usb: cannot open {port_name}: {e}")))?;
        Ok(UsbBridge { port })
    }

    /// List serial ports whose name starts with `COM` or is a `/dev/tty*`
    /// device, mirroring `GetSerialPortsList` (registry `SERIALCOMM`).
    pub fn available_ports() -> Vec<String> {
        serialport::available_ports()
            .map(|ports| {
                ports
                    .into_iter()
                    .map(|p| p.port_name)
                    .filter(|n| n.starts_with("COM") || n.starts_with("/dev/tty"))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Flush both directions (mirrors `PurgeComm(0x0F)`).
    fn purge(&mut self) -> Result<()> {
        self.port
            .clear(serialport::ClearBuffer::Input)
            .map_err(|e| Error::Other(format!("usb: clear input: {e}")))?;
        self.port
            .clear(serialport::ClearBuffer::Output)
            .map_err(|e| Error::Other(format!("usb: clear output: {e}")))?;
        Ok(())
    }

    /// Drain incoming bytes until silence (per the port read timeout); require
    /// the last received byte to be the acknowledgement `'R'`.
    ///
    /// Mirrors the disassembled loop `READ(rxbuf, 1, timeout)` + "last == 'R'".
    fn drain_ack(&mut self) -> Result<()> {
        let mut last: Option<u8> = None;
        loop {
            let mut byte = [0u8; 1];
            match self.port.read(&mut byte) {
                Ok(0) => break,
                Ok(_) => last = Some(byte[0]),
                Err(e) if e.kind() == std::io::ErrorKind::TimedOut => break,
                Err(e) => {
                    return Err(Error::Other(format!("usb: read: {e}")));
                }
            }
        }
        match last {
            Some(ACK) => Ok(()),
            Some(other) => Err(Error::Other(format!(
                "usb: unexpected acknowledgement 0x{other:02X} (expected 'R')"
            ))),
            None => Err(Error::Other("usb: no acknowledgement (timeout)".into())),
        }
    }

    /// Send one 16-byte block and wait for its `'R'` acknowledgement.
    ///
    /// `timeout_ms` is the per-block read-drain timeout (80 ms normally,
    /// 1000 ms for the config block).
    pub fn send_block(&mut self, block: &[u8; BLOCK_LEN], timeout_ms: u64) -> Result<()> {
        let _ = self.port.set_timeout(Duration::from_millis(timeout_ms));
        self.port
            .write(block)
            .map_err(|e| Error::Other(format!("usb: write: {e}")))?;
        self.purge()?;
        self.drain_ack()
    }

    /// Send one 16-byte block with the default 80 ms timeout.
    pub fn send(&mut self, block: &[u8; BLOCK_LEN]) -> Result<()> {
        self.send_block(block, BLOCK_TIMEOUT_MS)
    }
}

/// Backend replicating the official USB bridge block protocol.
///
/// Because the protocol has no read-back, the backend keeps a **shadow image**:
/// writes update it, and reads return it. "Verification" therefore means the
/// write phases completed (all `'R'` acknowledgements received), matching the
/// official tool's re-write passes.
pub struct UsbBridgeBackend {
    bridge: UsbBridge,
    shadow: MemoryImage,
    delay: bool,
    generator: GeneratorMode,
}

impl UsbBridgeBackend {
    /// Open a backend on `port_name` with the given generator configuration.
    pub fn open(port_name: &str, delay: bool, generator: GeneratorMode) -> Result<Self> {
        let bridge = UsbBridge::open(port_name)?;
        Ok(UsbBridgeBackend {
            bridge,
            shadow: MemoryImage::new(),
            delay,
            generator,
        })
    }

    /// Send the config-word block (phase D). Uses the slower timeout.
    fn send_config(&mut self) -> Result<()> {
        let word = config_word(self.delay, self.generator);
        let mut block = [0xFF; BLOCK_LEN];
        block[0] = (word >> 8) as u8;
        block[1] = (word & 0xFF) as u8;
        self.bridge.send_block(&block, CONFIG_TIMEOUT_MS)
    }

    /// Send one program pass: 128 blocks of 8 big-endian words. Words beyond
    /// the image are sent as `0xFFFF` (erased state).
    fn send_program_pass(&mut self, words: &[u16]) -> Result<()> {
        for block_idx in 0..PROGRAM_BLOCKS {
            let mut block = [0xFF; BLOCK_LEN];
            for word_idx in 0..WORDS_PER_BLOCK {
                let word = words
                    .get(block_idx * WORDS_PER_BLOCK + word_idx)
                    .copied()
                    .unwrap_or(0xFFFF);
                block[word_idx * 2] = (word >> 8) as u8;
                block[word_idx * 2 + 1] = (word & 0xFF) as u8;
            }
            self.bridge.send(&block)?;
        }
        Ok(())
    }

    /// Send one data EEPROM pass: 4 blocks of 16 bytes.
    fn send_data_pass(&mut self, data: &[u8; DATA_EEPROM_BYTES]) -> Result<()> {
        for block_idx in 0..DATA_BLOCKS {
            let mut block = [0xFF; BLOCK_LEN];
            block.copy_from_slice(&data[block_idx * BLOCK_LEN..(block_idx + 1) * BLOCK_LEN]);
            self.bridge.send(&block)?;
        }
        Ok(())
    }

    /// Run the full official sequence (phases 0, A, B, C, D, E) and update the
    /// shadow image.
    fn run_sequence(&mut self, image: &MemoryImage) -> Result<()> {
        // Phase 0 — sync/erase block (content ignored by the host).
        self.bridge.send(&[0xFF; BLOCK_LEN])?;
        // Phases A+B — data EEPROM, twice.
        self.send_data_pass(&image.data)?;
        self.send_data_pass(&image.data)?;
        // Phase C — program memory.
        self.send_program_pass(&image.program)?;
        // Phase D — config word.
        self.send_config()?;
        // Phase E — program memory again.
        self.send_program_pass(&image.program)?;
        self.shadow = image.clone();
        Ok(())
    }
}

impl Backend for UsbBridgeBackend {
    fn identify(&mut self) -> Result<DeviceId> {
        Ok(DeviceId {
            name: "KR1878VE1 (USB bridge)".into(),
            revision: None,
        })
    }

    fn erase(&mut self) -> Result<()> {
        self.bridge.send(&[0xFF; BLOCK_LEN])?;
        self.shadow = MemoryImage::new();
        Ok(())
    }

    fn write_program(&mut self, words: &[u16]) -> Result<()> {
        self.send_program_pass(words)?;
        let n = words.len().min(PROGRAM_WORDS);
        self.shadow.program[..n].copy_from_slice(&words[..n]);
        Ok(())
    }

    fn write_data(&mut self, bytes: &[u8; DATA_EEPROM_BYTES]) -> Result<()> {
        self.send_data_pass(bytes)?;
        self.send_data_pass(bytes)?;
        self.shadow.data = *bytes;
        Ok(())
    }

    fn read_program(&mut self, out: &mut [u16]) -> Result<()> {
        out.copy_from_slice(&self.shadow.program[..out.len()]);
        Ok(())
    }

    fn read_data(&mut self) -> Result<[u8; DATA_EEPROM_BYTES]> {
        Ok(self.shadow.data)
    }

    fn program(&mut self, image: &MemoryImage) -> Result<bool> {
        self.run_sequence(image)?;
        Ok(true)
    }

    fn write_config(&mut self, _value: u8) -> Result<()> {
        // The official tool derives the config word from the generator mode +
        // delay, not from an arbitrary byte; ignore the caller's byte.
        self.send_config()?;
        Ok(())
    }

    fn read_config(&mut self) -> Result<Option<u8>> {
        Ok(self.shadow.config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_word_masks_match_uprg1878() {
        assert_eq!(config_word(false, GeneratorMode::Rc), 0x0000);
        assert_eq!(config_word(false, GeneratorMode::Internal), 0x01C0);
        assert_eq!(config_word(false, GeneratorMode::QuartzHigh), 0x0038);
        assert_eq!(config_word(false, GeneratorMode::QuartzLow), 0x01F8);
        // "Задержка" adds RDE (bits 0-2).
        assert_eq!(config_word(true, GeneratorMode::QuartzHigh), 0x003F);
        assert_eq!(config_word(true, GeneratorMode::Internal), 0x01C7);
    }

    #[test]
    fn program_block_is_big_endian_words() {
        let words = [
            0x1234u16, 0xABCD, 0xFFFF, 0x0001, 0x0000, 0x0000, 0x0000, 0x0000,
        ];
        // Simulate the first block: 8 words big-endian.
        let mut block = [0xFF; BLOCK_LEN];
        for (wi, &w) in words.iter().take(WORDS_PER_BLOCK).enumerate() {
            block[wi * 2] = (w >> 8) as u8;
            block[wi * 2 + 1] = (w & 0xFF) as u8;
        }
        assert_eq!(
            block,
            [
                0x12, 0x34, 0xAB, 0xCD, 0xFF, 0xFF, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00
            ]
        );
    }

    #[test]
    fn config_block_puts_word_big_endian_first() {
        let word = config_word(true, GeneratorMode::QuartzLow); // 0x01FF
        let mut block = [0xFF; BLOCK_LEN];
        block[0] = (word >> 8) as u8;
        block[1] = (word & 0xFF) as u8;
        assert_eq!(block[0], 0x01);
        assert_eq!(block[1], 0xFF);
    }
}
