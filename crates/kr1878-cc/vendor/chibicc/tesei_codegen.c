// tesei_codegen.c — code generator for the KR1878VE1 ("ТЕСЕЙ") core.
//
// The target is a Harvard 8-bit RISC with *no general-purpose registers*:
// every operand is a byte of the 256-byte data space addressed as
// SR[segment] + index. A conventional register allocator is therefore
// impossible; the model below is a memory machine with one accumulator.
//
// Storage model (see docs/adr/ADR-006-tesei-c-abi.md):
//
//   data space 0x40..0xBF  RAM (128 bytes)
//     0x40  %b0  scratch cell — the way to move a value into a service
//                register (`mtpr n, %b0`)
//     0x41  %b1  accumulator (ACC) — every expression leaves its value here
//     0x42  %b2  return-value cell (RET)
//     0x43..     variables: globals, then each function's frame
//
//   SR1 (segment B) is pinned to 0x40, so %b0/%b1/%b2 have fixed addresses.
//   SR0 is used as a temp register (T0); the data stack (DSP) is the value
//   stack for nested expressions and for call arguments.
//   IR0 (SR5:SR4) is the data-address register; SR5 is never written, so it
//   stays 0 and IR0 == SR4.
//
// Every variable has a *static* address: the core has no addressable stack
// (the data stack is 16 bytes and not addressable) and the call stack holds
// only return addresses, so recursion is impossible by construction.
//
// Instruction set used here (the subset our assembler accepts):
//   mov d,s   movl d,K   add/sub/and/or/xor d,s   addl/subl d,K
//   cmp d,s   cmpl d,K   neg d   not d   shl d   shr d
//   jmp/jz/jnz/js/jns/jc a   jsr a   rts   rti
//   ldr n,K   ldal #4,addr   mtpr n,s   mfpr d,#n   push #n   pop #n

#include "chibicc.h"

#define DATA_RAM_BASE 0x40
#define DATA_RAM_END 0xC0
#define FIRST_VAR 0x43

static FILE *out;
static Obj *current_fn;
static int label_seq;

// The address currently loaded into IR0, or -1 when unknown.
static int ir0_cached = -1;

__attribute__((format(printf, 1, 2)))
static void p(const char *fmt, ...) {
  va_list ap;
  va_start(ap, fmt);
  vfprintf(out, fmt, ap);
  va_end(ap);
  fprintf(out, "\n");
}

static int next_label(void) { return ++label_seq; }

// align_to(5, 8) returns 8 and align_to(11, 8) returns 16.
//
// chibicc defines this helper in its x86-64 code generator; the front end
// (parse.c) uses it for struct layout, so the TESEI backend has to provide
// it as well.
int align_to(int n, int align) {
  return (n + align - 1) / align * align;
}

// Turn an arbitrary C identifier into a symbol our assembler accepts:
// the first character must be a letter, the rest may be letters, digits or
// underscores.
static char *mangle(const char *name) {
  static char buf[256];
  int n = 0;
  const char *p0 = name;
  while (*p0 && n < (int)sizeof(buf) - 2) {
    char c = *p0++;
    if (c >= 'a' && c <= 'z')
      buf[n++] = c;
    else if (c >= 'A' && c <= 'Z')
      buf[n++] = c;
    else if (c >= '0' && c <= '9')
      buf[n++] = c;
    else
      buf[n++] = '_';
  }
  buf[n] = '\0';
  if (!(buf[0] >= 'a' && buf[0] <= 'z') && !(buf[0] >= 'A' && buf[0] <= 'Z')) {
    memmove(buf + 1, buf, strlen(buf) + 1);
    buf[0] = 'L';
  }
  return buf;
}

// ---------------------------------------------------------------- addressing

// Set IR0 to a compile-time constant data address.
static void ir0_set_const(int addr) {
  if (ir0_cached == addr)
    return;
  p("  ldal #4, %02xh", addr & 0xFF);
  ir0_cached = addr;
}

