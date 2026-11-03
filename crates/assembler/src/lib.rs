//! The KR1878VE1 (TESSA) assembler: `.mic` source -> [`MemoryImage`] + listing + symbols.
//!
//! Pipeline:
//!   1. **Preprocess** — expand macros, `#define`, `#include`, handle `.if/.endif`.
//!   2. **Pass 1** — resolve labels/constants/scope, assign addresses (sizes).
//!   3. **Pass 2** — re-walk and encode, writing 16-bit words into program memory.
//!
//! Encodings mirror `docs/REVERSE_ENGINEERING.md` §5 via [`kr1878_core::isa`].

pub mod error;
pub mod expr;
pub mod lexer;
pub mod preprocess;
pub mod symbols;

pub use error::{AsmError, Result, Span};
pub use symbols::Symbols;

use kr1878_core::isa::{Instruction, Operand, Place, Segment};
use kr1878_core::memory::{MemoryImage, PROGRAM_WORDS};

use lexer::Token;
use preprocess::{Loader, Preprocessor, Stmt};

/// Output of an assembly run.
#[derive(Debug, Clone)]
pub struct AssembleOutput {
    pub image: MemoryImage,
    pub listing: String,
    pub symbols: Symbols,
}

// =====================================================================
// Public entry points
// =====================================================================

/// Assemble a single module (no `#include` loader).
pub fn assemble(source: &str) -> Result<AssembleOutput> {
    assemble_with_loader(source, None)
}

/// Assemble with an `#include` loader callback.
pub fn assemble_with_loader(source: &str, loader: Loader<'_>) -> Result<AssembleOutput> {
    let mut pp = Preprocessor::new(loader);
    let stmts = pp.run(source)?;

    let tx = |s: &Stmt| lexer::tokenize(&s.text, s.span);
    let toks: Vec<Vec<Token>> = stmts.iter().map(tx).collect::<Result<_>>()?;

    let mut syms = Symbols::new();

    // ---- Pass 1: symbols + address assignment (no emission).
    let mut pc: usize = 0;
    let mut has_endc = false;
    for st in stmts.iter().zip(toks.iter()) {
        let (_, npc) = process(st.0.span, st.1, &mut syms, pc, true, None, "")?;
        pc = npc;
        if let Ok((_, Class::Endc)) = classify(st.1, st.0.span) {
            has_endc = true;
        }
    }

    // ---- Pass 2: encode.
    let mut img = MemoryImage::new();
    let mut listing = String::new();
    pc = 0;
    for st in stmts.iter().zip(toks.iter()) {
        let (words, npc) = process(
            st.0.span,
            st.1,
            &mut syms,
            pc,
            false,
            Some(&mut img),
            &st.0.text,
        )?;
        for (off, w) in words.iter().enumerate() {
            listing.push_str(&format!("{:04x}: {:04x}  {}\n", pc + off, w, st.0.text));
        }
        pc = npc;
    }

    // ---- `.endc` 3-byte cumulative checksum over all emitted words.
    if has_endc {
        // `.endc` advanced pc by 2 (checksum body). Sum only the code actually
        // emitted before it (words [0, pc-2)), matching the official toolchain
        // which folds the program bytes with carry propagation.
        let code_len = pc.saturating_sub(2);
        let checksum = kr1878_core::crc::endc_checksum3(img.program[..code_len].iter().copied());
        // Write the three bytes into the program body at the `.endc` position;
        // the third byte lives in the low half of the following word (matching
        // `.byte` layout).
        let base = pc - 2;
        if base + 2 <= PROGRAM_WORDS {
            img.program[base] = (checksum[0] as u16) | ((checksum[1] as u16) << 8);
            if base + 1 < PROGRAM_WORDS {
                img.program[base + 1] = checksum[2] as u16;
            }
        }
    }

    Ok(AssembleOutput {
        image: img,
        listing,
        symbols: syms,
    })
}

/// Evaluate a constant condition (already define-replaced) as non-zero truth.
pub(crate) fn eval_cond(exp: &str) -> Result<i64> {
    let span = Span { file: 0, line: 0 };
    let toks = lexer::tokenize(exp, span)?;
    let clean = strip_eof(&toks);
    let ast = expr::Parser::new(&normalize_local_labels(&clean), span).parse()?;
    let r: &dyn Fn(&str) -> Option<i64> = &|_| None;
    ast.eval(&r, span)?
        .ok_or_else(|| AsmError::new(span, "undefined symbol in .if"))
}

