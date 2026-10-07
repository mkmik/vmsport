/* nam.h: the Name Block. VMS's field names and meaning, with this host's
 * pointers, so the layout is not VMS's byte for byte. Must agree with
 * crates/vms-c/src/rms.rs. */
#ifndef __NAM_LOADED
#define __NAM_LOADED 1
#include "namdef.h"

struct namdef {
    unsigned char nam$b_bid;
    unsigned char nam$b_bln;
    unsigned char nam$b_rss;
    unsigned char nam$b_rsl;
    unsigned int nam$l_fnb;
    struct namdef *nam$l_rlf;
    char *nam$l_rsa;
    char *nam$l_esa;
    unsigned char nam$b_ess;
    unsigned char nam$b_esl;
    unsigned char nam$b_nop;
    unsigned char nam$b_rfs;
    unsigned int nam$l_wcc;
    unsigned short nam$w_fid[3];
    unsigned short nam$w_did[3];
    char nam$t_dvi[16];
    unsigned char nam$b_node;
    unsigned char nam$b_dev;
    unsigned char nam$b_dir;
    unsigned char nam$b_name;
    unsigned char nam$b_type;
    unsigned char nam$b_ver;
    char *nam$l_node;
    char *nam$l_dev;
    char *nam$l_dir;
    char *nam$l_name;
    char *nam$l_type;
    char *nam$l_ver;
};
#define NAM namdef

extern const struct namdef cc$rms_nam;

#endif
