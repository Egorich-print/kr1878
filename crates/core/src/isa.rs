//! Instruction set of the KR1878VE1 microcontroller.
//!
//! Every instruction is a fixed 16-bit word. Operand fields are 5-bit slots
//! encoding a segment (`A`/`B`/`C`/`D`) plus a byte index 0..7 (see [`Operand`]).

pub mod operand;

pub use operand::{Operand, Segment};

use crate::error::{Error, Result};

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
    Mov {
        dst: Operand,
        src: Operand,
    },
    Cmp {
        dst: Operand,
        src: Operand,
    },
    Sub {
        dst: Operand,
        src: Operand,
    },
    Add {
        dst: Operand,
        src: Operand,
    },
    And {
        dst: Operand,
        src: Operand,
    },
    Or {
        dst: Operand,
        src: Operand,
    },
    Xor {
        dst: Operand,
        src: Operand,
    },

    // ---- literal ----
    Movl {
        dst: Operand,
        value: u8,
    }, // 8-bit
    Cmpl {
        dst: Operand,
        value: u8,
    }, // 8-bit
    Addl {
        dst: Operand,
        value: u8,
    }, // 5-bit (0..=31)
    Subl {
        dst: Operand,
        value: u8,
    }, // 5-bit (0..=31)
    Bic {
        dst: Operand,
        tetrad: u8,
        place: Place,
    },
    Bis {
        dst: Operand,
        tetrad: u8,
        place: Place,
    },
    Btg {
        dst: Operand,
        tetrad: u8,
        place: Place,
    },
    Btt {
        dst: Operand,
        tetrad: u8,
        place: Place,
    },

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
    Ldr {
        reg: u8,
        value: u8,
    }, // reg 0..=7, value 8-bit
    Mfpr {
        dst: Operand,
        reg: u8,
    },
    Mtpr {
        reg: u8,
        src: Operand,
    },
    Push {
        reg: u8,
    },
    Pop {
        reg: u8,
    },
    Sst {
        mask: u8,
    }, // 4-bit
    Cst {
        mask: u8,
    }, // 4-bit
    Tof,
    Tdc,

    // ---- control transfer ----
    Jmp {
        addr: u16,
    }, // 10-bit absolute
    Jsr {
        addr: u16,
    },
    Jz {
        addr: u16,
    },
    Jnz {
        addr: u16,
    },
    Jns {
        addr: u16,
    },
    Js {
        addr: u16,
    },
    Jnc {
        addr: u16,
    },
    Jc {
        addr: u16,
    },
    Ijmp,
    Ijsr,
    Rts,
    Rti,
    /// Return with carry set to `carry`.
    Rtsc {
        carry: bool,
    },

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
    pub const OR: u16 = 0b000110;
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

            Ldr { reg, value } => {
                (op::LDR << 11) | ((value as u16 & 0xFF) << 3) | (reg as u16 & 0x7)
            }
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

    /// Decode a 16-bit machine word back into an [`Instruction`]. The inverse
    /// of [`Instruction::encode`]; the exact opcode map mirrors `encode` so a
    /// round-trip is lossless.
    pub fn decode(word: u16) -> Result<Instruction> {
        use Instruction::*;
        let op6 = (word >> 10) as u8 & 0x3F;
        let op11 = word >> 5 & 0x07FF;
        let dst = Operand::from_u8((word & 0x1F) as u8);
        let op = |x: u8| Operand::from_u8(x);

        // ---- fixed words (exact) ----
        match word {
            w if w == fixed_word::NOP => return Ok(Nop),
            w if w == fixed_word::WAIT => return Ok(Wait),
            w if w == fixed_word::RESET => return Ok(Reset),
            w if w == fixed_word::IJMP => return Ok(Ijmp),
            w if w == fixed_word::TOF => return Ok(Tof),
            w if w == fixed_word::TDC => return Ok(Tdc),
            w if w == fixed_word::SKSP => return Ok(Sksp),
            w if w == fixed_word::IJSR => return Ok(Ijsr),
            w if w == fixed_word::STOP => return Ok(Stop),
            w if w == fixed_word::RTS => return Ok(Rts),
            w if w == fixed_word::RTI => return Ok(Rti),
            w if w == 0x000E || w == 0x000F => return Ok(Rtsc { carry: w & 1 == 1 }),
            _ => {}
        }

        // ---- service-register / RS group (checked before one-op, which it
        //      overlaps in the high opcode space) ----
        if word >> 11 == 0b00100 {
            // ldr: const(8)<<3 | reg(3)
            let value = ((word >> 3) & 0xFF) as u8;
            let reg = (word & 0x7) as u8;
            return Ok(Ldr { reg, value });
        }
        match word >> 8 {
            0b00000010 => {
                let reg = ((word >> 5) & 0x7) as u8;
                return Ok(Mtpr {
                    reg,
                    src: op((word & 0x1F) as u8),
                });
            }
            0b00000011 => {
                let reg = ((word >> 5) & 0x7) as u8;
                return Ok(Mfpr {
                    dst: op((word & 0x1F) as u8),
                    reg,
                });
            }
            _ => {}
        }
        match op11 {
            0b00000000001 => return Ok(Swap(dst)),
            0b00000000010 => return Ok(Neg(dst)),
            0b00000000011 => return Ok(Not(dst)),
            0b00000000100 => return Ok(Shl(dst)),
            0b00000000101 => return Ok(Shr(dst)),
            0b00000000110 => return Ok(Shra(dst)),
            0b00000000111 => return Ok(Rlc(dst)),
            0b00000001000 => return Ok(Rrc(dst)),
            0b00000001001 => return Ok(Adc(dst)),
            0b00000001010 => return Ok(Sbc(dst)),
            _ => {}
        }

        // ---- push/pop: `0000000000010/3` in bits 6..3 (reg in 2..0) ----
        match word >> 3 {
            0b0000000000010 => return Ok(Push { reg: (word & 0x7) as u8 }),
            0b0000000000011 => return Ok(Pop { reg: (word & 0x7) as u8 }),
            _ => {}
        }
        // ---- sst/cst: `000000011000/100` in bits 15..4 (mask in 3..0) ----
        match word >> 4 {
            0b000000011000 => return Ok(Sst { mask: (word & 0x0F) as u8 }),
            0b000000011100 => return Ok(Cst { mask: (word & 0x0F) as u8 }),
            _ => {}
        }

        // ---- branch (4-bit opcode in bits 15..12, 12-bit address) ----
        let op4 = (word >> 12) as u8 & 0x0F;
        let addr = word & 0x0FFF;
        match op4 {
            0b1000 => return Ok(Jmp { addr }),
            0b1001 => return Ok(Jsr { addr }),
            0b1010 => return Ok(Jz { addr }),
            0b1011 => return Ok(Jnz { addr }),
            0b1100 => return Ok(Jns { addr }),
            0b1101 => return Ok(Js { addr }),
            0b1110 => return Ok(Jnc { addr }),
            0b1111 => return Ok(Jc { addr }),
            _ => {}
        }

        // ---- two-operand data ops (6-bit opcode) ----
        let two_op_match = |c: u8| -> Option<(Operand, Operand)> {
            if op6 == c {
                let src = op(((word >> 5) & 0x1F) as u8);
                Some((dst, src))
            } else {
                None
            }
        };
        if let Some((d, s)) = two_op_match(op::MOV as u8) {
            return Ok(Mov { dst: d, src: s });
        }
        if let Some((d, s)) = two_op_match(op::CMP as u8) {
            return Ok(Cmp { dst: d, src: s });
        }
        if let Some((d, s)) = two_op_match(op::SUB as u8) {
            return Ok(Sub { dst: d, src: s });
        }
        if let Some((d, s)) = two_op_match(op::ADD as u8) {
            return Ok(Add { dst: d, src: s });
        }
        if let Some((d, s)) = two_op_match(op::AND as u8) {
            return Ok(And { dst: d, src: s });
        }
        if let Some((d, s)) = two_op_match(op::OR as u8) {
            return Ok(Or { dst: d, src: s });
        }
        if let Some((d, s)) = two_op_match(op::XOR as u8) {
            return Ok(Xor { dst: d, src: s });
        }

        // ---- 5-bit literal ops ----
        let lit5 = ((word >> 5) & 0x1F) as u8;
        if op6 == op::ADDL as u8 {
            return Ok(Addl { dst, value: lit5 });
        }
        if op6 == op::SUBL as u8 {
            return Ok(Subl { dst, value: lit5 });
        }

        // ---- tetrad ops: opcode(6) | p(1) | tetrad(4) | dst(5) ----
        let tetrad = ((word >> 5) & 0x0F) as u8;
        let place = if word & 0x0200 != 0 {
            Place::High
        } else {
            Place::Low
        };
        if op6 == op::BIC as u8 {
            return Ok(Bic { dst, tetrad, place });
        }
        if op6 == op::BIS as u8 {
            return Ok(Bis { dst, tetrad, place });
        }
        if op6 == op::BTG as u8 {
            return Ok(Btg { dst, tetrad, place });
        }
        if op6 == op::BTT as u8 {
            return Ok(Btt { dst, tetrad, place });
        }

        // ---- 8-bit literal ops (3-bit opcode in bits 15..13) ----
        let lit8 = ((word >> 5) & 0xFF) as u8;
        match word >> 13 {
            0b010 => return Ok(Movl { dst, value: lit8 }),
            0b011 => return Ok(Cmpl { dst, value: lit8 }),
            _ => {}
        }

        Err(Error::InvalidOpcode(word))
    }
}/// opcode(6) | src(5) | dst(5)
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
    ((opcode & 0x3F) << 10) | place_bit | ((tetrad as u16 & 0x0F) << 5) | (dst & 0x1F)
}

