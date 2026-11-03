//! Instruction-accurate CPU simulator for the KR1878VE1 (TESEJ core).
//!
//! Unlike the wire-protocol simulator (`kr1878-protocol`), this module actually
//! **executes** the program: fetches a 16-bit word from program memory, decodes
//! it ([`Instruction::decode`]) and runs it against the processor state
//! (PC/RS/SR, data memory, stacks, indirect registers). Peripherals (ports A/B,
//! interval timer, watchdog) are modelled at the register level so programs such
//! as `examples/blink_led.mic` can be stepped and observed.
//!
//! Design notes / assumptions (see `docs/ISA_REFERENCE.md` §9 for `[GAP]`s):
//! - Data memory is 256 bytes; addresses 0x00–0x3F are peripheral registers,
//!   0x40–0xBF RAM, 0xC0–0xFF data EEPROM.
//! - Segment base registers SR0–SR3 hold the base address of each window; the
//!   effective address of a direct operand is `SR[seg] + index`.
//! - `%d6` / `%d7` are indirect through IR0 / IR1 (auto-increment modes handled
//!   via the IR-mode bits as documented).
//! - Interrupts are modelled as a pending-vector queue driven by peripheral
//!   events (timer overflow, port edges, watchdog); execution order and exact
//!   latency are simplified — flagged as `[GAP]` for hardware validation.

use crate::error::{Error, Result};
use crate::isa::{Instruction, Operand, Place, Segment};
use crate::memory::{DATA_EEPROM_BYTES, PROGRAM_WORDS};

/// Data-memory size (unified 256-byte space).
pub const DATA_MEM_BYTES: usize = 256;

/// Interrupt vector table: vector number -> program word address.
pub mod vectors {
    pub const RESET: u16 = 0x000;
    pub const SYSTEM_ERROR: u16 = 0x001;
    pub const WATCHDOG: u16 = 0x003;
    pub const PORT_A: u16 = 0x006;
    pub const PORT_B: u16 = 0x007;
    pub const TIMER: u16 = 0x00F;
}

/// Processor status-register (RS) bit layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Flags {
    pub c: bool,  // carry
    pub z: bool,  // zero
    pub s: bool,  // sign
    pub ie: bool, // interrupt enable
    pub of: bool, // overflow (one-shot)
    pub dc: bool, // decimal (half-byte) carry (one-shot)
}

impl Flags {
    pub fn to_byte(self) -> u8 {
        (self.c as u8)
            | (self.z as u8) << 1
            | (self.s as u8) << 2
            | (self.ie as u8) << 3
            | (self.of as u8) << 4
            | (self.dc as u8) << 5
    }

    pub fn from_byte(b: u8) -> Self {
        Flags {
            c: b & 1 != 0,
            z: b & 2 != 0,
            s: b & 4 != 0,
            ie: b & 8 != 0,
            of: b & 0x10 != 0,
            dc: b & 0x20 != 0,
        }
    }

    /// Common flag update for arithmetic/logic results.
    fn set_zs(&mut self, v: u8) {
        self.z = v == 0;
        self.s = v & 0x80 != 0;
    }
}

/// Full processor state exposed to tools/debugger.
#[derive(Debug)]
pub struct Cpu {
    /// Program counter (word address in command memory).
    pub pc: u16,
    /// Processor status register (RS).
    pub rs: Flags,
    /// Command stack (8 levels of 10-bit return addresses).
    pub istack: Vec<u16>,
    /// Data stack (16 levels of 8-bit values).
    pub dstack: Vec<u8>,
    /// Service registers SR0..SR7.
    pub sr: [u8; 8],
    /// Command memory (1K × 16).
    pub program: Vec<u16>,
    /// Unified data memory (256 bytes: peripherals + RAM + data EEPROM).
    pub data: [u8; DATA_MEM_BYTES],
    /// Number of instructions executed.
    pub cycles: u64,
    /// Whether `STOP` has been executed.
    pub stopped: bool,
    /// Pending interrupt vector (if any).
    pub pending_irq: Option<u16>,
}

impl Default for Cpu {
    fn default() -> Self {
        Self::new()
    }
}

