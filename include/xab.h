/* xab.h: the Extended Attribute Blocks vmsport takes (key, allocation,
 * dates, file header, protection, summary). VMS's field names and
 * meaning, with this host's pointers, so the layouts are not VMS's byte
 * for byte. Must agree with crates/vms-c/src/rms.rs. */
#ifndef __XAB_LOADED
#define __XAB_LOADED 1
#include "xabdef.h"

#define __XAB_HEAD \
    unsigned char xab$b_cod; \
    unsigned char xab$b_bln; \
    void *xab$l_nxt;

struct xabkeydef {
    __XAB_HEAD
    unsigned char xab$b_ian;
    unsigned char xab$b_lan;
    unsigned char xab$b_dan;
    unsigned char xab$b_lvl;
    unsigned char xab$b_ibs;
    unsigned char xab$b_dbs;
    unsigned char xab$b_flg;
    unsigned char xab$b_dtp;
    unsigned int xab$l_rvb;
    unsigned char xab$b_nsg;
    unsigned char xab$b_nul;
    unsigned char xab$b_tks;
    unsigned char xab$b_ref;
    unsigned short xab$w_mrl;
    unsigned short xab$w_ifl;
    unsigned short xab$w_dfl;
    union {
        unsigned short xab$w_pos[8];
        struct {
            unsigned short xab$w_pos0, xab$w_pos1, xab$w_pos2, xab$w_pos3,
                xab$w_pos4, xab$w_pos5, xab$w_pos6, xab$w_pos7;
        };
    };
    union {
        unsigned char xab$b_siz[8];
        struct {
            unsigned char xab$b_siz0, xab$b_siz1, xab$b_siz2, xab$b_siz3,
                xab$b_siz4, xab$b_siz5, xab$b_siz6, xab$b_siz7;
        };
    };
    unsigned char xab$b_prolog;
    char *xab$l_knm;
    unsigned int xab$l_dvb;
    union {
        unsigned char xab$b_typ[8];
        struct {
            unsigned char xab$b_typ0, xab$b_typ1, xab$b_typ2, xab$b_typ3,
                xab$b_typ4, xab$b_typ5, xab$b_typ6, xab$b_typ7;
        };
    };
};
#define XABKEY xabkeydef

struct xaballdef {
    __XAB_HEAD
    unsigned char xab$b_aop;
    unsigned char xab$b_aln;
    unsigned short xab$w_vol;
    unsigned int xab$l_loc;
    unsigned int xab$l_alq;
    unsigned short xab$w_deq;
    unsigned char xab$b_bkz;
    unsigned char xab$b_aid;
    unsigned short xab$w_rfi[3];
};
#define XABALL xaballdef

struct xabdatdef {
    __XAB_HEAD
    unsigned short xab$w_rvn;
    unsigned long long xab$q_cdt;
    unsigned long long xab$q_rdt;
    unsigned long long xab$q_edt;
    unsigned long long xab$q_bdt;
    unsigned long long xab$q_acc;
    unsigned long long xab$q_att;
    unsigned long long xab$q_mod;
};
#define XABDAT xabdatdef

struct xabfhcdef {
    __XAB_HEAD
    unsigned char xab$b_rfo;
    unsigned char xab$b_atr;
    unsigned short xab$w_lrl;
    unsigned int xab$l_hbk;
    unsigned int xab$l_ebk;
    unsigned short xab$w_ffb;
    unsigned char xab$b_bkz;
    unsigned char xab$b_hsz;
    unsigned short xab$w_mrz;
    unsigned short xab$w_dxq;
    unsigned short xab$w_gbc;
    unsigned short xab$w_verlimit;
    unsigned int xab$l_sbn;
};
#define XABFHC xabfhcdef

struct xabprodef {
    __XAB_HEAD
    unsigned short xab$w_pro;
    unsigned char xab$b_mtacc;
    unsigned char xab$b_prot_opt;
    union {
        unsigned int xab$l_uic;
        struct {
            unsigned short xab$w_mbm;
            unsigned short xab$w_grp;
        };
    };
};
#define XABPRO xabprodef

struct xabsumdef {
    __XAB_HEAD
    unsigned char xab$b_noa;
    unsigned char xab$b_nok;
    unsigned short xab$w_pvn;
};
#define XABSUM xabsumdef

extern const struct xabkeydef cc$rms_xabkey;
extern const struct xaballdef cc$rms_xaball;
extern const struct xabdatdef cc$rms_xabdat;
extern const struct xabfhcdef cc$rms_xabfhc;
extern const struct xabprodef cc$rms_xabpro;
extern const struct xabsumdef cc$rms_xabsum;

#endif
