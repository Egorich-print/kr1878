//! Golden tests: C source → TESEI assembly → our assembler → the simulator.
//!
//! The compiler is checked against the *reference simulator*, so a wrong
//! code generation shows up as a wrong result rather than as a diff of the
//! generated text.

use kr1878_core::cpu::Cpu;

/// The address of the return-value cell (see ADR-006).
const RET: usize = 0x42;

/// Compiles, assembles and runs a C program; returns the CPU state.
fn run(source: &str) -> Cpu {
    let asm = kr1878_cc::compile(source, "golden.c")
        .unwrap_or_else(|e| panic!("compilation failed: {e}\n{source}"))
        .asm;
    let out =
        kr1878_assembler::assemble(&asm).unwrap_or_else(|e| panic!("assembly failed: {e}\n{asm}"));
    let mut cpu = Cpu::new();
    cpu.load_program(&out.image.program);
    cpu.reset();
    for _ in 0..50_000 {
        if let Err(e) = cpu.step() {
            panic!("fault: {e}");
        }
    }
    cpu
}

/// Compiles a program whose `main` returns a value and checks the result.
#[track_caller]
fn assert_returns(source: &str, expected: u8) {
    let cpu = run(source);
    assert_eq!(
        cpu.data[RET], expected,
        "expected {expected}, got {}\nprogram:\n{source}",
        cpu.data[RET]
    );
}

#[test]
fn returns_a_constant() {
    assert_returns("int main(void) { return 42; }", 42);
}

#[test]
fn computes_arithmetic() {
    // `*`, `/` and `%` are not implemented yet (they need a runtime helper),
    // so the arithmetic here sticks to what the backend supports.
    assert_returns("int main(void) { return 2 + 3 + 4; }", 9);
}

#[test]
fn keeps_local_variables() {
    assert_returns("int main(void) { int x = 7; int y = 5; return x - y; }", 2);
}

#[test]
fn uses_global_variables() {
    assert_returns("int g = 9; int main(void) { return g; }", 9);
}

#[test]
fn compares_values() {
    assert_returns("int main(void) { return 3 < 4; }", 1);
    assert_returns("int main(void) { return 4 < 3; }", 0);
    assert_returns("int main(void) { return 3 == 3; }", 1);
}

#[test]
fn branches_on_conditions() {
    assert_returns(
        "int main(void) { int x = 0; if (5 > 3) x = 1; else x = 2; return x; }",
        1,
    );
    assert_returns(
        "int main(void) { int x = 0; if (3 > 5) x = 1; else x = 2; return x; }",
        2,
    );
}

#[test]
fn loops_with_while() {
    assert_returns(
        "int main(void) { int i = 0; int s = 0; while (i < 5) { s = s + i; i = i + 1; } return s; }",
        10,
    );
}

#[test]
fn calls_functions_with_arguments() {
    assert_returns(
        "int add(int a, int b) { return a + b; }\nint main(void) { return add(3, 4); }",
        7,
    );
    assert_returns(
        "int twice(int a) { return a + a; }\nint main(void) { return twice(21); }",
        42,
    );
}

#[test]
fn nested_calls_keep_their_arguments() {
    assert_returns(
        "int inc(int a) { return a + 1; }\n\
         int add(int a, int b) { return a + b; }\n\
         int main(void) { return add(inc(1), inc(2)); }",
        5,
    );
}
