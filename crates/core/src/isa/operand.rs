//! Operand encoding: a 5-bit slot selecting a segment window plus an 8-byte index.

use crate::error::{Error, Result};

/// One of the four data-memory segment windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Segment {
    A,
    B,
    C,
    D,
}

impl Segment {
    /// 2-bit segment selector used in the operand field.
    pub const fn sel(self) -> u8 {
        match self {
            Segment::A => 0,
            Segment::B => 1,
            Segment::C => 2,
            Segment::D => 3,
        }
    }

    /// Parse from the assembler letter (`a`/`b`/`c`/`d`).
    pub fn from_letter(c: char) -> Option<Segment> {
        match c.to_ascii_lowercase() {
            'a' => Some(Segment::A),
            'b' => Some(Segment::B),
            'c' => Some(Segment::C),
            'd' => Some(Segment::D),
            _ => None,
        }
    }
}

/// A 5-bit operand slot = segment(2 bits) + index(3 bits, 0..=7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Operand {
    segment: Segment,
    index: u8,
}

impl Operand {
    /// Create an operand; `index` must be in 0..=7.
    pub const fn new(segment: Segment, index: u8) -> Self {
        Operand {
            segment,
            index: index & 0x07,
        }
    }

    /// Build from a raw 5-bit field value.
    pub const fn from_u8(v: u8) -> Self {
        let index = v & 0x07;
        let segment = match (v >> 3) & 0x03 {
            0 => Segment::A,
            1 => Segment::B,
            2 => Segment::C,
            _ => Segment::D,
        };
        Operand { segment, index }
    }

    pub const fn segment(self) -> Segment {
        self.segment
    }

    pub const fn index(self) -> u8 {
        self.index
    }

    /// The 5-bit machine encoding of this operand.
    pub const fn to_u8(self) -> u8 {
        (self.segment.sel() << 3) | self.index
    }

    /// Render in the textual (assembler-listing) form, e.g. `%a5`.
    pub fn to_text(self) -> String {
        let seg = match self.segment {
            Segment::A => 'a',
            Segment::B => 'b',
            Segment::C => 'c',
            Segment::D => 'd',
        };
        format!("%{seg}{}", self.index)
    }

    /// Parse the textual form `a5`, `%b3`, `d7`, case-insensitive.
    pub fn parse_text(s: &str) -> Result<Operand> {
        let s = s.trim().strip_prefix('%').unwrap_or(s.trim());
        let mut chars = s.chars();
        let seg_char = chars
            .next()
            .ok_or_else(|| Error::Other("empty operand".into()))?;
        let segment = Segment::from_letter(seg_char)
            .ok_or_else(|| Error::Other("bad segment letter".into()))?;
        let index: u8 = chars
            .collect::<String>()
            .parse()
            .map_err(|_| Error::Other("bad operand index".into()))?;
        if index > 7 {
            return Err(Error::Other("operand index out of range".into()));
        }
        Ok(Operand::new(segment, index))
    }
}
