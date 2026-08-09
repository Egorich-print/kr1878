//! Symbol table for the assembler: global labels, constants and local labels.

use std::collections::BTreeMap;

use crate::error::{AsmError, Result};

/// A global symbol (label or constant value).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SymValue {
    pub value: i64,
    pub is_label: bool,
    pub line: usize,
}

/// A local label `$nn`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalSym {
    pub value: u16,
    pub line: usize,
}

/// Symbol table exposed to the CLI (`.sym`).
#[derive(Debug, Clone, Default)]
pub struct Symbols {
    labels: BTreeMap<String, SymValue>,
    constants: BTreeMap<String, i64>,
    locals: BTreeMap<(u32, u32), LocalSym>,
}

impl Symbols {
    pub fn new() -> Self {
        Self::default()
    }

    /// Constant `name = expr` assigned once.
    pub fn define_constant(&mut self, name: &str, value: i64, line: usize) -> Result<()> {
        if self.labels.contains_key(name) || self.constants.contains_key(name) {
            return Err(AsmError::new(
                crate::error::Span { file: 0, line },
                format!("symbol `{name}` redefined"),
            ));
        }
        self.constants.insert(name.to_string(), value);
        Ok(())
    }

    /// Record a global label at `value` (its ordinal becomes its scope).
    pub fn define_label(&mut self, name: &str, value: u16, line: usize) -> Result<()> {
        if self.labels.contains_key(name) || self.constants.contains_key(name) {
            return Err(AsmError::new(
                crate::error::Span { file: 0, line },
                format!("label `{name}` redefined"),
            ));
        }
        self.labels.insert(
            name.to_string(),
            SymValue {
                value: value as i64,
                is_label: true,
                line,
            },
        );
        Ok(())
    }

    /// Record a local label under the given scope (ordinal of enclosing global).
    pub fn define_local(&mut self, scope: u32, num: u32, value: u16, line: usize) -> Result<()> {
        if self.locals.contains_key(&(scope, num)) {
            return Err(AsmError::new(
                crate::error::Span { file: 0, line },
                format!("local label `${num}` redefined in scope"),
            ));
        }
        self.locals.insert((scope, num), LocalSym { value, line });
        Ok(())
    }

    /// Current scope: the ordinal of the most recent global label.
    pub fn current_scope(&self) -> u32 {
        self.labels.len() as u32
    }

    /// Is `name` a local-label reference (`$digits`)?
    pub fn is_local(&self, name: &str) -> bool {
        name.starts_with('$')
    }

    /// Resolve any symbol (constant, global label, local label) to a value.
    pub fn resolve(&self, name: &str, scope: u32) -> Option<i64> {
        if let Some(&v) = self.constants.get(name) {
            return Some(v);
        }
        if let Some(s) = self.labels.get(name) {
            return Some(s.value);
        }
        if let Some(digits) = name.strip_prefix('$')
            && let Ok(num) = digits.parse::<u32>()
        {
            for sc in (0..=scope).rev() {
                if let Some(l) = self.locals.get(&(sc, num)) {
                    return Some(l.value as i64);
                }
            }
        }
        None
    }

    /// Iterate global symbols for the listing.
    pub fn globals(&self) -> impl Iterator<Item = (&String, &SymValue)> {
        self.labels.iter()
    }
}