// Set IR0 from the accumulator (a run-time value).
static void ir0_set_acc(void) {
  p("  mtpr #0, %%b1"); // T0 = ACC
  p("  mfpr %%b1, #0"); // ACC = T0
  p("  mtpr #4, %%b1"); // SR4 = ACC  → IR0 = ACC
  ir0_cached = -1;
}

static void acc_to_scratch(void) { p("  mov %%b0, %%b1"); }
static void scratch_to_acc(void) { p("  mov %%b1, %%b0"); }

// ------------------------------------------------------------- value stack

static void push_acc(void) {
  p("  mtpr #0, %%b1"); // T0 = ACC
  p("  push #0");
}

static void pop_acc(void) {
  p("  pop #0");        // T0 = value
  p("  mfpr %%b1, #0"); // ACC = T0
}

// ------------------------------------------------------------------- layout

static int data_ptr = FIRST_VAR;

static int alloc_data(int size) {
  int addr = data_ptr;
  data_ptr += size;
  if (data_ptr > DATA_RAM_END)
    error("out of data memory: need %d bytes, %d available", size,
          DATA_RAM_END - FIRST_VAR);
  return addr;
}

// Assign a static data address to every variable and to each function's
// parameters and locals.
static void layout(Obj *prog) {
  for (Obj *var = prog; var; var = var->next) {
    if (var->is_function) {
      for (Obj *v = var->params; v; v = v->next)
        v->offset = alloc_data(v->ty->size);
      for (Obj *v = var->locals; v; v = v->next)
        v->offset = alloc_data(v->ty->size);
      continue;
    }
    var->offset = alloc_data(var->ty->size);
  }
}

// The static data address of an lvalue, or -1 when the address is only known
// at run time.
static int static_addr(Node *node) {
  switch (node->kind) {
  case ND_VAR:
    return node->var->offset;
  case ND_MEMBER: {
    int base = static_addr(node->lhs);
    if (base < 0)
      return -1;
    return base + node->member->offset;
  }
  default:
    return -1;
  }
}

// --------------------------------------------------------------- expressions

static void gen_expr(Node *node);
static void gen_stmt(Node *node);

// Load the byte at a static address into ACC.
static void load_static(int addr) {
  ir0_set_const(addr);
  p("  mov %%b1, %%d6");
}

// Store ACC to a static address.
static void store_static(int addr) {
  ir0_set_const(addr);
  p("  mov %%d6, %%b1");
}

// Compute the address of an lvalue into ACC.
static void gen_addr(Node *node) {
  switch (node->kind) {
  case ND_VAR:
    p("  movl %%b1, %02xh", node->var->offset & 0xFF);
    return;
  case ND_MEMBER: {
    int base = static_addr(node->lhs);
    if (base >= 0) {
      p("  movl %%b1, %02xh", (base + node->member->offset) & 0xFF);
      return;
    }
    gen_addr(node->lhs);
    p("  addl %%b1, %d", node->member->offset);
    return;
  }
  case ND_DEREF:
    // The pointer value *is* the address.
    gen_expr(node->lhs);
    return;
  case ND_COMMA:
    gen_expr(node->lhs);
    gen_addr(node->rhs);
    return;
  default:
    error_tok(node->tok, "not an lvalue");
  }
}

// Compute `lhs - rhs` and leave the flags set accordingly.
//
// The right operand is evaluated first and kept on the value stack, so both
// operands are available at the comparison.
static void gen_cmp_flags(Node *node) {
  Node *rhs = node->rhs;
  Node *lhs = node->lhs;

  if (rhs->kind == ND_NUM && is_integer(rhs->ty)) {
    gen_expr(lhs); // ACC = lhs
    long k = rhs->val;
    if (k >= 0 && k <= 255) {
      p("  cmpl %%b1, %d", (int)k);
    } else {
      p("  movl %%b0, %d", (int)(k & 0xFF));
      p("  cmp %%b1, %%b0");
    }
    return;
  }

  gen_expr(rhs); // ACC = rhs
  push_acc();
  gen_expr(lhs); // ACC = lhs
  acc_to_scratch(); // scratch = lhs
  pop_acc();        // ACC = rhs
  p("  cmp %%b0, %%b1"); // flags = lhs - rhs
}