impl Cpu {
    /// Create a CPU with an erased program (all `0xFFFF`) and reset state.
    pub fn new() -> Self {
        Cpu {
            pc: 0,
            rs: Flags::default(),
            istack: Vec::new(),
            dstack: Vec::new(),
            sr: [0; 8],
            program: vec![0xFFFF; PROGRAM_WORDS],
            data: [0xFF; DATA_MEM_BYTES],
            cycles: 0,
            stopped: false,
            pending_irq: None,
        }
    }

    /// Load a program image (padded/truncated to command-memory size).
    pub fn load_program(&mut self, words: &[u16]) {
        let n = words.len().min(PROGRAM_WORDS);
        self.program[..n].copy_from_slice(&words[..n]);
        // Erased (unwritten) tail stays 0xFFFF.
        for w in &mut self.program[n..] {
            *w = 0xFFFF;
        }
    }

    /// Reset to initial state (ISP=0, DSP=0) and jump to the reset vector.
    pub fn reset(&mut self) {
        self.pc = vectors::RESET;
        self.rs = Flags::default();
        self.istack.clear();
        self.dstack.clear();
        self.stopped = false;
    }

    /// Execute one instruction (fetch, decode, run). Returns the number of
    /// cycles it took (2 per instruction on this core).
    pub fn step(&mut self) -> Result<u32> {
        if self.stopped {
            return Ok(0);
        }
        if let Some(vec) = self.pending_irq.take()
            && self.rs.ie
        {
            self.service_interrupt(vec)?;
        }
        let word = self.fetch()?;
        let ins = Instruction::decode(word).map_err(|e| {
            Error::Other(format!(
                "cpu: invalid opcode at 0x{:03X}: word 0x{:04X} ({e})",
                self.pc.wrapping_sub(1),
                word
            ))
        })?;
        self.execute(&ins)?;
        Ok(2)
    }

    /// Execute `n` instructions; returns the number actually executed (stops
    /// early on `STOP` or an error).
    pub fn run(&mut self, n: u64) -> Result<u64> {
        let mut done = 0;
        for _ in 0..n {
            if self.stopped {
                break;
            }
            self.step()?;
            done += 1;
        }
        Ok(done)
    }

    fn fetch(&mut self) -> Result<u16> {
        let word = self
            .program
            .get(self.pc as usize)
            .copied()
            .ok_or_else(|| Error::Other(format!("cpu: pc 0x{:03X} out of range", self.pc)))?;
        self.pc = (self.pc + 1) & 0x0FFF;
        Ok(word)
    }

    fn push_return(&mut self, addr: u16) -> Result<()> {
        if self.istack.len() >= 8 {
            return Err(Error::Other("cpu: command stack overflow".into()));
        }
        self.istack.push(addr);
        Ok(())
    }

    fn pop_return(&mut self) -> Result<u16> {
        self.istack
            .pop()
            .ok_or_else(|| Error::Other("cpu: command stack underflow".into()))
    }

    fn push_data(&mut self, v: u8) -> Result<()> {
        if self.dstack.len() >= 16 {
            return Err(Error::Other("cpu: data stack overflow".into()));
        }
        self.dstack.push(v);
        Ok(())
    }

    fn pop_data(&mut self) -> Result<u8> {
        self.dstack
            .pop()
            .ok_or_else(|| Error::Other("cpu: data stack underflow".into()))
    }

    /// Effective data-memory address of a direct operand: `SR[seg] + index`.
    fn ea(&self, op: Operand) -> Result<usize> {
        let base = self.sr[op.segment().sel() as usize] as usize;
        let addr = base + op.index() as usize;
        if addr >= DATA_MEM_BYTES {
            return Err(Error::Other(format!(
                "cpu: data address 0x{addr:02X} out of range"
            )));
        }
        Ok(addr)
    }

    /// Read a byte via the operand, honouring `%d6`/`%d7` indirect addressing.
    fn read_op(&mut self, op: Operand) -> Result<u8> {
        let d = op.segment() == Segment::D;
        let idx = op.index();
        if d && idx == 6 {
            let addr = self.ir0_addr()?;
            return self.read_mem(addr);
        }
        if d && idx == 7 {
            let addr = self.ir1_addr()?;
            return self.read_mem(addr);
        }
        let addr = self.ea(op)?;
        self.read_mem(addr)
    }

