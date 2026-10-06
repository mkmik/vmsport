/* starlet.h: the system services vmsport has. */
#ifndef __STARLET_LOADED
#define __STARLET_LOADED 1
#include "vms_args.h"

void sys$exit(unsigned int code) __attribute__((noreturn));
unsigned int sys$getmsg(unsigned int msgid, unsigned short *msglen, void *bufadr, unsigned int flags,
                        unsigned char outadr[4]);

#define sys$getmsg(...) (sys$getmsg)(_VMS_A5(__VA_ARGS__))

#endif
