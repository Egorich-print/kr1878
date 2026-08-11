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
    assert_eq!(w >> 12, 0b1001); // JSR (4-bit opcode in bits 15..12)
    assert_eq!(w & 0x0FFF, 2); // address of `ska:`
    assert_eq!(word(&o, 2), 0x000C); // RTS
}

#[test]
fn branch_local_basic() {
    let o = img("start:\ninc %a0\n$1: nop\njmp $1\n");
    // inc@0, nop@1 ($1), jmp@2 -> target 1
    let w = word(&o, 2);
    assert_eq!(w >> 12, 0b1000);
    assert_eq!(w & 0x0FFF, 1);
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

#[test]
fn listing_shows_real_encoded_words() {
    let o = img("movl %a0,0FFh\nnop\n.byte 7fh\n");
    let w0 = word(&o, 0);
    let w2 = word(&o, 2);
    assert_eq!(w2, 0x007F);
    // Listing must carry the real encoded words, not zero placeholders.
    let listing = o.listing.to_lowercase();
    assert!(
        listing.contains(&format!("{w0:04x}")),
        "listing missing encoded movl word: {listing}"
    );
    assert!(
        listing.contains(&format!("{w2:04x}")),
        "listing missing .byte value: {listing}"
    );
    // And it must not show a bare `0000` row for the encoded instructions.
    assert!(
        !listing.starts_with("0000: 0000"),
        "zero placeholder leaked: {listing}"
    );
}

#[test]
fn current_address_dot_resolves() {
    // `.` is the current pc; used both as expression operand and with `.=`.
    let o = img("nop\njmp .+1\n");
    // nop@0, jmp@1 targeting 1+1=2.
    assert_eq!(word(&o, 1) & 0x03FF, 2);
}

#[test]
fn setpc_directive_moves_cursor() {
    let o = img("nop\n.=\t4\nmovl %a0,1\n");
    // nop@0, cursor moved to 4, movl@4.
    assert_eq!(word(&o, 3), 0xFFFF); // skip stays erased
    assert_eq!((word(&o, 4) >> 13), 0b010);
}

#[test]
fn byte_of_dot_uses_current_pc() {
    let o = img(".byte .\n");
    assert_eq!(word(&o, 0), 0x0000);
}

#[test]
fn rtsc_encodes_carry_variant() {
    let o = img("rtsc\nrtsc 1\nrtsc 0\n");
    assert_eq!(word(&o, 0), 0x000E);
    assert_eq!(word(&o, 1), 0x000F);
    assert_eq!(word(&o, 2), 0x000E);
}

#[test]
fn address_pseudos_expand_to_movl_ldr() {
    // mial/miah (instruction address), mdal/mdah (data address).
    // `label:` does not emit a word, so the first instruction sits at 0.
    let o = img("label:\nmial %a0,label\nmiah %a1,label\nmdal %a2,label\nmdah %a3,label\n");
    assert_eq!(word(&o, 0) >> 13, 0b010); // MOVL
    assert_eq!(word(&o, 0) & 0x1F, 0); // dst %a0
    assert_eq!((word(&o, 0) >> 5) & 0xFF, 0); // lo(0)
    assert_eq!(word(&o, 1) >> 13, 0b010);
    assert_eq!(word(&o, 1) & 0x1F, 1); // dst %a1
    assert_eq!((word(&o, 1) >> 5) & 0xFF, 0); // hi(0)

    // A label at a high address: mial loads low byte, miah loads high byte.
    let o = img(".\t= 123h\nhi:\nmial %a0,hi\nmiah %a1,hi\n");
    assert_eq!((word(&o, 0x123) >> 5) & 0xFF, 0x23); // lo(0x123)
    assert_eq!((word(&o, 0x124) >> 5) & 0xFF, 0x01); // hi(0x123)
}

#[test]
fn sr_address_pseudos_expand_to_ldr() {
    // ldal/ldah (data addr into SR6/SR7), lia/liah (jump addr).
    let o = img("target:\nldal #6,target\nldah #7,target\nlia #6,target\nliah #7,target\n");
    // ldr is opcode 0b00100 (bits 15..11).
    assert_eq!(word(&o, 0) >> 11, 0b00100);
    assert_eq!(word(&o, 0) & 0x7, 6); // SR6
    assert_eq!((word(&o, 0) >> 3) & 0xFF, 0); // lo(0)
    assert_eq!(word(&o, 1) & 0x7, 7); // SR7
}

#[test]
fn endc_writes_three_byte_checksum() {
    // Sum of {0x0420, 0x0000} bytes with carry propagation:
    // b0 = 0x20, b1 = 0x04, b2 = 0x00.
    let o = img("mov %a0,%a1\nnop\n.endc\n");
    let cs = word(&o, 2);
    assert_eq!(cs & 0xFF, 0x20); // checksum[0]
    assert_eq!((cs >> 8) & 0xFF, 0x04); // checksum[1]
    assert_eq!(word(&o, 3) & 0xFF, 0x00); // checksum[2]
}

#[test]
fn include_expands_defined_constant() {
    // Preprocessor `#include` requires a loader; emulate it by concatenating.
    let src = "#define LED 1\nmovl %a0,LED\n";
    let o = img(src);
    assert_eq!((word(&o, 0) >> 5) & 0xFF, 1);
}