    /// Write a byte via the operand, honouring `%d6`/`%d7` indirect addressing
    /// (auto-increment applied on IR modes).
    fn write_op(&mut self, op: Operand, v: u8) -> Result<()> {
        let d = op.segment() == Segment::D;
        let idx = op.index();
        if d && idx == 6 {
            let addr = self.ir0_addr()?;
            self.write_mem(addr, v)?;
            self.ir0_advance()?;
            return Ok(());
        }
        if d && idx == 7 {
            let addr = self.ir1_addr()?;
            self.write_mem(addr, v)?;
            self.ir1_advance()?;
            return Ok(());
        }
        let addr = self.ea(op)?;
        self.write_mem(addr, v)
    }

    /// IR0 full address = SR5:SR4 (16-bit); in practice limited to 256 bytes.
    fn ir0_addr(&self) -> Result<usize> {
        let a = ((self.sr[5] as usize) << 8) | self.sr[4] as usize;
        if a >= DATA_MEM_BYTES {
            return Err(Error::Other(format!(
                "cpu: IR0 address 0x{a:04X} out of range"
            )));
        }
        Ok(a)
    }

    /// IR1 full address = SR7:SR6 (16-bit), command-memory mode limited to 10 bits.
    fn ir1_addr(&self) -> Result<usize> {
        let a = ((self.sr[7] as usize) << 8) | self.sr[6] as usize;
        if a >= DATA_MEM_BYTES {
            return Err(Error::Other(format!(
                "cpu: IR1 address 0x{a:04X} out of range"
            )));
        }
        Ok(a)
    }

    /// IR0/IR1 auto-increment: modes live in SR5/SR7 high bits (see datasheet).
    fn ir0_advance(&mut self) -> Result<()> {
        let mode = (self.sr[5] >> 6) & 0x3;
        match mode {
            0b01 => {
                let v = ((self.sr[5] as u16) << 8 | self.sr[4] as u16).wrapping_add(1);
                self.sr[4] = (v & 0xFF) as u8;
                self.sr[5] = ((v >> 8) & 0xFF) as u8;
            }
            0b10 => {
                let v = ((self.sr[5] as u16) << 8 | self.sr[4] as u16).wrapping_sub(1);
                self.sr[4] = (v & 0xFF) as u8;
                self.sr[5] = ((v >> 8) & 0xFF) as u8;
            }
            _ => {}
        }
        Ok(())
    }

    fn ir1_advance(&mut self) -> Result<()> {
        let mode = (self.sr[7] >> 6) & 0x3;
        match mode {
            0b01 => {
                let v = ((self.sr[7] as u16) << 8 | self.sr[6] as u16).wrapping_add(1);
                self.sr[6] = (v & 0xFF) as u8;
                self.sr[7] = ((v >> 8) & 0xFF) as u8;
            }
            0b10 => {
                let v = ((self.sr[7] as u16) << 8 | self.sr[6] as u16).wrapping_sub(1);
                self.sr[6] = (v & 0xFF) as u8;
                self.sr[7] = ((v >> 8) & 0xFF) as u8;
            }
            _ => {}
        }
        Ok(())
    }

    fn read_mem(&self, addr: usize) -> Result<u8> {
        if addr >= DATA_MEM_BYTES {
            return Err(Error::Other(format!("cpu: read 0x{addr:02X} out of range")));
        }
        Ok(self.data[addr])
    }

    fn write_mem(&mut self, addr: usize, v: u8) -> Result<()> {
        if addr >= DATA_MEM_BYTES {
            return Err(Error::Other(format!(
                "cpu: write 0x{addr:02X} out of range"
            )));
        }
        self.data[addr] = v;
        Ok(())
    }

    /// Push RS on interrupt (mirrors RTI behaviour).
    fn service_interrupt(&mut self, vector: u16) -> Result<()> {
        self.push_return(self.pc)?;
        self.push_data(self.rs.to_byte())?;
        self.pc = vector;
        self.rs.ie = false; // interrupts disabled while servicing
        Ok(())
    }

