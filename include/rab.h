/* rab.h: the Record Access Block. VMS's field names, order and meaning,
 * with this host's pointers, so the layout is not VMS's byte for byte.
 * Must agree with crates/vms-c/src/rms.rs. */
#ifndef __RAB_LOADED
#define __RAB_LOADED 1
#include "rabdef.h"
#include "fab.h"

struct rabdef {
    unsigned char rab$b_bid;
    unsigned char rab$b_bln;
    unsigned short rab$w_isi;
    unsigned int rab$l_rop;
    unsigned int rab$l_sts;
    unsigned int rab$l_stv;
    union {
        unsigned short rab$w_rfa[3];
        struct {
            unsigned int rab$l_rfa0;
            unsigned short rab$w_rfa4;
        };
    };
    unsigned int rab$l_ctx;
    unsigned char rab$b_rac;
    unsigned char rab$b_tmo;
    unsigned short rab$w_usz;
    unsigned short rab$w_rsz;
    char *rab$l_ubf;
    char *rab$l_rbf;
    char *rab$l_rhb;
    char *rab$l_kbf;
    unsigned char rab$b_ksz;
    unsigned char rab$b_krf;
    unsigned char rab$b_mbf;
    unsigned char rab$b_mbc;
    unsigned int rab$l_bkt;
    struct fabdef *rab$l_fab;
    void *rab$l_xab;
};
#define RAB rabdef
/* The prompt buffer and the bucket count share the key's fields. */
#define rab$l_pbf rab$l_kbf
#define rab$b_psz rab$b_ksz
#define rab$l_dct rab$l_bkt

extern const struct rabdef cc$rms_rab;

#endif