// Branch to `label` when `node` (a comparison) holds.
//
// The ISA computes `d - s`, and the flags are Z (zero), S (sign) and C
// (carry). For signed 8-bit values:  lhs == rhs ⇔ Z,  lhs < rhs ⇔ S.
static void emit_cmp_jump(Node *node, const char *label, bool when_true) {
  gen_cmp_flags(node);
  switch (node->kind) {
  case ND_EQ:
    p(when_true ? "  jz %s" : "  jnz %s", label);
    return;
  case ND_NE:
    p(when_true ? "  jnz %s" : "  jz %s", label);
    return;
  case ND_LT:
    p(when_true ? "  js %s" : "  jns %s", label);
    return;
  case ND_LE: {
    // lhs <= rhs ⇔ S = 1 or Z = 1
    if (when_true) {
      p("  js %s", label);
      p("  jz %s", label);
      return;
    }
    int c = next_label();
    p("  jns Lchk%d", c); // S = 0 → check Z
    p("  jmp Lend%d", c); // S = 1 → the relation holds → no jump
    p("Lchk%d:", c);
    p("  jnz %s", label); // Z = 0 → the relation does not hold → jump
    p("Lend%d:", c);
    return;
  }
  default:
    error_tok(node->tok, "invalid comparison");
  }
}

// Materialise a boolean expression as 0 or 1 in ACC.
static void gen_bool_value(Node *node) {
  int c_true = next_label();
  int c_false = next_label();
  int c_end = next_label();
  char l_true[32], l_false[32], l_end[32];
  snprintf(l_true, sizeof(l_true), "Ltrue%d", c_true);
  snprintf(l_false, sizeof(l_false), "Lfalse%d", c_false);
  snprintf(l_end, sizeof(l_end), "Lend%d", c_end);

  switch (node->kind) {
  case ND_EQ:
  case ND_NE:
  case ND_LT:
  case ND_LE:
    emit_cmp_jump(node, l_true, true);
    p("  jmp %s", l_false);
    p("%s:", l_true);
    p("  movl %%b1, 1");
    p("  jmp %s", l_end);
    p("%s:", l_false);
    p("  movl %%b1, 0");
    p("%s:", l_end);
    return;

  case ND_NOT: {
    gen_bool_value(node->lhs);
    p("  cmpl %%b1, 0");
    p("  jz %s", l_true);
    p("  jmp %s", l_false);
    p("%s:", l_true);
    p("  movl %%b1, 1");
    p("  jmp %s", l_end);
    p("%s:", l_false);
    p("  movl %%b1, 0");
    p("%s:", l_end);
    return;
  }

  case ND_LOGAND:
    gen_bool_value(node->lhs);
    p("  cmpl %%b1, 0");
    p("  jz %s", l_false);
    gen_bool_value(node->rhs);
    p("  cmpl %%b1, 0");
    p("  jz %s", l_false);
    p("  movl %%b1, 1");
    p("  jmp %s", l_end);
    p("%s:", l_false);
    p("  movl %%b1, 0");
    p("%s:", l_end);
    return;

  case ND_LOGOR:
    gen_bool_value(node->lhs);
    p("  cmpl %%b1, 0");
    p("  jnz %s", l_true);
    gen_bool_value(node->rhs);
    p("  cmpl %%b1, 0");
    p("  jnz %s", l_true);
    p("  movl %%b1, 0");
    p("  jmp %s", l_end);
    p("%s:", l_true);
    p("  movl %%b1, 1");
    p("%s:", l_end);
    return;

  default:
    gen_expr(node);
    p("  cmpl %%b1, 0");
    p("  jz %s", l_false);
    p("  movl %%b1, 1");
    p("  jmp %s", l_end);
    p("%s:", l_false);
    p("  movl %%b1, 0");
    p("%s:", l_end);
    return;
  }
}

