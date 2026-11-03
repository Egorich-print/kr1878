// Regression tests for the mnemonics that end in `l`/`h`: the bit-manipulation
// guard must not swallow them.
#[test]
fn service_register_and_suffix_mnemonics() {
    for src in [
        "push #0",
        "pop #0",
        "shl %b1",
        "shr %b1",
        "shra %b1",
        "btgl %a1, 3",
        "bisl %a1, 3",
    ] {
        assert!(
            kr1878_assembler::assemble(src).is_ok(),
            "`{src}` must assemble"
        );
    }
}