/// opcode(11) | dst(5)
fn one_op(opcode11: u16, dst: u16) -> u16 {
    ((opcode11 & 0x07FF) << 5) | (dst & 0x1F)
}

/// opcode(4) | addr(12) — branch instructions use a 4-bit opcode in bits 15..12
/// and a 12-bit absolute address (datasheet: `JMP 1000 address`, etc.).
fn branch(opcode: u16, addr: u16) -> u16 {
    ((opcode & 0x0F) << 12) | (addr & 0x0FFF)
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
        // Branch opcode lives in bits 15..12; the address is 12 bits wide.
        let w = Instruction::Jmp { addr: 0x8FF }.encode().unwrap();
        assert_eq!(w >> 12, 0b1000);
        assert_eq!(w & 0x0FFF, 0x8FF);
        // ADDL (6-bit opcode 0b001100) must NOT collide with JNS (0b1100 in bits 15..12).
        let addl = Instruction::Addl {
            dst: Operand::new(Segment::D, 0),
            value: 31,
        }
        .encode()
        .unwrap();
        let jns = Instruction::Jns { addr: 0x1FF }.encode().unwrap();
        assert_ne!(addl, jns);
        assert_eq!(addl >> 10, 0b001100);
        assert_eq!(jns >> 12, 0b1100);
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
        let lo = Instruction::Bic {
            dst,
            tetrad: 0x9,
            place: Place::Low,
        }
        .encode()
        .unwrap();
        let hi = Instruction::Bic {
            dst,
            tetrad: 0x9,
            place: Place::High,
        }
        .encode()
        .unwrap();
        assert_eq!(op_of(lo), op::BIC);
        assert_eq!(lo & 0x0200, 0);
        assert_eq!(hi & 0x0200, 0x0200);
        assert_eq!((hi >> 5) & 0x0F, 9);
    }

    fn all_instructions() -> Vec<Instruction> {
        use Instruction::*;
        let d0 = Operand::new(Segment::D, 0);
        let a5 = Operand::new(Segment::A, 5);
        let b3 = Operand::new(Segment::B, 3);
        vec![
            Mov { dst: d0, src: a5 },
            Cmp { dst: d0, src: a5 },
            Sub { dst: d0, src: a5 },
            Add { dst: d0, src: a5 },
            And { dst: d0, src: a5 },
            Or { dst: d0, src: a5 },
            Xor { dst: d0, src: a5 },
            Movl { dst: d0, value: 0xFF },
            Cmpl { dst: d0, value: 0x55 },
            Addl { dst: d0, value: 31 },
            Subl { dst: d0, value: 1 },
            Bic { dst: d0, tetrad: 0xF, place: Place::Low },
            Bic { dst: d0, tetrad: 0xF, place: Place::High },
            Bis { dst: d0, tetrad: 0x5, place: Place::Low },
            Btg { dst: d0, tetrad: 0x3, place: Place::High },
            Btt { dst: d0, tetrad: 0xA, place: Place::Low },
            Swap(d0),
            Neg(d0),
            Not(d0),
            Shl(d0),
            Shr(d0),
            Shra(d0),
            Rlc(d0),
            Rrc(d0),
            Adc(d0),
            Sbc(d0),
            Ldr { reg: 7, value: 0x42 },
            Mfpr { dst: d0, reg: 3 },
            Mtpr { reg: 6, src: a5 },
            Push { reg: 2 },
            Pop { reg: 4 },
            Sst { mask: 0xB },
            Cst { mask: 0x9 },
            Tof,
            Tdc,
            Jmp { addr: 0x2FF },
            Jsr { addr: 0x123 },
            Jz { addr: 0x0AA },
            Jnz { addr: 0x1BB },
            Jns { addr: 0x2CC },
            Js { addr: 0x3DD },
            Jnc { addr: 0x0EE },
            Jc { addr: 0x1FF },
            Ijmp,
            Ijsr,
            Rts,
            Rti,
            Rtsc { carry: false },
            Rtsc { carry: true },
            Nop,
            Wait,
            Reset,
            Sksp,
            Stop,
        ]
    }

    #[test]
    fn decode_roundtrips_all_instructions() {
        for ins in all_instructions() {
            let w = ins.encode().unwrap();
            let back = Instruction::decode(w).unwrap();
            assert_eq!(back, ins, "round-trip failed for {ins:?} (word {w:04X})");
        }
    }
}