// Branch to `label` when the condition `node` holds (or does not hold, when
// `when_true` is false).
static void gen_cond_jump_ex(Node *node, const char *label, bool when_true) {
  switch (node->kind) {
  case ND_EQ:
  case ND_NE:
  case ND_LT:
  case ND_LE:
    emit_cmp_jump(node, label, when_true);
    return;
  case ND_NOT:
    // !x has the opposite truth value of x.
    gen_cond_jump_ex(node->lhs, label, !when_true);
    return;
  case ND_LOGAND: {
    // x && y is true only when both are true.
    if (when_true) {
      int c = next_label();
      char l_false[32];
      snprintf(l_false, sizeof(l_false), "Lfalse%d", c);
      gen_cond_jump_ex(node->lhs, l_false, false); // the left one is false
      gen_cond_jump_ex(node->rhs, label, true);   // the right one is true
      p("  jmp %s", l_false);
      p("%s:", l_false);
    } else {
      // The whole expression is false as soon as one operand is false.
      gen_cond_jump_ex(node->lhs, label, false);
      gen_cond_jump_ex(node->rhs, label, false);
    }
    return;
  }
  case ND_LOGOR: {
    // x || y is true when either is true.
    if (when_true) {
      gen_cond_jump_ex(node->lhs, label, true);
      gen_cond_jump_ex(node->rhs, label, true);
    } else {
      int c = next_label();
      char l_true[32];
      snprintf(l_true, sizeof(l_true), "Ltrue%d", c);
      gen_cond_jump_ex(node->lhs, l_true, true);
      gen_cond_jump_ex(node->rhs, l_true, true);
      p("  jmp %s", label);
      p("%s:", l_true);
    }
    return;
  }
  default:
    gen_expr(node);
    p("  cmpl %%b1, 0");
    p(when_true ? "  jnz %s" : "  jz %s", label);
    return;
  }
}

static void gen_cond_jump(Node *node, const char *label) {
  gen_cond_jump_ex(node, label, true);
}

// Emit `ACC op= rhs` where rhs is a constant.
static void gen_binop_const(Node *node, const char *op) {
  long k = node->rhs->val;
  gen_expr(node->lhs); // ACC = lhs

  if (strcmp(op, "add") == 0 && k >= 0 && k <= 31) {
    p("  addl %%b1, %d", (int)k);
    return;
  }
  if (strcmp(op, "sub") == 0 && k >= 0 && k <= 31) {
    p("  subl %%b1, %d", (int)k);
    return;
  }
  p("  movl %%b0, %d", (int)(k & 0xFF));
  if (strcmp(op, "sub") == 0) {
    // ACC = lhs - K: the scratch holds K, the accumulator the left operand.
    p("  sub %%b1, %%b0");
    return;
  }
  p("  %s %%b1, %%b0", op); // ACC = lhs op K
}

// Emit `ACC op= rhs` where rhs lives at a static address.
static void gen_binop_mem(Node *node, const char *op, int addr) {
  gen_expr(node->lhs); // ACC = lhs
  ir0_set_const(addr);
  p("  %s %%b1, %%d6", op);
}

static void gen_shift(Node *node) {
  Node *rhs = node->rhs;
  if (rhs->kind == ND_NUM) {
    long n = rhs->val;
    gen_expr(node->lhs);
    for (long i = 0; i < n; i++)
      p("  %s %%b1", node->kind == ND_SHL ? "shl" : "shr");
    return;
  }

  // Variable shift count: a loop of single-bit shifts.
  int c = next_label();
  gen_expr(node->rhs);
  p("  mov %%b0, %%b1"); // count
  gen_expr(node->lhs);
  p("Lbegin%d:", c);
  p("  cmpl %%b0, 0");
  p("  jz Lend%d", c);
  p("  %s %%b1", node->kind == ND_SHL ? "shl" : "shr");
  p("  subl %%b0, 1");
  p("  jmp Lbegin%d", c);
  p("Lend%d:", c);
}