// =====================================================================
// Page model
// =====================================================================

#[derive(Debug)]
enum Def {
    Label(String),
    Local(u32),
    Const { name: String, ex: expr::Expr },
}

#[derive(Debug)]
enum Class {
    /// Nothing emitted.
    Pass,
    /// A line that declared only label(s).
    Label,
    /// `.=expr`.
    SetPc { ex: expr::Expr },
    /// `.even`.
    Even,
    /// `.byte` / `.word`.
    Data { wide: bool, items: Vec<expr::Expr> },
    /// `.bpt` breakpoint.
    Bpt,
    /// `.endc` — end of translation + compute the 3-byte cumulative checksum.
    Endc,
    /// Program instruction.
    Insn { mnem: String, ops: Vec<Vec<Token>> },
}

/// Classify a statement, then (in one step) apply it to `pc`, optionally
/// defining symbols and/or emitting words. Returns the emitted words and the
/// new pc. `emitting` selects whether to write into `img`.
fn process(
    span: Span,
    tk: &[Token],
    syms: &mut Symbols,
    pc: usize,
    defining: bool,
    img: Option<&mut MemoryImage>,
    src: &str,
) -> Result<(Vec<u16>, usize)> {
    let (defs, class) = classify(tk, span)?;

    if defining {
        for d in &defs {
            match d {
                Def::Label(n) => syms.define_label(n, pc as u16, span.line)?,
                Def::Local(k) => {
                    syms.define_local(syms.current_scope(), *k, pc as u16, span.line)?
                }
                Def::Const { name, ex } => {
                    let v = eval_expr(ex, pc, syms, span)?;
                    syms.define_constant(name, v, span.line)?;
                }
            }
        }
    }

    let npc = apply(&class, pc, syms, span)?;

    let emitted: Vec<u16> = if defining {
        Vec::new()
    } else if let Some(img) = img {
        // Emit real words so the listing shows what was actually encoded.
        match &class {
            Class::Data { wide, items } => items
                .iter()
                .enumerate()
                .map(|(k, item)| {
                    let v = eval_expr(item, pc, syms, span)?;
                    let w = if *wide { v as u16 } else { (v & 0xFF) as u16 };
                    img.set_program_word((pc as u16).saturating_add(k as u16), w)?;
                    Ok(w)
                })
                .collect::<Result<Vec<u16>>>()?,
            Class::Bpt => {
                let w = 0x0000;
                img.set_program_word(pc as u16, w)?;
                vec![w]
            }
            Class::Insn { mnem, ops } => {
                let w = encode_word(mnem, ops, pc, syms, span)?;
                img.set_program_word(pc as u16, w)?;
                vec![w]
            }
            _ => Vec::new(),
        }
    } else {
        Vec::new()
    };
    let _ = src;
    Ok((emitted, npc))
}

/// Compute the next pc for a statement.
fn apply(class: &Class, pc: usize, syms: &Symbols, span: Span) -> Result<usize> {
    if pc > PROGRAM_WORDS {
        return Err(AsmError::new(span, "program memory overflow"));
    }
    match class {
        Class::Pass | Class::Label => Ok(pc),
        Class::Even => Ok((pc + 1) & !1),
        Class::SetPc { ex } => {
            let v = eval_expr(ex, pc, syms, span)?;
            if v < 0 || v as usize > PROGRAM_WORDS {
                return Err(AsmError::new(span, format!(".= {v} out of range")));
            }
            Ok(v as usize)
        }
        Class::Bpt | Class::Insn { .. } => Ok(pc + 1),
        Class::Data { items, .. } => Ok(pc + items.len()),
        // `.endc` reserves 3 bytes (2 words) for the checksum so addresses of
        // anything after it (nothing, typically) stay consistent.
        Class::Endc => Ok(pc + 2),
    }
}

