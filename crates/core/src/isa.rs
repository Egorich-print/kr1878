//! Instruction set of the KR1878VE1 microcontroller.
//!
//! Every instruction is a fixed 16-bit word. Operand fields are 5-bit slots
//! encoding a segment (`A`/`B`/`C`/`D`) plus a byte index 0..7 (see [`Operand`]).

pub mod operand;

pub use operand::{Operand, Segment};

use crate::error::Result;

/// Which tetrad (nibble) a 4-bit constant occupies when forming an 8-bit mask.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// Low tetrad (bits 3..0). Mnemonics `BICL/BISL/BTGL/BTTL`.
    Low,
    /// High tetrad (bits 7..4). Mnemonics `BICH/BISH/BTGH/BTTH`.
    High,
}

/// A fully-decoded KR1878VE1 instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Instruction {
    // ---- two-operand: opcode(6) | src(5) | dst(5) ----
    Mov { dst: Operand, src: Operand },
    Cmp { dst: Operand, src: Operand },
    Sub { dst: Operand, src: Operand },
    Add { dst: Operand, src: Operand },
    And { dst: Operand, src: Operand },
    Or  { dst: Operand, src: Operand },
    Xor { dst: Operand, src: Operand },

    // ---- literal ----
    Movl { dst: Operand, value: u8 }, // 8-bit
    Cmpl { dst: Operand, value: u8 }, // 8-bit
    Addl { dst: Operand, value: u8 }, // 5-bit (0..=31)
    Subl { dst: Operand, value: u8 }, // 5-bit (0..=31)
    Bic  { dst: Operand, tetrad: u8, place: Place },
    Bis  { dst: Operand, tetrad: u8, place: Place },
    Btg  { dst: Operand, tetrad: u8, place: Place },
    Btt  { dst: Operand, tetrad: u8, place: Place },

    // ---- one-operand ----
    Swap(Operand),
    Neg(Operand),
    Not(Operand),
    Shl(Operand),
    Shr(Operand),
    Shra(Operand),
    Rlc(Operand),
    Rrc(Operand),
    Adc(Operand),
    Sbc(Operand),

    // ---- service registers & processor status ----
    Ldr { reg: u8, value: u8 }, // reg 0..=7, value 8-bit
    Mfpr { dst: Operand, reg: u8 },
    Mtpr { reg: u8, src: Operand },
    Push { reg: u8 },
    Pop  { reg: u8 },
    Sst  { mask: u8 }, // 4-bit
    Cst  { mask: u8 }, // 4-bit
    Tof,
    Tdc,

    // ---- control transfer ----
    Jmp  { addr: u16 }, // 10-bit absolute
    Jsr  { addr: u16 },
    Jz   { addr: u16 },
    Jnz  { addr: u16 },
    Jns  { addr: u16 },
    Js   { addr: u16 },
    Jnc  { addr: u16 },
    Jc   { addr: u16 },
    Ijmp,
    Ijsr,
    Rts,
    Rti,
    /// Return with carry set to `carry`.
    Rtsc { carry: bool },

    // ---- special ----
    Nop,
    Wait,
    Reset,
    Sksp,
    Stop,
}

/// Opcode constants (see `docs/REVERSE_ENGINEERING.md` §5).
mod op {
    pub const MOV: u16 = 0b000001;
    pub const CMP: u16 = 0b000010;
    pub const SUB: u16 = 0b000011;
    pub const ADD: u16 = 0b000100;
    pub const AND: u16 = 0b000101;
    pub const OR:  u16 = 0b000110;
    pub const XOR: u16 = 0b000111;

    pub const BIC: u16 = 0b001010;
    pub const SUBL: u16 = 0b001011;
    pub const ADDL: u16 = 0b001100;
    pub const BTT: u16 = 0b001101;
    pub const BIS: u16 = 0b001110;
    pub const BTG: u16 = 0b001111;

    pub const LDR: u16 = 0b00100;

    pub const MTPR: u16 = 0b00000010;
    pub const MFPR: u16 = 0b00000011;

    pub const SSTS: u16 = 0b000000011000;
    pub const CSTS: u16 = 0b000000011100;
}