static void gen_expr(Node *node) {
  switch (node->kind) {
  case ND_NULL_EXPR:
    return;

  case ND_NUM:
    p("  movl %%b1, %d", (int)(node->val & 0xFF));
    return;

  case ND_NEG:
    gen_expr(node->lhs);
    p("  neg %%b1");
    return;

  case ND_BITNOT:
    gen_expr(node->lhs);
    p("  not %%b1");
    return;

  case ND_VAR: {
    Type *ty = node->ty;
    if (ty->kind == TY_ARRAY || ty->kind == TY_FUNC) {
      p("  movl %%b1, %02xh", node->var->offset & 0xFF);
      return;
    }
    load_static(node->var->offset);
    return;
  }

  case ND_MEMBER: {
    int addr = static_addr(node);
    if (addr < 0)
      error_tok(node->tok, "struct member access needs a static address");
    load_static(addr);
    return;
  }

  case ND_DEREF: {
    int addr = static_addr(node);
    if (addr >= 0) {
      load_static(addr);
      return;
    }
    gen_expr(node->lhs); // ACC = pointer
    ir0_set_acc();
    p("  mov %%b1, %%d6");
    return;
  }

  case ND_ADDR: {
    int addr = static_addr(node->lhs);
    if (addr >= 0) {
      p("  movl %%b1, %02xh", addr & 0xFF);
      return;
    }
    gen_addr(node->lhs);
    return;
  }

  case ND_ASSIGN: {
    int addr = static_addr(node->lhs);
    gen_expr(node->rhs); // ACC = value
    if (addr >= 0) {
      acc_to_scratch();
      store_static(addr);
      scratch_to_acc();
      return;
    }
    // Dynamic address: the value is parked in the scratch cell while the
    // address is computed.
    acc_to_scratch();
    gen_addr(node->lhs); // ACC = address
    ir0_set_acc();
    p("  mov %%d6, %%b0"); // mem[addr] = value
    scratch_to_acc();
    return;
  }

  case ND_COMMA:
    gen_expr(node->lhs);
    gen_expr(node->rhs);
    return;

  case ND_CAST:
    gen_expr(node->lhs);
    if (node->lhs->ty->kind == TY_ARRAY || node->lhs->ty->kind == TY_FUNC)
      return;
    if (node->ty->kind == TY_LONG || node->lhs->ty->kind == TY_LONG)
      error_tok(node->tok, "`long` is not supported on TESEI (int is 8-bit)");
    return; // int and char are both one byte

  case ND_MEMZERO: {
    int addr = node->var->offset;
    for (int i = 0; i < node->var->ty->size; i++) {
      p("  movl %%b1, 0");
      store_static(addr + i);
    }
    return;
  }

  case ND_COND: {
    int c_else = next_label();
    int c_end = next_label();
    char l_else[32], l_end[32];
    snprintf(l_else, sizeof(l_else), "Lelse%d", c_else);
    snprintf(l_end, sizeof(l_end), "Lend%d", c_end);
    gen_cond_jump_ex(node->cond, l_else, false);
    gen_expr(node->then);
    p("  jmp %s", l_end);
    p("%s:", l_else);
    gen_expr(node->els);
    p("%s:", l_end);
    return;
  }

  case ND_NOT:
  case ND_LOGAND:
  case ND_LOGOR:
  case ND_EQ:
  case ND_NE:
  case ND_LT:
  case ND_LE:
    gen_bool_value(node);
    return;

  case ND_ADD:
  case ND_SUB:
  case ND_BITAND:
  case ND_BITOR:
  case ND_BITXOR: {
    const char *op = node->kind == ND_ADD      ? "add"
                     : node->kind == ND_SUB    ? "sub"
                     : node->kind == ND_BITAND ? "and"
                     : node->kind == ND_BITOR  ? "or"
                                               : "xor";
    if (node->rhs->kind == ND_NUM && is_integer(node->rhs->ty)) {
      gen_binop_const(node, op);
      return;
    }
    int addr = static_addr(node->rhs);
    if (addr >= 0) {
      gen_binop_mem(node, op, addr);
      return;
    }
    // General case: evaluate the right operand first and keep it on the
    // value stack; the left operand ends up in the scratch cell.
    gen_expr(node->rhs);
    push_acc();
    gen_expr(node->lhs);
    acc_to_scratch(); // scratch = lhs
    pop_acc();        // ACC = rhs
    if (strcmp(op, "sub") == 0) {
      // ACC = lhs - rhs: the scratch holds the left operand, the accumulator
      // the right one, so the subtraction has to go into the scratch.
      p("  sub %%b0, %%b1"); // scratch = lhs - rhs
      scratch_to_acc();
      return;
    }
    p("  %s %%b1, %%b0", op); // ACC = rhs op lhs
    return;
  }

  case ND_MUL:
  case ND_DIV:
  case ND_MOD:
    error_tok(node->tok,
              "`%s` is not implemented for TESEI yet (it needs a runtime "
              "helper)",
              node->kind == ND_MUL ? "*" : (node->kind == ND_DIV ? "/" : "%"));
    return;

  case ND_SHL:
  case ND_SHR:
    gen_shift(node);
    return;

  case ND_FUNCALL: {
    int nargs = 0;
    for (Node *arg = node->args; arg; arg = arg->next)
      nargs++;
    if (nargs > 15)
      error_tok(node->tok, "too many arguments (the data stack holds 16)");

    Node *args[16];
    int i = 0;
    for (Node *arg = node->args; arg && i < 16; arg = arg->next)
      args[i++] = arg;

    // Arguments are pushed in reverse order, so the callee pops them in
    // declaration order.
    for (int j = nargs - 1; j >= 0; j--) {
      gen_expr(args[j]);
      push_acc();
    }

    if (node->lhs->kind != ND_VAR)
      error_tok(node->tok, "indirect calls are not supported on TESEI");
    p("  jsr F_%s", mangle(node->lhs->var->name));
    ir0_cached = -1; // the callee may have changed IR0
    p("  mov %%b1, %%b2"); // ACC = return value
    return;
  }

  case ND_STMT_EXPR:
    for (Node *n = node->body; n; n = n->next)
      gen_stmt(n);
    return;

  default:
    error_tok(node->tok, "unsupported expression on TESEI");
  }
}

