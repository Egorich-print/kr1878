//! Source preprocessor: `#define`, `#include`, `.macro`/`.endmacro` expansion
//! and `.if`/`.else`/`.endif` conditional compilation.
//!
//! Emits a flat list of assembly statements (strings) ready for lexing.

use std::collections::HashMap;

use crate::error::{AsmError, Result, Span};

/// A post-preprocessing assembly statement (single logical line, no trailing `\n`).
#[derive(Debug, Clone)]
pub struct Stmt {
    pub text: String,
    pub span: Span,
}

#[derive(Debug)]
struct MacroBody {
    params: Vec<String>,
    lines: Vec<String>,
}

/// Preprocessor state (macros/defines kept across modules and includes).
pub struct Preprocessor<'a> {
    loader: Option<&'a mut dyn FnMut(&str) -> Result<String>>,
    macros: HashMap<String, MacroBody>,
    defines: HashMap<String, String>,
}

impl<'a> Preprocessor<'a> {
    pub fn new(loader: Option<&'a mut dyn FnMut(&str) -> Result<String>>) -> Self {
        Preprocessor { loader, macros: HashMap::new(), defines: HashMap::new() }
    }

    /// Run the preprocessor over the whole source (all modules concatenated).
    pub fn run(&mut self, source: &str) -> Result<Vec<Stmt>> {
        self.run_inner(source, 0, 0)
    }

    fn run_inner(&mut self, source: &str, base_file: usize, base_line: usize) -> Result<Vec<Stmt>> {
        let mut out = Vec::new();
        let lines: Vec<&str> = source.lines().collect();
        let mut i = 0usize;
        let mut cond: Vec<bool> = Vec::new();
        while i < lines.len() {
            let span = Span { file: base_file, line: base_line + i + 1 };
            let line = lines[i].trim();
            i += 1;
            if line.is_empty() {
                continue;
            }
            let lower = line.to_ascii_lowercase();

            // ---- macro definition capture
            if lower.starts_with(".macro") {
                let (name, params) = parse_macro_header(line, &span)?;
                let mut body = Vec::new();
                loop {
                    if i >= lines.len() {
                        return Err(AsmError::new(span, "unterminated .macro"));
                    }
                    let b = lines[i].trim().to_ascii_lowercase();
                    i += 1;
                    if b == ".endmacro" || b == ".endm" {
                        break;
                    }
                    body.push(lines[i - 1].to_string());
                }
                if self.macros.contains_key(&name) {
                    return Err(AsmError::new(span, format!("macro `{name}` redefined")));
                }
                self.macros.insert(name, MacroBody { params, lines: body });
                continue;
            }

            // skip while a parent conditional is false
            if !cond.iter().all(|c| *c) && !is_control(line) {
                continue;
            }

            // ---- conditionals
            if line == ".if" || lower.starts_with(".if ") {
                let expr = line[".if".len()..].trim();
                let val = crate::eval_cond(&apply_defines(expr, &self.defines))?;
                cond.push(val != 0);
                continue;
            }
            if lower == ".else" {
                if let Some(c) = cond.last_mut() {
                    *c = !*c;
                }
                continue;
            }
            if lower == ".endif" || lower == ".endc" {
                cond.pop();
                continue;
            }

            // ---- #define
            if let Some(rest) = line.strip_prefix("#define") {
                let tmp = rest.trim();
                if let Some(eq) = tmp.find(char::is_whitespace) {
                    let name = &tmp[..eq];
                    if !name.is_empty() {
                        self.defines.insert(name.to_string(), tmp[eq..].trim().to_string());
                    }
                }
                continue;
            }

            // ---- #include (recursively preprocess the included file)
            if let Some(rest) = line.strip_prefix("#include") {
                let fname = rest.trim().trim_matches('"').trim();
                let src = {
                    let loader = self.loader.as_mut().ok_or_else(|| {
                        AsmError::new(span, format!("`#include \"{fname}\"` but no loader provided"))
                    })?;
                    loader(fname)?
                };
                let extra = self.run_inner(&src, base_file + 1, 0)?;
                out.extend(extra);
                continue;
            }

            // ---- macro invocation
            if let Some(first) = line.split_whitespace().next() {
                if let Some(m) = self.macros.get(first) {
                    let operand = line[first.len()..].trim();
                    let args = split_args(operand);
                    let expanded = expand_macro(m, &args, &span)?;
                    for s in expanded {
                        out.push(Stmt { text: s, span });
                    }
                    continue;
                }
            }

            // ---- apply #define replacements and emit
            out.push(Stmt { text: apply_defines(line, &self.defines), span });
        }
        Ok(out)
    }
}

fn is_control(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    l == ".if" || l.starts_with(".if ") || l == ".else" || l == ".endif" || l == ".endc"
}

fn parse_macro_header(line: &str, span: &Span) -> Result<(String, Vec<String>)> {
    let rest = line[".macro".len()..].trim();
    let mut parts = rest.split_whitespace();
    let name = parts
        .next()
        .ok_or_else(|| AsmError::new(*span, ".macro without a name"))?
        .to_string();
    if !name.starts_with(|c: char| c.is_ascii_alphabetic()) {
        return Err(AsmError::new(*span, format!("bad macro name `{name}`")));
    }
    let rest = parts.collect::<Vec<_>>().join(" ");
    let params = rest
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    Ok((name, params))
}

/// Expand a macro body: substitute positional params, auto-rename `@N` labels.
fn expand_macro(m: &MacroBody, args: &[String], span: &Span) -> Result<Vec<String>> {
    if args.len() < m.params.len() {
        return Err(AsmError::new(*span, format!(
            "macro expects {} args, got {}",
            m.params.len(),
            args.len()
        )));
    }
    let mut sub: HashMap<String, String> = HashMap::new();
    for (i, p) in m.params.iter().enumerate() {
        sub.insert(p.clone(), args.get(i).cloned().unwrap_or_default());
    }
    let mut out = Vec::new();
    for (j, line) in m.lines.iter().enumerate() {
        let mut l = line.clone();
        for (k, v) in &sub {
            l = l.replace(k, v);
        }
        l = rename_at_labels(&l, j);
        out.push(l);
    }
    Ok(out)
}

/// Replace `@N` label markers with unique numeric locals `$N{roff}`.
fn rename_at_labels(line: &str, roff: usize) -> String {
    let mut s = line.to_string();
    let mut more = true;
    while more {
        more = false;
        if let Some(pos) = s.find('@') {
            let mut end = pos + 1;
            while end < s.len() && s.as_bytes()[end].is_ascii_digit() {
                end += 1;
            }
            if end > pos + 1 && end < s.len() {
                let unique = format!("${}{}", &s[pos + 1..end], roff * 997);
                s.replace_range(pos..end, &unique);
                more = true;
            } else {
                break;
            }
        }
    }
    s
}

fn split_args(s: &str) -> Vec<String> {
    let mut depth = 0i32;
    let mut cur = String::new();
    let mut out = Vec::new();
    for c in s.chars() {
        match c {
            '(' => {
                depth += 1;
                cur.push(c);
            }
            ')' => {
                depth -= 1;
                cur.push(c);
            }
            ',' if depth == 0 => {
                out.push(cur.trim().to_string());
                cur.clear();
            }
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

fn apply_defines(line: &str, defines: &HashMap<String, String>) -> String {
    let mut s = line.to_string();
    for (k, v) in defines {
        s = s.replace(k, v);
    }
    s
}