/// Fixed 16-bit machine words of no/argument instructions.
pub mod fixed_word {
    pub const NOP: u16 = 0x0000;
    pub const WAIT: u16 = 0x0001;
    pub const RESET: u16 = 0x0002;
    pub const IJMP: u16 = 0x0003;
    pub const TOF: u16 = 0x0004;
    pub const TDC: u16 = 0x0005;
    pub const SKSP: u16 = 0x0006;
    pub const IJSR: u16 = 0x0007;
    pub const STOP: u16 = 0x0008;
    pub const RTS: u16 = 0x000C;
    pub const RTI: u16 = 0x000D;
}

impl Instruction {
/// Encode `self` into its 16-bit machine word.
    pub fn encode(&self) -> Result<u16> {
        use Instruction::*;
        let d = |x: Operand| x.to_u8() as u16;

        Ok(match *self {
            Mov { dst, src } => two_op(op::MOV, d(dst), d(src)),
            Cmp { dst, src } => two_op(op::CMP, d(dst), d(src)),
            Sub { dst, src } => two_op(op::SUB, d(dst), d(src)),
            Add { dst, src } => two_op(op::ADD, d(dst), d(src)),
            And { dst, src } => two_op(op::AND, d(dst), d(src)),
            Or { dst, src } => two_op(op::OR, d(dst), d(src)),
            Xor { dst, src } => two_op(op::XOR, d(dst), d(src)),

            Movl { dst, value } => (0b010 << 13) | ((value as u16 & 0xFF) << 5) | d(dst),
            Cmpl { dst, value } => (0b011 << 13) | ((value as u16 & 0xFF) << 5) | d(dst),

            Addl { dst, value } => two::short(op::ADDL, value, d(dst)),
            Subl { dst, value } => two::short(op::SUBL, value, d(dst)),

            Bic { dst, tetrad, place } => tetrad_op(op::BIC, tetrad, place, d(dst)),
            Bis { dst, tetrad, place } => tetrad_op(op::BIS, tetrad, place, d(dst)),
            Btg { dst, tetrad, place } => tetrad_op(op::BTG, tetrad, place, d(dst)),
            Btt { dst, tetrad, place } => tetrad_op(op::BTT, tetrad, place, d(dst)),

            Swap(o) => one_op(0b00000000001, d(o)),
            Neg(o) => one_op(0b00000000010, d(o)),
            Not(o) => one_op(0b00000000011, d(o)),
            Shl(o) => one_op(0b00000000100, d(o)),
            Shr(o) => one_op(0b00000000101, d(o)),
            Shra(o) => one_op(0b00000000110, d(o)),
            Rlc(o) => one_op(0b00000000111, d(o)),
            Rrc(o) => one_op(0b00000001000, d(o)),
            Adc(o) => one_op(0b00000001001, d(o)),
            Sbc(o) => one_op(0b00000001010, d(o)),

            Ldr { reg, value } => (op::LDR << 11) | ((value as u16 & 0xFF) << 3) | (reg as u16 & 0x7),
            Mfpr { dst, reg } => (op::MFPR << 8) | ((reg as u16 & 0x7) << 5) | d(dst),
            Mtpr { reg, src } => (op::MTPR << 8) | ((reg as u16 & 0x7) << 5) | d(src),
            Push { reg } => (0b0000000000010 << 3) | (reg as u16 & 0x7),
            Pop { reg } => (0b0000000000011 << 3) | (reg as u16 & 0x7),
            Sst { mask } => (op::SSTS << 4) | (mask as u16 & 0x0F),
            Cst { mask } => (op::CSTS << 4) | (mask as u16 & 0x0F),
            Tof => fixed_word::TOF,
            Tdc => fixed_word::TDC,

            Jmp { addr } => branch(0b1000, addr),
            Jsr { addr } => branch(0b1001, addr),
            Jz { addr } => branch(0b1010, addr),
            Jnz { addr } => branch(0b1011, addr),
            Jns { addr } => branch(0b1100, addr),
            Js { addr } => branch(0b1101, addr),
            Jnc { addr } => branch(0b1110, addr),
            Jc { addr } => branch(0b1111, addr),
            Ijmp => fixed_word::IJMP,
            Ijsr => fixed_word::IJSR,
            Rts => fixed_word::RTS,
            Rti => fixed_word::RTI,
            Rtsc { carry } => 0x000E | (carry as u16),

            Nop => fixed_word::NOP,
            Wait => fixed_word::WAIT,
            Reset => fixed_word::RESET,
            Sksp => fixed_word::SKSP,
            Stop => fixed_word::STOP,
        })
    }
}

