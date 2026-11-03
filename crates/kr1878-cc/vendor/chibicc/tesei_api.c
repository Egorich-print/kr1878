// tesei_api.c — library entry point for the TESEI backend.
//
// The vendored chibicc frontend is a command-line program; this file turns
// it into a library: the Rust wrapper calls tesey_compile() and gets either
// the generated assembly or a diagnostic.
//
// Patches applied to the vendored frontend are listed in PATCHES.md.

#include "chibicc.h"
#include "tesei_hook.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>

// --- globals the frontend expects from chibicc's original main.c ----------

bool opt_fcommon = true;
bool opt_fpic;
StringArray include_paths;
char *base_file;

// --- error plumbing -------------------------------------------------------

char tesey_error_buf[2048];
jmp_buf tesey_jmp;
int tesey_jmp_active = 0;

void tesey_set_error(const char *fmt, ...) {
  va_list ap;
  va_start(ap, fmt);
  vsnprintf(tesey_error_buf, sizeof(tesey_error_buf), fmt, ap);
  va_end(ap);
}

_Noreturn void tesey_fail(void) {
  if (tesey_jmp_active) {
    longjmp(tesey_jmp, 1);
  }
  fprintf(stderr, "%s\n", tesey_error_buf);
  exit(1);
}

bool file_exists(char *path) {
  struct stat st;
  return !stat(path, &st);
}

// --- the entry point ------------------------------------------------------

// Compiles `source` into TESEI assembly.
//
// Returns 0 on success and stores a malloc'd NUL-terminated assembly text in
// *out (the caller frees it). On failure it returns 1 and copies the
// diagnostic into `err`.
int tesey_compile(const char *source, const char *filename, char **out,
                  char *err, size_t err_len) {
  FILE *mem;
  char *buf = NULL;
  size_t len = 0;
  Token *tok;
  Obj *prog;

  *out = NULL;
  err[0] = '\0';

  // A memory stream keeps the generated assembly out of the file system.
  mem = open_memstream(&buf, &len);
  if (!mem) {
    snprintf(err, err_len, "cannot create the output stream");
    return 1;
  }

  if (setjmp(tesey_jmp) != 0) {
    // A diagnostic was recorded and the compilation was aborted.
    fclose(mem);
    snprintf(err, err_len, "%s", tesey_error_buf);
    free(buf);
    return 1;
  }
  tesey_jmp_active = 1;

  base_file = (char *)filename;
  parse_reset();

  // The tokenizer rewrites the buffer in place, so it needs a private,
  // mutable copy of the source.
  size_t src_len = strlen(source);
  char *contents = malloc(src_len + 2);
  if (!contents) {
    fclose(mem);
    snprintf(err, err_len, "out of memory");
    return 1;
  }
  memcpy(contents, source, src_len);
  if (src_len == 0 || contents[src_len - 1] != '\n')
    contents[src_len++] = '\n';
  contents[src_len] = '\0';

  File *file = new_file((char *)filename, 1, contents);
  tok = tokenize(file);
  // The preprocessor also converts identifiers into keywords, so it has to
  // run between tokenization and parsing (chibicc's own main.c does the
  // same).
  tok = preprocess(tok);
  prog = parse(tok);
  codegen(prog, mem);

  tesey_jmp_active = 0;
  fclose(mem);

  *out = buf;
  return 0;
}
