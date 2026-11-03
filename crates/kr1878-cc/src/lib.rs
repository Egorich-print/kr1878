//! C compiler for the KR1878VE1 ("ТЕСЕЙ") core.
//!
//! The front end is [chibicc] (MIT, vendored under `vendor/chibicc`); the
//! back end is our own (`vendor/chibicc/tesei_codegen.c`), which emits
//! assembly in the syntax of our own assembler (TESSA).
//!
//! # Target model
//!
//! The core has no general-purpose registers: every operand is a byte of the
//! 256-byte data space. The backend therefore uses a one-accumulator memory
//! model; the details are in `docs/adr/ADR-006-tesei-c-abi.md`.
//!
//! # Example
//!
//! ```no_run
//! let out = kr1878_cc::compile("int main(void) { return 0; }", "main.c").unwrap();
//! assert!(out.asm.contains("F_main:"));
//! ```
//!
//! [chibicc]: https://github.com/rui314/chibicc

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::sync::Mutex;

/// The vendored front end keeps its state in globals (the scope chain, the
/// symbol tables, the current file), so two compilations must not run at the
/// same time. The lock serialises them.
static COMPILE_LOCK: Mutex<()> = Mutex::new(());

unsafe extern "C" {
    /// Compiles C source into TESEI assembly.
    ///
    /// Returns 0 on success and stores a malloc'd NUL-terminated string in
    /// `out` (the caller must free it). On failure it returns 1 and writes a
    /// diagnostic into `err`.
    fn tesey_compile(
        source: *const c_char,
        filename: *const c_char,
        out: *mut *mut c_char,
        err: *mut c_char,
        err_len: usize,
    ) -> c_int;

    fn free(ptr: *mut c_void);
}

/// The result of a successful compilation.
#[derive(Debug, Clone)]
pub struct CompileOutput {
    /// The generated assembly, in the syntax of our own assembler.
    pub asm: String,
}

/// Compiles C source into TESEI assembly.
///
/// `filename` only appears in diagnostics. On failure the returned `Err`
/// holds the compiler's diagnostic (`file:line: message` plus the offending
/// source line).
pub fn compile(source: &str, filename: &str) -> Result<CompileOutput, String> {
    let _guard = COMPILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let source_c = CString::new(source).map_err(|_| "source contains a NUL byte".to_string())?;
    let filename_c =
        CString::new(filename).map_err(|_| "file name contains a NUL byte".to_string())?;

    let mut out_ptr: *mut c_char = std::ptr::null_mut();
    let mut err_buf = vec![0i8; 2048];

    let rc = unsafe {
        tesey_compile(
            source_c.as_ptr(),
            filename_c.as_ptr(),
            &mut out_ptr,
            err_buf.as_mut_ptr(),
            err_buf.len(),
        )
    };

    if rc != 0 {
        let msg = unsafe { CStr::from_ptr(err_buf.as_ptr()) }
            .to_string_lossy()
            .into_owned();
        return Err(if msg.is_empty() {
            "compilation failed".to_string()
        } else {
            msg
        });
    }

    let asm = unsafe {
        let s = CStr::from_ptr(out_ptr).to_string_lossy().into_owned();
        free(out_ptr as *mut c_void);
        s
    };

    Ok(CompileOutput { asm })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiles_a_trivial_program() {
        let out = compile("int main(void) { return 0; }", "main.c").expect("must compile");
        assert!(out.asm.contains("F_main:"), "asm:\n{}", out.asm);
        assert!(out.asm.contains("jmp Lstart"), "asm:\n{}", out.asm);
    }

    #[test]
    fn reports_syntax_errors() {
        let err = compile("int main(void) { return 1 +; }", "bad.c").expect_err("must fail");
        assert!(err.contains("bad.c"), "err: {err}");
    }
}