// --------------------------------------------------------------- statements

static void gen_stmt(Node *node) {
  switch (node->kind) {
  case ND_IF: {
    int c_else = next_label();
    int c_end = next_label();
    char l_else[32], l_end[32];
    snprintf(l_else, sizeof(l_else), "Lelse%d", c_else);
    snprintf(l_end, sizeof(l_end), "Lend%d", c_end);
    // Jump to the else branch when the condition does *not* hold.
    gen_cond_jump_ex(node->cond, l_else, false);
    gen_stmt(node->then);
    p("  jmp %s", l_end);
    p("%s:", l_else);
    if (node->els)
      gen_stmt(node->els);
    p("%s:", l_end);
    return;
  }

  case ND_FOR: {
    int c = next_label();
    if (node->init)
      gen_stmt(node->init);
    p("Lbegin%d:", c);
    if (node->cond)
      gen_cond_jump_ex(node->cond, mangle(node->brk_label), false);
    gen_stmt(node->then);
    p("%s:", mangle(node->cont_label));
    if (node->inc)
      gen_expr(node->inc);
    p("  jmp Lbegin%d", c);
    p("%s:", mangle(node->brk_label));
    return;
  }

  case ND_DO: {
    int c = next_label();
    p("Lbegin%d:", c);
    gen_stmt(node->then);
    p("%s:", mangle(node->cont_label));
    gen_expr(node->cond);
    p("  cmpl %%b1, 0");
    p("  jnz Lbegin%d", c);
    p("%s:", mangle(node->brk_label));
    return;
  }

  case ND_SWITCH: {
    gen_expr(node->cond);
    for (Node *n = node->case_next; n; n = n->case_next) {
      p("  cmpl %%b1, %d", (int)n->begin);
      p("  je %s", mangle(n->label));
    }
    if (node->default_case)
      p("  jmp %s", mangle(node->default_case->label));
    p("  jmp %s", mangle(node->brk_label));
    gen_stmt(node->then);
    p("%s:", mangle(node->brk_label));
    return;
  }

  case ND_CASE:
    p("%s:", mangle(node->label));
    gen_stmt(node->lhs);
    return;

  case ND_BLOCK:
    for (Node *n = node->body; n; n = n->next)
      gen_stmt(n);
    return;

  case ND_GOTO:
    p("  jmp %s", mangle(node->unique_label));
    return;

  case ND_LABEL:
    p("%s:", mangle(node->unique_label));
    gen_stmt(node->lhs);
    return;

  case ND_RETURN:
    if (node->lhs) {
      gen_expr(node->lhs);
      p("  mov %%b2, %%b1"); // RET = value
    }
    p("  jmp Lret_%s", mangle(current_fn->name));
    return;

  case ND_EXPR_STMT:
    gen_expr(node->lhs);
    return;

  default:
    error_tok(node->tok, "unsupported statement on TESEI");
  }
}