fn eval_expr(e: &expr::Expr, pc: usize, syms: &Symbols, span: Span) -> Result<i64> {
    let r: &dyn Fn(&str) -> Option<i64> = &|name: &str| -> Option<i64> {
        if name == "." {
            Some(pc as i64)
        } else {
            syms.resolve(name, syms.current_scope())
        }
    };
    // `.` is parsed as `Expr::Here`; substitute it with the current pc first.
    e.with_here(pc as i64)
        .eval(&r, span)?
        .ok_or_else(|| AsmError::new(span, "undefined symbol in expression"))
}

/// Turn `$ n` token pairs into a `$<n>` symbol so local-label references parse
/// as identifiers within expressions (e.g. `jmp $1`).
fn normalize_local_labels(toks: &[Token]) -> Vec<Token> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < toks.len() {
        match (toks.get(i), toks.get(i + 1)) {
            (Some(Token::Dollar), Some(Token::Number(n))) => {
                out.push(Token::Sym(format!("${n}")));
                i += 2;
            }
            (other, _) => {
                if let Some(t) = other {
                    out.push(t.clone());
                }
                i += 1;
            }
        }
    }
    out
}

/// Parse an expression from tokens, normalizing `$`-prefixed locals.
fn parse_expr(toks: &[Token], span: Span) -> Result<expr::Expr> {
    let norm = normalize_local_labels(toks);
    expr::Parser::new(&norm, span).parse()
}

fn parse_label_or_local(tk: &[Token], i: &mut usize, defs: &mut Vec<Def>, _span: Span) -> bool {
    match tk.get(*i) {
        Some(Token::Sym(s)) if matches!(tk.get(*i + 1), Some(Token::Colon)) => {
            defs.push(Def::Label(s.clone()));
            *i += 2;
            true
        }
        Some(Token::Dollar)
            if matches!(tk.get(*i + 1), Some(Token::Number(_)))
                && matches!(tk.get(*i + 2), Some(Token::Colon)) =>
        {
            if let Some(Token::Number(k)) = tk.get(*i + 1) {
                defs.push(Def::Local(*k as u32));
            }
            *i += 3;
            true
        }
        _ => false,
    }
}

/// Parse statement tokens into its declarations and [`Class`].
fn classify(tk: &[Token], span: Span) -> Result<(Vec<Def>, Class)> {
    let mut defs = Vec::new();
    let mut i = 0usize;
    // Constants `name = expr` must come first (a label `name:` looks similar).
    if let (Some(Token::Sym(name)), Some(Token::Equal)) = (tk.first(), tk.get(1)) {
        let rest = strip_eof(&tk[2..]);
        let ex = parse_expr(&rest, span)?;
        return Ok((
            vec![Def::Const {
                name: name.clone(),
                ex,
            }],
            Class::Pass,
        ));
    }
    while parse_label_or_local(tk, &mut i, &mut defs, span) {}
    let _ = span;
    match tk.get(i) {
        None | Some(Token::Eof) => Ok((defs, Class::Label)),
        Some(Token::Dot) => {
            // `.= expr` sets the program counter. The lexer yields `Token::Equal`
            // (not a symbol), so it is matched here rather than as a directive name.
            if let Some(Token::Equal) = tk.get(i + 1) {
                let rest = strip_eof(&tk[i + 2..]);
                return Ok((
                    defs,
                    Class::SetPc {
                        ex: parse_expr(&rest, span)?,
                    },
                ));
            }
            let dir = match tk.get(i + 1) {
                Some(Token::Sym(s)) => s.to_lowercase(),
                _ => return Err(AsmError::new(span, "expected directive name after `.`")),
            };
            let rest = strip_eof(&tk[i + 2..]);
            let c = match dir.as_str() {
                "byte" => Class::Data {
                    wide: false,
                    items: expr_list(&rest, span)?,
                },
                "word" => Class::Data {
                    wide: true,
                    items: expr_list(&rest, span)?,
                },
                "even" => Class::Even,
                "bpt" => Class::Bpt,
                "end" => Class::Pass,
                "endc" => Class::Endc,
                "list" => Class::Pass,
                other => return Err(AsmError::new(span, format!("unknown directive `.{other}`"))),
            };
            Ok((defs, c))
        }
        Some(Token::Sym(m)) => {
            let mn = m.to_lowercase();
            let ops = split_commas(&strip_eof(&tk[i + 1..]));
            Ok((defs, Class::Insn { mnem: mn, ops }))
        }
        Some(other) => Err(AsmError::new(
            span,
            format!("syntax error: {}", other.describe()),
        )),
    }
}

