# Patches to the vendored chibicc

The files under `crates/kr1878-cc/vendor/chibicc/` come from
[chibicc](https://github.com/rui314/chibicc) by Rui Ueyama (MIT, see
`LICENSE.chibicc`). We keep the front end unmodified as far as possible and
apply the smallest possible changes, each of them listed here.

The x86-64 code generator (`codegen.c`) is **not** vendored: it is replaced by
our own TESEI back end, `tesei_codegen.c`. `main.c` is not vendored either —
the library entry point is `tesei_api.c`.

## Patches to the front end

### 1. `tokenize.c` — diagnostics instead of `exit(1)`

chibicc reports errors by printing to stderr and calling `exit(1)`. As a
library that would kill the host process, so the error path now goes through
`tesey_hook.h`:

* `verror_at()` formats the diagnostic into `tesey_error_buf` instead of
  printing it;
* `error()`, `error_at()` and `error_tok()` call `tesey_fail()`, which
  `longjmp`s back to `tesey_compile()`;
* `warn_tok()` still prints to stderr — a warning must not abort the
  compilation nor overwrite the error buffer.

### 2. `parse.c` — `parse_reset()`

The original program compiles once per process, so the global scope never had
to be cleared. As a library it does: without the reset the second compilation
in the same process reports `redefinition of main`.

`parse_reset()` frees the buckets of the outermost scope and starts from a
fresh global scope.

### 3. `parse.c` — no `__func__` / `__FUNCTION__`

chibicc defines `__func__` and `__FUNCTION__` for every function as a string
literal. On TESEI the data memory is 128 bytes in total, and those strings
would take a quarter of it for nothing. The definition is skipped, so the two
names are not available in C programs (documented in ADR-006).

## Files added by us

| File | Purpose |
|---|---|
| `tesei_codegen.c` | the TESEI back end (replaces chibicc's `codegen.c`) |
| `tesei_api.c` | library entry point, globals the front end expects from `main.c` (`file_exists`, `base_file`, `include_paths`, …) |
| `tesei_hook.h` | error plumbing (`setjmp`/`longjmp`) |
