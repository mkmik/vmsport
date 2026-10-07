/* lksbdef.h: values recorded on OpenVMS (fixtures/cabi), made by to_headers.py. */
#ifndef __LKSBDEF_LOADED
#define __LKSBDEF_LOADED 1

#define LKSB$K_LENGTH 24
#define LKSB$C_LENGTH 24

/* The lock status block $ENQ fills. */
struct lksb {
    unsigned short lksb$w_status;
    unsigned short lksb$w_reserved;
    unsigned int lksb$l_lkid;
    unsigned char lksb$b_valblk[16];
};
typedef struct lksb LKSB;

#endif