// ------------------------------------------------------------------ program

static void emit_text(Obj *prog) {
  for (Obj *fn = prog; fn; fn = fn->next) {
    if (!fn->is_function || !fn->is_definition)
      continue;

    current_fn = fn;
    p("F_%s:", mangle(fn->name));

    // Prologue: the arguments arrive on the data stack in declaration
    // order; move them into their static slots. The popped value has to go
    // through the accumulator — that is where `store_static` reads from.
    for (Obj *v = fn->params; v; v = v->next) {
      p("  pop #0");
      p("  mfpr %%b1, #0");
      store_static(v->offset);
    }

    gen_stmt(fn->body);

    p("Lret_%s:", mangle(fn->name));
    p("  rts");
    current_fn = NULL;
  }
}

// Emit the reset vector, the startup code and the global initialisers.
static void emit_startup(Obj *prog) {
  p("  jmp Lstart");
  for (int i = 1; i < 16; i++)
    p("  jmp Lisr");

  p("Lstart:");
  // SR[B] = 0x40 — pins the scratch window (%b0/%b1/%b2).
  p("  ldr #1, %02xh", DATA_RAM_BASE);

  // Global initialisers. Globals that the program does not reference (for
  // example the `__func__` string that chibicc creates for every function)
  // are dropped by the front end, so they must not occupy data memory.
  for (Obj *var = prog; var; var = var->next) {
    if (var->is_function || !var->init_data)
      continue;
    for (int i = 0; i < var->ty->size; i++) {
      p("  movl %%b1, %d", (unsigned char)var->init_data[i]);
      store_static(var->offset + i);
    }
  }

  p("  jsr F_main");
  p("Lspin:");
  p("  jmp Lspin");

  p("Lisr:");
  p("  rti");
}

void codegen(Obj *prog, FILE *fp) {
  out = fp;
  label_seq = 0;
  data_ptr = FIRST_VAR;
  ir0_cached = -1;

  layout(prog);
  emit_startup(prog);
  emit_text(prog);

  // The checksum is computed by the assembler over the whole image.
  p("  .endc");
}