fn expr_list(toks: &[Token], span: Span) -> Result<Vec<expr::Expr>> {
    if toks.is_empty() {
        return Ok(Vec::new());
    }
    split_commas(toks)
        .iter()
        .map(|g| parse_expr(g, span))
        .collect()
}

fn split_commas(toks: &[Token]) -> Vec<Vec<Token>> {
    let mut out = Vec::new();
    let mut cur = Vec::new();
    let mut depth = 0i32;
    for t in toks {
        match t {
            Token::LParen => {
                depth += 1;
                cur.push(t.clone());
            }
            Token::RParen => {
                depth -= 1;
                cur.push(t.clone());
            }
            Token::Comma if depth == 0 => {
                out.push(cur);
                cur = Vec::new();
            }
            _ => cur.push(t.clone()),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn strip_eof(toks: &[Token]) -> Vec<Token> {
    toks.iter()
        .filter(|&t| !matches!(t, Token::Eof))
        .cloned()
        .collect()
}

// =====================================================================
// Encoding
// =====================================================================

/// Parse a `%an`/`a0..d7` data-memory operand to its 5-bit slot.
fn data_operand(g: &[Token], span: Span) -> Result<Operand> {
    let mut idx = 0usize;
    if matches!(g.first(), Some(Token::Percent)) {
        idx = 1;
    }
    match g.get(idx) {
        Some(Token::Sym(s)) => {
            let seg = Segment::from_letter(s.chars().next().unwrap_or('_'))
                .ok_or_else(|| AsmError::new(span, format!("bad segment in operand `{s}`")))?;
            let idx: u8 = s[1..]
                .parse()
                .map_err(|_| AsmError::new(span, format!("bad index in `{s}`")))?;
            if idx > 7 {
                return Err(AsmError::new(
                    span,
                    format!("operand index {idx} out of range"),
                ));
            }
            Ok(Operand::new(seg, idx))
        }
        _ => Err(AsmError::new(
            span,
            "expected a data operand (e.g. `%a5` or `d3`",
        )),
    }
}

/// Parse a service-register operand `#n` / `#letter`.
fn service_reg(toks: &[Token], span: Span) -> Result<u8> {
    let mut idx = 0usize;
    if matches!(toks.first(), Some(Token::Hash)) {
        idx = 1;
    }
    match toks.get(idx) {
        Some(Token::Number(n)) if *n >= 0 && *n <= 7 => Ok(*n as u8),
        Some(Token::Sym(c)) if c.len() == 1 => {
            let v = match c.to_ascii_lowercase().as_str() {
                "a" => 0,
                "b" => 1,
                "c" => 2,
                "d" => 3,
                _ => return Err(AsmError::new(span, format!("bad service register `{c}`"))),
            };
            Ok(v)
        }
        _ => Err(AsmError::new(span, "expected a service register")),
    }
}

fn operand(toks: &[Token], span: Span) -> Result<Operand> {
    data_operand(toks, span)
}

fn imm8(toks: &[Token], pc: usize, syms: &Symbols, span: Span) -> Result<u8> {
    let ex = parse_expr(toks, span)?;
    let v = eval_expr(&ex, pc, syms, span)?;
    Ok((v & 0xFF) as u8)
}

fn imm5(toks: &[Token], pc: usize, syms: &Symbols, span: Span) -> Result<u8> {
    let ex = parse_expr(toks, span)?;
    let v = eval_expr(&ex, pc, syms, span)?;
    Ok((v & 0x1F) as u8)
}

fn br_target(toks: &[Token], pc: usize, syms: &Symbols, span: Span) -> Result<u16> {
    let ex = parse_expr(toks, span)?;
    let v = eval_expr(&ex, pc, syms, span)?;
    if !(0..=0xFFF).contains(&v) {
        return Err(AsmError::new(
            span,
            format!("branch target {v} out of range (0..0xFFF)"),
        ));
    }
    Ok(v as u16)
}

/// Which tetrad position a mnemonic selects (`l`=low, `h`=high).
/// Recognises the bit-manipulation mnemonics `bic`/`bis`/`btg`/`btt` with an
/// optional `l`/`h` suffix (the place of the affected tetrad).
///
/// The suffix alone is not enough to identify them: `push` and `shl` also end
/// in `h`/`l`, so the base mnemonic has to be one of the four.
fn tetrad_place(mnem: &str) -> Option<(Place, String)> {
    let lower = mnem.to_ascii_lowercase();
    let (place, base) = if let Some(base) = lower.strip_suffix('l') {
        (Place::Low, base)
    } else {
        let base = lower.strip_suffix('h')?;
        (Place::High, base)
    };
    if !matches!(base, "bic" | "bis" | "btg" | "btt") {
        return None;
    }

    Some((place, base.to_string()))
}

/// Encode one instruction word.
fn encode_word(
    mnem: &str,
    ops: &[Vec<Token>],
    pc: usize,
    syms: &Symbols,
    span: Span,
) -> Result<u16> {
    let one = |i: usize| -> Result<Operand> {
        ops.get(i)
            .map(|g| operand(g, span))
            .unwrap_or_else(|| Err(AsmError::new(span, "missing operand")))
    };
    let two = |i: usize| -> Result<Operand> { one(i) };

    // ---- literal immediate two-arg: dst + const
    let lit8 = |i: usize| {
        imm8(
            ops.get(i)
                .ok_or_else(|| AsmError::new(span, "missing operand"))?,
            pc,
            syms,
            span,
        )
    };
    let lit5 = |i: usize| {
        imm5(
            ops.get(i)
                .ok_or_else(|| AsmError::new(span, "missing operand"))?,
            pc,
            syms,
            span,
        )
    };

    Ok(match mnem {
        // ---- two-operand data (dst, src)
        "mov" => Instruction::Mov {
            dst: two(0)?,
            src: two(1)?,
        }
        .encode()?,
        "cmp" => Instruction::Cmp {
            dst: two(0)?,
            src: two(1)?,
        }
        .encode()?,
        "sub" => Instruction::Sub {
            dst: two(0)?,
            src: two(1)?,
        }
        .encode()?,
        "add" => Instruction::Add {
            dst: two(0)?,
            src: two(1)?,
        }
        .encode()?,
        "and" => Instruction::And {
            dst: two(0)?,
            src: two(1)?,
        }
        .encode()?,
        "or" => Instruction::Or {
            dst: two(0)?,
            src: two(1)?,
        }
        .encode()?,
        "xor" => Instruction::Xor {
            dst: two(0)?,
            src: two(1)?,
        }
        .encode()?,

        // ---- 8-bit literal
        "movl" => Instruction::Movl {
            dst: two(0)?,
            value: lit8(1)?,
        }
        .encode()?,
        "cmpl" => Instruction::Cmpl {
            dst: two(0)?,
            value: lit8(1)?,
        }
        .encode()?,

        // ---- 5-bit literal
        "addl" => Instruction::Addl {
            dst: two(0)?,
            value: lit5(1)?,
        }
        .encode()?,
        "subl" => Instruction::Subl {
            dst: two(0)?,
            value: lit5(1)?,
        }
        .encode()?,

        // ---- pseudo increments
        "inc" => Instruction::Addl {
            dst: two(0)?,
            value: 1,
        }
        .encode()?,
        "dec" => Instruction::Subl {
            dst: two(0)?,
            value: 1,
        }
        .encode()?,
        "clr" => Instruction::Movl {
            dst: two(0)?,
            value: 0,
        }
        .encode()?,

        // ---- address-loading pseudos (expand to movl/ldr) ----
        // mial dst,addr  -> movl dst, lo(addr)    (instruction address, word)
        // miah dst,addr  -> movl dst, hi(addr)
        // mdal dst,addr  -> movl dst, lo(addr)    (data address in command memory)
        // mdah dst,addr  -> movl dst, hi(addr)&0x3F (bits 6-7 cleared per datasheet)
        // ldal #6,addr   -> ldr  #6, lo(addr)
        // ldah #7,addr   -> ldr  #7, hi(addr)&0x3F
        // lia  #6,addr   -> ldr  #6, lo(addr)     (jump target address)
        // liah #7,addr   -> ldr  #7, hi(addr)
        "mial" | "mdal" => {
            let dst = two(0)?;
            let v = eval_expr(
                &parse_expr(ops.get(1).unwrap_or(&Vec::new()), span)?,
                pc,
                syms,
                span,
            )?;
            Instruction::Movl {
                dst,
                value: (v & 0xFF) as u8,
            }
            .encode()?
        }
        "miah" => {
            let dst = two(0)?;
            let v = eval_expr(
                &parse_expr(ops.get(1).unwrap_or(&Vec::new()), span)?,
                pc,
                syms,
                span,
            )?;
            Instruction::Movl {
                dst,
                value: ((v >> 8) & 0xFF) as u8,
            }
            .encode()?
        }
        "mdah" => {
            let dst = two(0)?;
            let v = eval_expr(
                &parse_expr(ops.get(1).unwrap_or(&Vec::new()), span)?,
                pc,
                syms,
                span,
            )?;
            Instruction::Movl {
                dst,
                value: ((v >> 8) & 0x3F) as u8,
            }
            .encode()?
        }
        "ldal" | "lia" => {
            let reg = service_reg(
                ops.first()
                    .ok_or_else(|| AsmError::new(span, "missing operand"))?,
                span,
            )?;
            let v = eval_expr(
                &parse_expr(ops.get(1).unwrap_or(&Vec::new()), span)?,
                pc,
                syms,
                span,
            )?;
            Instruction::Ldr {
                reg,
                value: (v & 0xFF) as u8,
            }
            .encode()?
        }
        "ldah" | "liah" => {
            let reg = service_reg(
                ops.first()
                    .ok_or_else(|| AsmError::new(span, "missing operand"))?,
                span,
            )?;
            let v = eval_expr(
                &parse_expr(ops.get(1).unwrap_or(&Vec::new()), span)?,
                pc,
                syms,
                span,
            )?;
            Instruction::Ldr {
                reg,
                value: ((v >> 8) & 0xFF) as u8,
            }
            .encode()?
        }

        // ---- bit manipulation (BIC/BIS/BTG/BTT with l/h suffix)
        _ if tetrad_place(mnem).is_some() => {
            let (place, base) = tetrad_place(mnem).unwrap();
            let v = imm5(
                ops.get(1)
                    .ok_or_else(|| AsmError::new(span, "missing operand"))?,
                pc,
                syms,
                span,
            )?;
            let dst = two(0)?;
            let tetrad = (v & 0x0F) as u8;
            let ins = match base.as_str() {
                "bic" => Instruction::Bic { dst, tetrad, place },
                "bis" => Instruction::Bis { dst, tetrad, place },
                "btg" => Instruction::Btg { dst, tetrad, place },
                "btt" => Instruction::Btt { dst, tetrad, place },
                other => {
                    return Err(AsmError::new(
                        span,
                        format!("unknown bit mnemonic `{other}`"),
                    ));
                }
            };
            ins.encode()?
        }

        // ---- one-operand data ops
        "swap" => Instruction::Swap(one(0)?).encode()?,
        "neg" => Instruction::Neg(one(0)?).encode()?,
        "not" => Instruction::Not(one(0)?).encode()?,
        "shl" => Instruction::Shl(one(0)?).encode()?,
        "shr" => Instruction::Shr(one(0)?).encode()?,
        "shra" => Instruction::Shra(one(0)?).encode()?,
        "rlc" => Instruction::Rlc(one(0)?).encode()?,
        "rrc" => Instruction::Rrc(one(0)?).encode()?,
        "adc" => Instruction::Adc(one(0)?).encode()?,
        "sbc" => Instruction::Sbc(one(0)?).encode()?,

        // ---- service registers
        "ldr" => Instruction::Ldr {
            reg: service_reg(
                ops.first()
                    .ok_or_else(|| AsmError::new(span, "missing operand"))?,
                span,
            )?,
            value: lit8(1)?,
        }
        .encode()?,
        "mfpr" => Instruction::Mfpr {
            dst: two(0)?,
            reg: service_reg(
                ops.get(1)
                    .ok_or_else(|| AsmError::new(span, "missing operand"))?,
                span,
            )?,
        }
        .encode()?,
        "mtpr" => Instruction::Mtpr {
            reg: service_reg(
                ops.first()
                    .ok_or_else(|| AsmError::new(span, "missing operand"))?,
                span,
            )?,
            src: two(1)?,
        }
        .encode()?,
        "push" => Instruction::Push {
            reg: service_reg(
                ops.first()
                    .ok_or_else(|| AsmError::new(span, "missing operand"))?,
                span,
            )?,
        }
        .encode()?,
        "pop" => Instruction::Pop {
            reg: service_reg(
                ops.first()
                    .ok_or_else(|| AsmError::new(span, "missing operand"))?,
                span,
            )?,
        }
        .encode()?,
        "sst" => Instruction::Sst {
            mask: (imm8(
                ops.first()
                    .ok_or_else(|| AsmError::new(span, "missing operand"))?,
                pc,
                syms,
                span,
            )? & 0x0F),
        }
        .encode()?,
        "cst" => Instruction::Cst {
            mask: (imm8(
                ops.first()
                    .ok_or_else(|| AsmError::new(span, "missing operand"))?,
                pc,
                syms,
                span,
            )? & 0x0F),
        }
        .encode()?,

        // ---- control flow
        "jmp" => Instruction::Jmp {
            addr: br_target(
                ops.first()
                    .ok_or_else(|| AsmError::new(span, "missing operand"))?,
                pc,
                syms,
                span,
            )?,
        }
        .encode()?,
        "jsr" => Instruction::Jsr {
            addr: br_target(
                ops.first()
                    .ok_or_else(|| AsmError::new(span, "missing operand"))?,
                pc,
                syms,
                span,
            )?,
        }
        .encode()?,
        "jz" | "jeq" => Instruction::Jz {
            addr: br_target(
                ops.first()
                    .ok_or_else(|| AsmError::new(span, "missing operand"))?,
                pc,
                syms,
                span,
            )?,
        }
        .encode()?,
        "jnz" | "jne" => Instruction::Jnz {
            addr: br_target(
                ops.first()
                    .ok_or_else(|| AsmError::new(span, "missing operand"))?,
                pc,
                syms,
                span,
            )?,
        }
        .encode()?,
        "jns" => Instruction::Jns {
            addr: br_target(
                ops.first()
                    .ok_or_else(|| AsmError::new(span, "missing operand"))?,
                pc,
                syms,
                span,
            )?,
        }
        .encode()?,
        "js" => Instruction::Js {
            addr: br_target(
                ops.first()
                    .ok_or_else(|| AsmError::new(span, "missing operand"))?,
                pc,
                syms,
                span,
            )?,
        }
        .encode()?,
        "jnc" => Instruction::Jnc {
            addr: br_target(
                ops.first()
                    .ok_or_else(|| AsmError::new(span, "missing operand"))?,
                pc,
                syms,
                span,
            )?,
        }
        .encode()?,
        "jc" => Instruction::Jc {
            addr: br_target(
                ops.first()
                    .ok_or_else(|| AsmError::new(span, "missing operand"))?,
                pc,
                syms,
                span,
            )?,
        }
        .encode()?,
        "ijmp" => Instruction::Ijmp.encode()?,
        "ijsr" => Instruction::Ijsr.encode()?,
        "rts" => Instruction::Rts.encode()?,
        "rti" => Instruction::Rti.encode()?,
        // Return with carry set to `c` (0x000E | c). `rtsc` accepts an optional
        // single-bit operand (`rtsc` or `rtsc 1`).
        "rtsc" => {
            let carry = if ops.is_empty() {
                false
            } else {
                let g = ops
                    .first()
                    .ok_or_else(|| AsmError::new(span, "missing operand"))?;
                eval_expr(&parse_expr(g, span)?, pc, syms, span)? != 0
            };
            Instruction::Rtsc { carry }.encode()?
        }

        // ---- fixed words / special
        "nop" => Instruction::Nop.encode()?,
        "wait" => Instruction::Wait.encode()?,
        "reset" => Instruction::Reset.encode()?,
        "stop" => Instruction::Stop.encode()?,
        "sksp" => Instruction::Sksp.encode()?,
        "tof" => Instruction::Tof.encode()?,
        "tdc" => Instruction::Tdc.encode()?,

        other => {
            return Err(AsmError::new(span, format!("unknown mnemonic `{other}`")));
        }
    })
}
