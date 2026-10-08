/* stsdef.h: values recorded on OpenVMS (fixtures/cabi), made by to_headers.py. */
#ifndef __STSDEF_LOADED
#define __STSDEF_LOADED 1

#define STS$M_SEVERITY 0x7
#define STS$M_COND_ID 0xFFFFFF8
#define STS$M_CONTROL 0xF0000000
#define STS$M_SUCCESS 0x1
#define STS$M_MSG_NO 0xFFF8
#define STS$M_CODE 0x7FF8
#define STS$M_FAC_SP 0x8000
#define STS$M_CUST_DEF 0x8000000
#define STS$M_INHIB_MSG 0x10000000
#define STS$M_FAC_NO 0xFFF0000
#define STS$K_WARNING 0
#define STS$K_SUCCESS 1
#define STS$K_ERROR 2
#define STS$K_INFO 3
#define STS$K_SEVERE 4
#define STS$S_CODE 0x0C
#define STS$S_COND_ID 0x19
#define STS$S_CONTROL 0x04
#define STS$S_FAC_NO 0x0C
#define STS$S_MSG_NO 0x0D
#define STS$S_SEVERITY 0x03
#define STS$V_CODE 0x03
#define STS$V_COND_ID 0x03
#define STS$V_CONTROL 0x1C
#define STS$V_CUST_DEF 0x1B
#define STS$V_FAC_NO 0x10
#define STS$V_FAC_SP 0x0F
#define STS$V_INHIB_MSG 0x1C
#define STS$V_MSG_NO 0x03
#define STS$V_SEVERITY 0x00
#define STS$V_SUCCESS 0x00

#define $VMS_STATUS_SUCCESS(code) (((code) & STS$M_SUCCESS) >> STS$V_SUCCESS)
#define $VMS_STATUS_SEVERITY(code) (((code) & STS$M_SEVERITY) >> STS$V_SEVERITY)
#define $VMS_STATUS_FAC_NO(code) (((code) & STS$M_FAC_NO) >> STS$V_FAC_NO)
#define $VMS_STATUS_MSG_NO(code) (((code) & STS$M_MSG_NO) >> STS$V_MSG_NO)
#define $VMS_STATUS_COND_ID(code) (((code) & STS$M_COND_ID) >> STS$V_COND_ID)
#define $VMS_STATUS_INHIB_MSG(code) (((code) & STS$M_INHIB_MSG) >> STS$V_INHIB_MSG)

#endif