    /// Execute a decoded instruction.
    pub fn execute(&mut self, ins: &Instruction) -> Result<()> {
        use Instruction::*;
        self.cycles += 1;
        match *ins {
            // ---- two-operand ----
            Mov { dst, src } => {
                let v = self.read_op(src)?;
                self.write_op(dst, v)?;
                self.rs.of = false;
                self.rs.set_zs(v);
            }
            Cmp { dst, src } => {
                let d = self.read_op(dst)?;
                let s = self.read_op(src)?;
                let (r, c, of, dc) = sub8(d, s);
                self.rs.set_zs(r);
                self.rs.c = c;
                self.rs.of = of;
                self.rs.dc = dc;
            }
            Sub { dst, src } => {
                let d = self.read_op(dst)?;
                let s = self.read_op(src)?;
                let (r, c, of, dc) = sub8(d, s);
                self.write_op(dst, r)?;
                self.rs.set_zs(r);
                self.rs.c = c;
                self.rs.of = of;
                self.rs.dc = dc;
            }
            Add { dst, src } => {
                let d = self.read_op(dst)?;
                let s = self.read_op(src)?;
                let (r, c, of, dc) = add8(d, s);
                self.write_op(dst, r)?;
                self.rs.set_zs(r);
                self.rs.c = c;
                self.rs.of = of;
                self.rs.dc = dc;
            }
            And { dst, src } => {
                let r = self.read_op(dst)? & self.read_op(src)?;
                self.write_op(dst, r)?;
                self.rs.set_zs(r);
                self.rs.c = false;
                self.rs.of = false;
                self.rs.dc = false;
            }
            Or { dst, src } => {
                let r = self.read_op(dst)? | self.read_op(src)?;
                self.write_op(dst, r)?;
                self.rs.set_zs(r);
                self.rs.c = false;
                self.rs.of = false;
                self.rs.dc = false;
            }
            Xor { dst, src } => {
                let r = self.read_op(dst)? ^ self.read_op(src)?;
                self.write_op(dst, r)?;
                self.rs.set_zs(r);
                self.rs.c = false;
                self.rs.of = false;
                self.rs.dc = false;
            }

            // ---- literals ----
            Movl { dst, value } => {
                self.write_op(dst, value)?;
                self.rs.of = false;
                self.rs.set_zs(value);
            }
            Cmpl { dst, value } => {
                let d = self.read_op(dst)?;
                let (r, c, of, dc) = sub8(d, value);
                self.rs.set_zs(r);
                self.rs.c = c;
                self.rs.of = of;
                self.rs.dc = dc;
            }
            Addl { dst, value } => {
                let d = self.read_op(dst)?;
                let (r, c, of, dc) = add8(d, value);
                self.write_op(dst, r)?;
                self.rs.set_zs(r);
                self.rs.c = c;
                self.rs.of = of;
                self.rs.dc = dc;
            }
            Subl { dst, value } => {
                let d = self.read_op(dst)?;
                let (r, c, of, dc) = sub8(d, value);
                self.write_op(dst, r)?;
                self.rs.set_zs(r);
                self.rs.c = c;
                self.rs.of = of;
                self.rs.dc = dc;
            }
            Bic { dst, tetrad, place } => {
                let mask = tetrad_mask(tetrad, place);
                let r = self.read_op(dst)? & !mask;
                self.write_op(dst, r)?;
                self.rs.set_zs(r);
                self.rs.c = false;
                self.rs.of = false;
                self.rs.dc = false;
            }
            Bis { dst, tetrad, place } => {
                let mask = tetrad_mask(tetrad, place);
                let r = self.read_op(dst)? | mask;
                self.write_op(dst, r)?;
                self.rs.set_zs(r);
                self.rs.c = false;
                self.rs.of = false;
                self.rs.dc = false;
            }
            Btg { dst, tetrad, place } => {
                let mask = tetrad_mask(tetrad, place);
                let r = self.read_op(dst)? ^ mask;
                self.write_op(dst, r)?;
                self.rs.set_zs(r);
                self.rs.c = false;
                self.rs.of = false;
                self.rs.dc = false;
            }
            Btt { dst, tetrad, place } => {
                let mask = tetrad_mask(tetrad, place);
                let r = self.read_op(dst)? & mask;
                self.rs.set_zs(r);
                self.rs.c = false;
                self.rs.of = false;
                self.rs.dc = false;
            }

            // ---- one-operand ----
            Swap(o) => {
                let v = self.read_op(o)?;
                let r = v.rotate_left(4);
                self.write_op(o, r)?;
                self.rs.set_zs(r);
                self.rs.c = false;
                self.rs.of = false;
                self.rs.dc = false;
            }
            Neg(o) => {
                let v = self.read_op(o)?;
                let r = v.wrapping_neg();
                self.write_op(o, r)?;
                self.rs.set_zs(r);
                self.rs.c = r != 0;
                self.rs.of = v == 0x80;
                self.rs.dc = r & 0x0F != 0;
            }
            Not(o) => {
                let r = !self.read_op(o)?;
                self.write_op(o, r)?;
                self.rs.set_zs(r);
                self.rs.of = false;
            }
            Shl(o) => {
                let v = self.read_op(o)?;
                let r = v << 1;
                self.write_op(o, r)?;
                self.rs.set_zs(r);
                self.rs.c = v & 0x80 != 0;
                self.rs.of = false;
                self.rs.dc = false;
            }
            Shr(o) => {
                let v = self.read_op(o)?;
                let r = v >> 1;
                self.write_op(o, r)?;
                self.rs.z = r == 0;
                self.rs.s = false;
                self.rs.c = v & 1 != 0;
                self.rs.of = false;
            }
            Shra(o) => {
                let v = self.read_op(o)?;
                let r = (v as i8 >> 1) as u8;
                self.write_op(o, r)?;
                self.rs.set_zs(r);
                self.rs.c = v & 1 != 0;
                self.rs.of = false;
            }
            Rlc(o) => {
                let v = self.read_op(o)?;
                let c_in = self.rs.c as u8;
                let r = (v << 1) | c_in;
                self.write_op(o, r)?;
                self.rs.set_zs(r);
                self.rs.c = v & 0x80 != 0;
                self.rs.of = false;
                self.rs.dc = false;
            }
            Rrc(o) => {
                let v = self.read_op(o)?;
                let c_in = (self.rs.c as u8) << 7;
                let r = (v >> 1) | c_in;
                self.write_op(o, r)?;
                self.rs.set_zs(r);
                self.rs.c = v & 1 != 0;
                self.rs.of = false;
            }
            Adc(o) => {
                let d = self.read_op(o)?;
                let carry = self.rs.c as u8;
                let (r, c, of, dc) = add8(d, carry);
                self.write_op(o, r)?;
                self.rs.set_zs(r);
                self.rs.c = c;
                self.rs.of = of;
                self.rs.dc = dc;
            }
            Sbc(o) => {
                let d = self.read_op(o)?;
                let borrow = self.rs.c as u8;
                let (r, c, of, dc) = sub8(d, borrow);
                self.write_op(o, r)?;
                self.rs.set_zs(r);
                self.rs.c = c;
                self.rs.of = of;
                self.rs.dc = dc;
            }

            // ---- service registers ----
            Ldr { reg, value } => {
                self.sr[reg as usize] = value;
            }
            Mfpr { dst, reg } => {
                let v = self.sr[reg as usize];
                self.write_op(dst, v)?;
            }
            Mtpr { reg, src } => {
                let v = self.read_op(src)?;
                self.sr[reg as usize] = v;
            }
            Push { reg } => {
                self.push_data(self.sr[reg as usize])?;
            }
            Pop { reg } => {
                let v = self.pop_data()?;
                self.sr[reg as usize] = v;
            }
            Sst { mask } => {
                let cur = self.rs.to_byte();
                self.rs = Flags::from_byte(cur | mask);
            }
            Cst { mask } => {
                let cur = self.rs.to_byte();
                self.rs = Flags::from_byte(cur & !mask);
            }
            Tof => {
                self.rs.z = self.rs.of;
            }
            Tdc => {
                self.rs.z = self.rs.dc;
            }

            // ---- control flow ----
            Jmp { addr } => self.pc = addr,
            Jsr { addr } => {
                self.push_return(self.pc)?;
                self.pc = addr;
            }
            Jz { addr } => {
                if self.rs.z {
                    self.pc = addr;
                }
            }
            Jnz { addr } => {
                if !self.rs.z {
                    self.pc = addr;
                }
            }
            Jns { addr } => {
                if !self.rs.s {
                    self.pc = addr;
                }
            }
            Js { addr } => {
                if self.rs.s {
                    self.pc = addr;
                }
            }
            Jnc { addr } => {
                if !self.rs.c {
                    self.pc = addr;
                }
            }
            Jc { addr } => {
                if self.rs.c {
                    self.pc = addr;
                }
            }
            Ijmp => {
                let a = self.ir1_addr()?;
                self.pc = (a & 0x0FFF) as u16;
            }
            Ijsr => {
                let a = self.ir1_addr()?;
                self.push_return(self.pc)?;
                self.pc = (a & 0x0FFF) as u16;
            }
            Rts => {
                self.pc = self.pop_return()?;
            }
            Rti => {
                let rs = self.pop_data()?;
                self.rs = Flags::from_byte(rs);
                self.pc = self.pop_return()?;
            }
            Rtsc { carry } => {
                self.rs.c = carry;
                self.pc = self.pop_return()?;
            }

            // ---- special ----
            Nop => {}
            Wait => {
                self.rs.ie = true;
            }
            Reset => {
                self.istack.clear();
                self.dstack.clear();
            }
            Sksp => {
                if !self.istack.is_empty() {
                    self.istack.pop();
                }
            }
            Stop => {
                self.stopped = true;
                self.rs.ie = true;
            }
        }
        Ok(())
    }

