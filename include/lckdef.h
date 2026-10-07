/* lckdef.h: values recorded on OpenVMS (fixtures/cabi), made by to_headers.py. */
#ifndef __LCKDEF_LOADED
#define __LCKDEF_LOADED 1

#define LCK$M_VALBLK 0x1
#define LCK$M_CONVERT 0x2
#define LCK$M_NOQUEUE 0x4
#define LCK$M_SYNCSTS 0x8
#define LCK$M_SYSTEM 0x10
#define LCK$M_NOQUOTA 0x20
#define LCK$M_CVTSYS 0x40
#define LCK$M_RECOVER 0x80
#define LCK$M_PROTECT 0x100
#define LCK$M_NODLCKWT 0x200
#define LCK$M_NODLCKBLK 0x400
#define LCK$M_EXPEDITE 0x800
#define LCK$M_QUECVT 0x1000
#define LCK$M_BYPASS 0x2000
#define LCK$M_NOIOLOCK8 0x4000
#define LCK$M_NOFORK 0x8000
#define LCK$M_XVALBLK 0x10000
#define LCK$M_DEQALL 0x1
#define LCK$M_CANCEL 0x2
#define LCK$M_INVVALBLK 0x4
#define LCK$M_RESV_NOIOLOCK8 0x4000
#define LCK$M_RESV_NOFORK 0x8000
#define LCK$M_RESV_XVALBLK 0x10000
#define LCK$K_NLMODE 0
#define LCK$K_CRMODE 1
#define LCK$K_CWMODE 2
#define LCK$K_PRMODE 3
#define LCK$K_PWMODE 4
#define LCK$K_EXMODE 5

#endif
