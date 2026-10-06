/* lib$routines.h: the run-time library routines vmsport has. */
#ifndef __LIB$ROUTINES_LOADED
#define __LIB$ROUTINES_LOADED 1
#include <stdarg.h>
#include <stdint.h>
#include "vms_args.h"

unsigned int lib$get_foreign(void *resultant, void *prompt, unsigned short *resultant_length, unsigned int *flags);
unsigned int lib$put_output(void *message_string);
unsigned int lib$get_symbol(void *symbol, void *resultant, unsigned short *resultant_length, unsigned int *table);
unsigned int lib$set_symbol(void *symbol, void *value, unsigned int *table);
unsigned int lib$delete_symbol(void *symbol, unsigned int *table);

#define lib$get_foreign(...) (lib$get_foreign)(_VMS_A4(__VA_ARGS__))
#define lib$get_symbol(...) (lib$get_symbol)(_VMS_A4(__VA_ARGS__))
#define lib$set_symbol(...) (lib$set_symbol)(_VMS_A3(__VA_ARGS__))
#define lib$delete_symbol(...) (lib$delete_symbol)(_VMS_A2(__VA_ARGS__))

/* lib$signal(condition [, fao_count, fao_args...] [, condition ...]) and
 * lib$stop: variadic in C only. This wrapper collects the arguments as
 * pointer-sized words up to a marker the macro adds, and hands them to
 * vms$signal, which reads each FAO argument as its directive says (a
 * number is the low 32 bits of its word, !AS a descriptor's address). */
unsigned int vms$signal(int stop, const intptr_t *words, int count);
#define VMS$END_OF_ARGS ((intptr_t)0x5EA1ED0DDEADBEEFLL)
static inline unsigned int vms$signal_(int stop, unsigned int condition, ...) {
    intptr_t w[64];
    int n = 0;
    va_list ap;
    w[n++] = (intptr_t)condition;
    va_start(ap, condition);
    for (;;) {
        intptr_t x = va_arg(ap, intptr_t);
        if (x == VMS$END_OF_ARGS || n == 64) break;
        w[n++] = x;
    }
    va_end(ap);
    return vms$signal(stop, w, n);
}
#define lib$signal(...) vms$signal_(0, __VA_ARGS__, VMS$END_OF_ARGS)
#define lib$stop(...) vms$signal_(1, __VA_ARGS__, VMS$END_OF_ARGS)

#endif
