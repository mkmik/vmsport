"""Writes include/{ssdef,climsgdef,libdef,stsdef,lckdef,lksbdef}.h,
vmsport's status and lock headers, from the values recorded on OpenVMS
(recorded/codes.log): names and numbers only, comments dropped."""
import pathlib, re

here = pathlib.Path(__file__).parent
inc = here.parent.parent / "include"
defs = {}
for line in (here / "recorded/codes.log").read_text().splitlines():
    m = re.match(r"#define (\S+)\s+(\S+)", line)
    if m:
        defs[m.group(1)] = m.group(2)

def header(name, prefix, extra=""):
    guard = f"__{name.upper()}_LOADED"
    lines = [f"/* {name}.h: values recorded on OpenVMS (fixtures/cabi), made by to_headers.py. */",
             f"#ifndef {guard}", f"#define {guard} 1", ""]
    lines += [f"#define {k} {v}" for k, v in defs.items() if k.startswith(prefix)]
    lines += [extra, "#endif", ""]
    (inc / f"{name}.h").write_text("\n".join(lines))

header("ssdef", "SS$_")
header("climsgdef", "CLI$_")
header("libdef", "LIB$_", "#define LIB$K_CLI_LOCAL_SYM 1\n#define LIB$K_CLI_GLOBAL_SYM 2\n")
header("stsdef", "STS$", """
#define $VMS_STATUS_SUCCESS(code) (((code) & STS$M_SUCCESS) >> STS$V_SUCCESS)
#define $VMS_STATUS_SEVERITY(code) (((code) & STS$M_SEVERITY) >> STS$V_SEVERITY)
#define $VMS_STATUS_FAC_NO(code) (((code) & STS$M_FAC_NO) >> STS$V_FAC_NO)
#define $VMS_STATUS_MSG_NO(code) (((code) & STS$M_MSG_NO) >> STS$V_MSG_NO)
#define $VMS_STATUS_COND_ID(code) (((code) & STS$M_COND_ID) >> STS$V_COND_ID)
#define $VMS_STATUS_INHIB_MSG(code) (((code) & STS$M_INHIB_MSG) >> STS$V_INHIB_MSG)
""")
header("lckdef", "LCK$")
header("lksbdef", "LKSB$", """
/* The lock status block $ENQ fills. */
struct lksb {
    unsigned short lksb$w_status;
    unsigned short lksb$w_reserved;
    unsigned int lksb$l_lkid;
    unsigned char lksb$b_valblk[16];
};
typedef struct lksb LKSB;
""")
print("headers written")