/// opcode(6) | src(5) | dst(5)
fn two_op(opcode: u16, dst: u16, src: u16) -> u16 {
    ((opcode & 0x3F) << 10) | (src << 5) | (dst & 0x1F)
}

/// literal-with-5-bit-slot: opcode(6) | value(5) | dst(5)
mod two {
    pub fn short(op: u16, value: u8, dst: u16) -> u16 {
        ((op & 0x3F) << 10) | ((value as u16 & 0x1F) << 5) | (dst & 0x1F)
    }
}

/// opcode(6) | p(1) | tetrad(4) | dst(5)
fn tetrad_op(opcode: u16, tetrad: u8, place: Place, dst: u16) -> u16 {
    let place_bit = match place {
        Place::High => 1 << 9,
        Place::Low => 0,
    };
    ((opcode & 0x3F) << 10)
        | place_bit
        | ((tetrad as u16 & 0x0F) << 5)
        | (dst & 0x1F)
}

/// opcode(11) | dst(5)
fn one_op(opcode11: u16, dst: u16) -> u16 {
    ((opcode11 & 0x07FF) << 5) | (dst & 0x1F)
}

/// opcode(6) | addr(10)
fn branch(opcode: u16, addr: u16) -> u16 {
    ((opcode & 0x3F) << 10) | (addr & 0x03FF)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn op_of(w: u16) -> u16 {
        w >> 10
    }

    #[test]
    fn mov_encoding() {
        let dst = Operand::new(Segment::D, 0);
        let src = Operand::new(Segment::A, 5);
        let w = Instruction::Mov { dst, src }.encode().unwrap();
        assert_eq!(w >> 10, op::MOV);
        assert_eq!((w >> 5) & 0x1F, src.to_u8() as u16);
        assert_eq!(w & 0x1F, dst.to_u8() as u16);
    }

    #[test]
    fn movl_eight_bit() {
        let dst = Operand::new(Segment::B, 2);
        let w = Instruction::Movl { dst, value: 0xFF }.encode().unwrap();
        assert_eq!(w >> 13, 0b010);
        assert_eq!((w >> 5) & 0xFF, 0xFF);
        assert_eq!(w & 0x1F, dst.to_u8() as u16);
    }

    #[test]
    fn one_operand_encodes_dst_slot() {
        let dst = Operand::new(Segment::D, 7);
        let w = Instruction::Sbc(dst).encode().unwrap();
        assert_eq!(w >> 5, 0b00000001010);
        assert_eq!(w & 0x1F, dst.to_u8() as u16);
    }

    #[test]
    fn branch_ten_bit_address() {
        let w = Instruction::Jmp { addr: 0x2FF }.encode().unwrap();
        assert_eq!(op_of(w), 0b1000);
        assert_eq!(w & 0x03FF, 0x2FF);
    }

    #[test]
    fn fixed_words() {
        assert_eq!(Instruction::Nop.encode().unwrap(), 0x0000);
        assert_eq!(Instruction::Wait.encode().unwrap(), 0x0001);
        assert_eq!(Instruction::Rts.encode().unwrap(), 0x000C);
        assert_eq!(Instruction::Rti.encode().unwrap(), 0x000D);
        assert_eq!(Instruction::Tof.encode().unwrap(), 0x0004);
    }

    #[test]
    fn bic_place_bits() {
        let dst = Operand::new(Segment::D, 7);
        let lo = Instruction::Bic { dst, tetrad: 0x9, place: Place::Low }.encode().unwrap();
        let hi = Instruction::Bic { dst, tetrad: 0x9, place: Place::High }.encode().unwrap();
        assert_eq!(op_of(lo), op::BIC);
        assert_eq!(lo & 0x0200, 0);
        assert_eq!(hi & 0x0200, 0x0200);
        assert_eq!((hi >> 5) & 0x0F, 9);
    }
}