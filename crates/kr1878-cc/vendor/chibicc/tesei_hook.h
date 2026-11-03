// tesei_hook.h — error plumbing for the TESEI backend.
//
// chibicc's original error path prints to stderr and calls exit(1), which is
// fine for a command-line tool but fatal for a library. The vendored
// frontend is patched (see PATCHES.md) to route diagnostics through these
// hooks: the message is captured into a buffer and the failure is signalled
// with longjmp back to the library entry point.
#ifndef TESEY_HOOK_H
#define TESEY_HOOK_H

#include <setjmp.h>
#include <stdarg.h>

// Where the diagnostic is stored (a fixed buffer, filled by the first
// failing call).
extern char tesey_error_buf[2048];

// Longjmp target established by tesey_compile().
extern jmp_buf tesey_jmp;
extern int tesey_jmp_active;

// Records the diagnostic (overwrites any previous one).
void tesey_set_error(const char *fmt, ...);

// Aborts the compilation: longjmps when a jmp_buf is active, exits otherwise.
// Never returns.
_Noreturn void tesey_fail(void);

#endif // TESEY_HOOK_H
