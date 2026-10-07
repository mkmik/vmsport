/* starlet.h: the system services vmsport has. */
#ifndef __STARLET_LOADED
#define __STARLET_LOADED 1
#include "vms_args.h"

void sys$exit(unsigned int code) __attribute__((noreturn));
unsigned int sys$getmsg(unsigned int msgid, unsigned short *msglen, void *bufadr, unsigned int flags,
                        unsigned char outadr[4]);

#define sys$getmsg(...) (sys$getmsg)(_VMS_A5(__VA_ARGS__))

/* RMS: a block, then the optional ERR and SUC routines, which are called
 * with the block before the service returns. */
#define __RMS_SERVICE(name) \
    unsigned int name(void *block, void *err, void *suc);
__RMS_SERVICE(sys$open)
__RMS_SERVICE(sys$create)
__RMS_SERVICE(sys$close)
__RMS_SERVICE(sys$display)
__RMS_SERVICE(sys$erase)
__RMS_SERVICE(sys$parse)
__RMS_SERVICE(sys$search)
__RMS_SERVICE(sys$connect)
__RMS_SERVICE(sys$disconnect)
__RMS_SERVICE(sys$get)
__RMS_SERVICE(sys$find)
__RMS_SERVICE(sys$put)
__RMS_SERVICE(sys$update)
__RMS_SERVICE(sys$delete)
__RMS_SERVICE(sys$rewind)
__RMS_SERVICE(sys$free)
__RMS_SERVICE(sys$release)
#define sys$open(...) (sys$open)(_VMS_A3(__VA_ARGS__))
#define sys$create(...) (sys$create)(_VMS_A3(__VA_ARGS__))
#define sys$close(...) (sys$close)(_VMS_A3(__VA_ARGS__))
#define sys$display(...) (sys$display)(_VMS_A3(__VA_ARGS__))
#define sys$erase(...) (sys$erase)(_VMS_A3(__VA_ARGS__))
#define sys$parse(...) (sys$parse)(_VMS_A3(__VA_ARGS__))
#define sys$search(...) (sys$search)(_VMS_A3(__VA_ARGS__))
#define sys$connect(...) (sys$connect)(_VMS_A3(__VA_ARGS__))
#define sys$disconnect(...) (sys$disconnect)(_VMS_A3(__VA_ARGS__))
#define sys$get(...) (sys$get)(_VMS_A3(__VA_ARGS__))
#define sys$find(...) (sys$find)(_VMS_A3(__VA_ARGS__))
#define sys$put(...) (sys$put)(_VMS_A3(__VA_ARGS__))
#define sys$update(...) (sys$update)(_VMS_A3(__VA_ARGS__))
#define sys$delete(...) (sys$delete)(_VMS_A3(__VA_ARGS__))
#define sys$rewind(...) (sys$rewind)(_VMS_A3(__VA_ARGS__))
#define sys$free(...) (sys$free)(_VMS_A3(__VA_ARGS__))
#define sys$release(...) (sys$release)(_VMS_A3(__VA_ARGS__))

/* The lock manager: lksbdef.h's struct lksb, lckdef.h's modes and flags;
 * the resource name a descriptor. A twelfth argument is dropped. */
unsigned int sys$enq(unsigned int efn, unsigned int lkmode, void *lksb, unsigned int flags,
                     void *resnam, unsigned int parid, void *astadr, unsigned long astprm,
                     void *blkast, unsigned int acmode, unsigned int rsdm_id);
unsigned int sys$enqw(unsigned int efn, unsigned int lkmode, void *lksb, unsigned int flags,
                      void *resnam, unsigned int parid, void *astadr, unsigned long astprm,
                      void *blkast, unsigned int acmode, unsigned int rsdm_id);
unsigned int sys$deq(unsigned int lkid, void *valblk, unsigned int acmode, unsigned int flags);
#define sys$enq(...) (sys$enq)(_VMS_A11(__VA_ARGS__))
#define sys$enqw(...) (sys$enqw)(_VMS_A11(__VA_ARGS__))
#define sys$deq(...) (sys$deq)(_VMS_A4(__VA_ARGS__))

#endif
