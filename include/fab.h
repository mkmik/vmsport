/* fab.h: the File Access Block. VMS's field names, order and meaning, with
 * this host's pointers (8 bytes here), so the layout is not VMS's byte for
 * byte; FAB$C_BLN stays VMS's value. Must agree with crates/vms-c/src/rms.rs. */
#ifndef __FAB_LOADED
#define __FAB_LOADED 1
#include "fabdef.h"

struct fabdef {
    unsigned char fab$b_bid;
    unsigned char fab$b_bln;
    unsigned short fab$w_ifi;
    unsigned int fab$l_fop;
    unsigned int fab$l_sts;
    unsigned int fab$l_stv;
    unsigned int fab$l_alq;
    unsigned short fab$w_deq;
    unsigned char fab$b_fac;
    unsigned char fab$b_shr;
    unsigned int fab$l_ctx;
    unsigned char fab$b_rtv;
    unsigned char fab$b_org;
    unsigned char fab$b_rat;
    unsigned char fab$b_rfm;
    void *fab$l_jnl;
    void *fab$l_xab;
    void *fab$l_nam;
    char *fab$l_fna;
    char *fab$l_dna;
    unsigned char fab$b_fns;
    unsigned char fab$b_dns;
    unsigned short fab$w_mrs;
    unsigned int fab$l_mrn;
    unsigned short fab$w_bls;
    unsigned char fab$b_bks;
    unsigned char fab$b_fsz;
    unsigned int fab$l_dev;
    unsigned int fab$l_sdc;
    unsigned short fab$w_gbc;
    unsigned char fab$b_acmodes;
    unsigned char fab$b_rcf;
};
#define FAB fabdef

/* The prototype: struct FAB fab = cc$rms_fab; */
extern const struct fabdef cc$rms_fab;

#endif
