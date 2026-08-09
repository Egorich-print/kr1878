//! Lexical analysis for the TESSA assembly language.
//!
//! The language alphabet (from `docs/REVERSE_ENGINEERING.md`):
//! digits, letters, operator symbols `_ = - + * / & ! ^ \ ( )`,
//! service symbols `. % # : ; ( $ @` and comma. Comments start with `;`
//! and run to the end of the line.

use crate::error::{AsmError, Result, Span};

/// A single lexical token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    /// Symbol: identifier, label, constant, mnemonic or directive.
    Sym(String),
    /// A number in the language-specific bases (suffix `h/o/b`).
    Number(i64),
    /// Comma `,`.
    Comma,
    /// Colon `:` (label terminator).
    Colon,
    /// Equal `=` (constant assignment).
    Equal,
    /// Parenthesis `(`.
    LParen,
    /// Parenthesis `)`.
    RParen,
    /// Dot `.` (current program counter) or directive prefix.
    Dot,
    /// Percent `%` (data-memory operand prefix).
    Percent,
    /// Hash `#` (service-register prefix).
    Hash,
    /// Plus `+`.
    Plus,
    /// Minus `-`.
    Minus,
    /// Star `*` (multiplication).
    Star,
    /// Slash `/` (division).
    Slash,
    /// Ampersand `&` (bitwise AND).
    Amp,
    /// Exclamation `!` (bitwise OR).
    Bang,
    /// Caret `^` (bitwise NOT / inversion).
    Caret,
    /// Backslash `\` (bitwise XOR).
    Backslash,
    /// Dollar `$` (local label prefix).
    Dollar,
    /// At `@` (macro label prefix).
    At,
    /// Newline (logical line boundary).
    Newline,
    /// End of input.
    Eof,
}

impl Token {
    pub fn describe(&self) -> String {
        match self {
            Token::Sym(s) => format!("identifier `{s}`"),
            Token::Number(n) => format!("number `{n}`"),
            Token::Comma => "comma".into(),
            Token::Colon => "colon".into(),
            Token::Equal => "`=`".into(),
            Token::LParen => "`(`".into(),
            Token::RParen => "`)`".into(),
            Token::Dot => "`.`".into(),
            Token::Percent => "`%`".into(),
            Token::Hash => "`#`".into(),
            Token::Plus => "`+`".into(),
            Token::Minus => "`-`".into(),
            Token::Star => "`*`".into(),
            Token::Slash => "`/`".into(),
            Token::Amp => "`&`".into(),
            Token::Bang => "`!`".into(),
            Token::Caret => "`^`".into(),
            Token::Backslash => "`\\`".into(),
            Token::Dollar => "`$`".into(),
            Token::At => "`@`".into(),
            Token::Newline => "end of line".into(),
            Token::Eof => "end of input".into(),
        }
    }
}