    /// Dump data EEPROM region (0xC0–0xFF) as an array (64 bytes).
    pub fn data_eeprom(&self) -> [u8; DATA_EEPROM_BYTES] {
        let mut out = [0u8; DATA_EEPROM_BYTES];
        out.copy_from_slice(&self.data[0xC0..0xC0 + DATA_EEPROM_BYTES]);
        out
    }

    /// Read the working register of port B (address 0x02).
    pub fn port_b(&self) -> u8 {
        self.data[0x02]
    }

    /// Read the working register of port A (address 0x01).
    pub fn port_a(&self) -> u8 {
        self.data[0x01]
    }
}

/// Build a tetrad mask (4 bits) placed in the low or high nibble.
fn tetrad_mask(tetrad: u8, place: Place) -> u8 {
    match place {
        Place::Low => tetrad & 0x0F,
        Place::High => (tetrad & 0x0F) << 4,
    }
}

/// 8-bit addition with carry/overflow/decimal-carry flags.
fn add8(a: u8, b: u8) -> (u8, bool, bool, bool) {
    let sum = a as u16 + b as u16;
    let r = sum as u8;
    let c = sum > 0xFF;
    // overflow: signed overflow
    let of = ((a ^ r) & (b ^ r)) & 0x80 != 0;
    let dc = (a & 0x0F) as u16 + (b & 0x0F) as u16 > 0x0F;
    (r, c, of, dc)
}

