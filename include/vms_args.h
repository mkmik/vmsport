/* vms_args.h: optional trailing arguments for vmsport's VMS routines.
 * Each routine has a fixed C prototype; a macro of the same name pads
 * the arguments a call leaves out with 0, as VMS passes them omitted:
 * lib$get_foreign(&line) calls lib$get_foreign(&line, 0, 0, 0). */
#ifndef __VMS_ARGS_LOADED
#define __VMS_ARGS_LOADED 1

#define _VMS_A2_(a, b, ...) a, b
#define _VMS_A3_(a, b, c, ...) a, b, c
#define _VMS_A4_(a, b, c, d, ...) a, b, c, d
#define _VMS_A5_(a, b, c, d, e, ...) a, b, c, d, e
#define _VMS_A2(...) _VMS_A2_(__VA_ARGS__, 0, 0)
#define _VMS_A3(...) _VMS_A3_(__VA_ARGS__, 0, 0, 0)
#define _VMS_A4(...) _VMS_A4_(__VA_ARGS__, 0, 0, 0, 0)
#define _VMS_A5(...) _VMS_A5_(__VA_ARGS__, 0, 0, 0, 0, 0)
#define _VMS_A11_(a, b, c, d, e, f, g, h, i, j, k, ...) a, b, c, d, e, f, g, h, i, j, k
#define _VMS_A11(...) _VMS_A11_(__VA_ARGS__, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0)

#endif