/// Tokenize a single source line.
///
/// `;` comments are stripped here; the returned token stream ends with
/// [`Token::Eof`].
pub fn tokenize(source: &str, span: Span) -> Result<Vec<Token>> {
    let mut out = Vec::new();
    let chars: Vec<char> = source.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        match c {
            ';' => break, // comment to end of line
            ' ' | '\t' | '\r' => {
                i += 1;
            }
            '\n' => {
                out.push(Token::Newline);
                i += 1;
            }
            ',' => {
                out.push(Token::Comma);
                i += 1;
            }
            ':' => {
                out.push(Token::Colon);
                i += 1;
            }
            '=' => {
                out.push(Token::Equal);
                i += 1;
            }
            '(' => {
                out.push(Token::LParen);
                i += 1;
            }
            ')' => {
                out.push(Token::RParen);
                i += 1;
            }
            '.' => {
                out.push(Token::Dot);
                i += 1;
            }
            '%' => {
                out.push(Token::Percent);
                i += 1;
            }
            '#' => {
                out.push(Token::Hash);
                i += 1;
            }
            '+' => {
                out.push(Token::Plus);
                i += 1;
            }
            '-' => {
                out.push(Token::Minus);
                i += 1;
            }
            '*' => {
                out.push(Token::Star);
                i += 1;
            }
            '/' => {
                out.push(Token::Slash);
                i += 1;
            }
            '&' => {
                out.push(Token::Amp);
                i += 1;
            }
            '!' => {
                out.push(Token::Bang);
                i += 1;
            }
            '^' => {
                out.push(Token::Caret);
                i += 1;
            }
            '\\' => {
                out.push(Token::Backslash);
                i += 1;
            }
            '$' => {
                out.push(Token::Dollar);
                i += 1;
            }
            '@' => {
                out.push(Token::At);
                i += 1;
            }
            c if c.is_ascii_digit() => {
                // Number with optional suffix, or an identifier starting with digit
                // (e.g. `0FFh` is a hex literal; labels never start with a digit).
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                let word: String = chars[start..i].iter().collect();
                match parse_number(&word) {
                    Some(n) => out.push(Token::Number(n)),
                    None => {
                        return Err(AsmError::new(
                            span,
                            format!("invalid number literal `{word}`"),
                        ))
                    }
                }
            }
            c if c.is_ascii_alphabetic() => {
                let start = i;
                while i < chars.len()
                    && (chars[i].is_ascii_alphanumeric() || chars[i] == '_')
                {
                    i += 1;
                }
                out.push(Token::Sym(chars[start..i].iter().collect()));
            }
            other => {
                return Err(AsmError::new(span, format!("unexpected character `{other}`")));
            }
        }
    }
    out.push(Token::Eof);
    Ok(out)
}

/// Parse a number in one of the TESSA bases:
/// `12` (decimal), `12o` (octal), `12h` (hex), `0101b` (binary).
pub fn parse_number(word: &str) -> Option<i64> {
    if word.is_empty() {
        return None;
    }
    let (digits, radix) = match word.as_bytes().last() {
        Some(b'h') | Some(b'H') => (&word[..word.len() - 1], 16),
        Some(b'o') | Some(b'O') => (&word[..word.len() - 1], 8),
        Some(b'b') | Some(b'B') => (&word[..word.len() - 1], 2),
        _ => (word, 10),
    };
    if digits.is_empty() {
        return None;
    }
    i64::from_str_radix(digits, radix).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lex(s: &str) -> Result<Vec<Token>> {
        tokenize(s, Span { file: 0, line: 1 })
    }

    fn syms(s: &str) -> Vec<String> {
        lex(s)
            .unwrap()
            .into_iter()
            .filter_map(|t| match t {
                Token::Sym(x) => Some(x),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn comments_stripped() {
        let toks = lex("mov %a1,%a2 ; comment").unwrap();
        assert!(toks.contains(&Token::Sym("mov".into())));
        assert!(!toks.contains(&Token::Sym("comment".into())));
        assert_eq!(toks.last(), Some(&Token::Eof));
    }

    #[test]
    fn number_suffixes() {
        assert_eq!(parse_number("12"), Some(12));
        assert_eq!(parse_number("12o"), Some(10));
        assert_eq!(parse_number("0101b"), Some(5));
        assert_eq!(parse_number("0FFh"), Some(255));
        assert_eq!(parse_number("10h"), Some(16));
        assert_eq!(parse_number("zz"), None);
    }

    #[test]
    fn operands_with_prefixes() {
        let toks = lex("%a1,#d,%b2").unwrap();
        assert_eq!(toks[0], Token::Percent);
        assert_eq!(toks[1], Token::Sym("a1".into()));
        assert_eq!(toks[2], Token::Comma);
        assert_eq!(toks[3], Token::Hash);
        assert_eq!(toks[4], Token::Sym("d".into()));
    }

    #[test]
    fn local_label_dollar() {
        let toks = lex("$53: movl a0,0").unwrap();
        assert_eq!(toks[0], Token::Dollar);
        assert_eq!(toks[1], Token::Number(53));
        assert_eq!(toks[2], Token::Colon);
    }
}