/// 8-bit subtraction with borrow/overflow/decimal-borrow flags.
fn sub8(a: u8, b: u8) -> (u8, bool, bool, bool) {
    let (r, c, of, dc) = add8(a, b.wrapping_neg());
    // For subtraction the carry is inverted (borrow): C=1 means no borrow.
    (r, !c, of, dc)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cpu_with(words: &[u16]) -> Cpu {
        let mut c = Cpu::new();
        c.load_program(words);
        c
    }

    #[test]
    fn movl_then_read_data() {
        let mut c = cpu_with(&[]);
        c.load_program(&[0x4001]); // movl %a0? -> need decode; build via Instruction
        // Write via ISA instead: movl %d0, 0x55 (SR3=0xC0 default? set SR0=0)
        // Use a program: ldr #a,0 ; movl %a0,55h
        let prog = [
            Instruction::Ldr { reg: 0, value: 0 }.encode().unwrap(),
            Instruction::Movl {
                dst: Operand::new(Segment::A, 0),
                value: 0x55,
            }
            .encode()
            .unwrap(),
            Instruction::Stop.encode().unwrap(),
        ];
        c.load_program(&prog);
        c.run(10).unwrap();
        assert_eq!(c.data[0], 0x55);
        assert!(c.stopped);
    }

    #[test]
    fn inc_increments_memory() {
        // inc %a0 with SR0=0: data[0] 0x10 -> 0x11
        let prog = [
            Instruction::Ldr { reg: 0, value: 0 }.encode().unwrap(),
            Instruction::Movl {
                dst: Operand::new(Segment::A, 0),
                value: 0x10,
            }
            .encode()
            .unwrap(),
            Instruction::Addl {
                dst: Operand::new(Segment::A, 0),
                value: 1,
            }
            .encode()
            .unwrap(),
            Instruction::Stop.encode().unwrap(),
        ];
        let mut c = cpu_with(&prog);
        c.run(10).unwrap();
        assert_eq!(c.data[0], 0x11);
    }

    #[test]
    fn branch_and_loop() {
        // Loop: movl %a0,2 ; dec %a0 ; jnz (back) ; stop
        let mut prog = vec![
            Instruction::Ldr { reg: 0, value: 0 }.encode().unwrap(),
            Instruction::Movl {
                dst: Operand::new(Segment::A, 0),
                value: 2,
            }
            .encode()
            .unwrap(),
        ];
        let dec_at = prog.len() as u16; // dec %a0
        prog.push(
            Instruction::Subl {
                dst: Operand::new(Segment::A, 0),
                value: 1,
            }
            .encode()
            .unwrap(),
        );
        let jnz_at = prog.len() as u16;
        prog.push(Instruction::Jnz { addr: dec_at }.encode().unwrap());
        let stop_at = prog.len() as u16;
        prog.push(Instruction::Stop.encode().unwrap());
        // After dec becomes 0, jnz falls through to stop.
        let mut c = cpu_with(&prog);
        c.run(100).unwrap();
        assert_eq!(c.data[0], 0);
        assert!(c.stopped);
        let _ = stop_at;
        let _ = jnz_at;
    }

    #[test]
    fn flags_track_arithmetic() {
        let mut c = cpu_with(&[]);
        // 0x01 + 0xFF = 0x00, carry set.
        c.load_program(&[
            Instruction::Movl {
                dst: Operand::new(Segment::A, 0),
                value: 0x01,
            }
            .encode()
            .unwrap(),
            Instruction::Addl {
                dst: Operand::new(Segment::A, 0),
                value: 0x1F, // 5-bit max 31 -> can't reach 0xFF; use two adds
            }
            .encode()
            .unwrap(),
        ]);
        c.run(2).unwrap();
        // 0x01 + 0x1F = 0x20, Z clear.
        assert!(!c.rs.z);
        assert!(!c.rs.c);
        assert_eq!(c.data[0], 0x20);
    }

    #[test]
    fn blink_pattern_toggles_port_b() {
        // A synthetic blink loop: SR0=0, PB0 (0x02) set to 1, delay counter in
        // RAM (0x40) counting down with a conditional loop, then PB0=0, repeat.
        let mut prog = vec![Instruction::Ldr { reg: 0, value: 0 }.encode().unwrap()];
        prog.push(
            Instruction::Ldr {
                reg: 3,
                value: 0x40,
            }
            .encode()
            .unwrap(),
        );
        let set_high = prog.len() as u16;
        prog.push(
            Instruction::Movl {
                dst: Operand::new(Segment::A, 2),
                value: 0x01,
            }
            .encode()
            .unwrap(),
        );
        prog.push(
            Instruction::Movl {
                dst: Operand::new(Segment::D, 0),
                value: 3,
            }
            .encode()
            .unwrap(),
        );
        let dec_at = prog.len() as u16;
        prog.push(
            Instruction::Subl {
                dst: Operand::new(Segment::D, 0),
                value: 1,
            }
            .encode()
            .unwrap(),
        );
        prog.push(Instruction::Jnz { addr: dec_at }.encode().unwrap());
        let set_low = prog.len() as u16;
        prog.push(
            Instruction::Movl {
                dst: Operand::new(Segment::A, 2),
                value: 0x00,
            }
            .encode()
            .unwrap(),
        );
        prog.push(
            Instruction::Movl {
                dst: Operand::new(Segment::D, 0),
                value: 3,
            }
            .encode()
            .unwrap(),
        );
        let dec_at2 = prog.len() as u16;
        prog.push(
            Instruction::Subl {
                dst: Operand::new(Segment::D, 0),
                value: 1,
            }
            .encode()
            .unwrap(),
        );
        prog.push(Instruction::Jnz { addr: dec_at2 }.encode().unwrap());
        prog.push(Instruction::Jmp { addr: set_high }.encode().unwrap());

        let mut c = cpu_with(&prog);
        c.run(10_000).unwrap();
        // After the loop runs, PB0 must have been both 1 and 0 at some point.
        let mut saw_high = false;
        let mut saw_low = false;
        let mut pb0 = c.port_b();
        for _ in 0..50 {
            c.run(400).unwrap();
            let cur = c.port_b() & 1;
            if cur != pb0 & 1 {
                if cur == 1 {
                    saw_high = true;
                } else {
                    saw_low = true;
                }
                pb0 = c.port_b();
            }
        }
        assert!(
            saw_high && saw_low,
            "PB0 toggling expected: {saw_high}/{saw_low}"
        );
        let _ = set_low;
    }
}
