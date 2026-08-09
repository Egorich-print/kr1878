//! End-to-end tests for the TESSA assembler.

use kr1878_assembler::{AssembleOutput, assemble};

fn img(src: &str) -> AssembleOutput {
    assemble(src).expect("assembly should succeed")
}

fn word(o: &AssembleOutput, addr: u16) -> u16 {
    o.image.program_word(addr).unwrap()
}

#[test]
fn empty_program() {
    let o = img("");
    assert_eq!(o.image.program[0], 0xFFFF);
}

#[test]
fn mov_two_operands_encodes() {
    let o = img("mov %a1,%d4\n");
    // opcode 000001 | src(%d4 = 0b11100 = 28) <<5 | dst(%a1=1)
    let src = 0b11100u16;
    let dst = 1u16;
    assert_eq!(word(&o, 0) >> 10, 0b000001);
    assert_eq!((word(&o, 0) >> 5) & 0x1F, src);
    assert_eq!(word(&o, 0) & 0x1F, dst);
}

#[test]
fn movl_eight_bit_immediate() {
    let o = img("movl d0,0FFh\n");
    assert_eq!(word(&o, 0) >> 13, 0b010);
    assert_eq!((word(&o, 0) >> 5) & 0xFF, 0xFF);
}

#[test]
fn branch_to_label_resolves_forward() {
    let o = img("jsr ska ; call\nnop\nska: rts\n");
    let w = word(&o, 0);
    assert_eq!(w >> 10, 0b1001); // JSR
    assert_eq!(w & 0x03FF, 2); // address of `ska:`
    assert_eq!(word(&o, 2), 0x000C); // RTS
}

#[test]
fn branch_local_basic() {
    let o = img("start:\ninc %a0\n$1: nop\njmp $1\n");
    // inc@0, nop@1 ($1), jmp@2 -> target 1
    let w = word(&o, 2);
    assert_eq!(w >> 10, 0b1000);
    assert_eq!(w & 0x03FF, 1);
}

#[test]
fn inc_dec_clr_expand() {
    let o = img("inc b3\ndec b3\nclr c0\nd0:\n");
    // inc = ADDL b3,1 ; dec = SUBL b3,1 ; clr = MOVL c0,0
    assert_eq!(word(&o, 0) >> 10, 0b001100);
    assert_eq!(word(&o, 1) >> 10, 0b001011);
    assert_eq!(word(&o, 2) >> 13, 0b010);
}

#[test]
fn constants_and_expressions() {
    let o = img("second = 2+3\nmovl %a0,second\n");
    assert_eq!((word(&o, 0) >> 5) & 0xFF, 5);
}

#[test]
fn bit_manipulation_place_bit() {
    let o = img("bich %d0,5\nbicl %d0,5\n");
    // place bit (bit9) = 1 for High
    assert_eq!(word(&o, 0) & 0x0200, 0x0200);
    assert_eq!(word(&o, 1) & 0x0200, 0);
}

#[test]
fn data_directives_step_program_counter() {
    let o = img(".byte 0ABh\n.word 1234h\n");
    assert_eq!(word(&o, 0) & 0xFF, 0xAB);
    assert_eq!(word(&o, 1), 0x1234);
}

#[test]
fn listing_has_lines() {
    let o = img("nop\n");
    assert!(o.listing.contains("nop"));
}

#[test]
fn even_directive_pads() {
    let o = img("movl %a0,1\n.even\nnop\n");
    // movl@0, pad 1..2 (addr1 unwritten), nop@2
    assert_eq!(word(&o, 1), 0xFFFF);
    assert_eq!(word(&o, 2), 0x0000);
}

#[test]
fn inc_then_even_pads() {
    // at pc=0 there are 2 instructions -> 2, already even
    let o = img("nop\nnop\n.even\nnop\n");
    assert_eq!(word(&o, 2), 0x0000);
}
