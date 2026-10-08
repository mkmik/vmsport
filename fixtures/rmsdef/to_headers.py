"""Writes include/{rmsdef,fabdef,rabdef,namdef,xabdef}.h, the RMS constants
vmsport's C headers use, from the values recorded on OpenVMS
(recorded/rmsdef.log): names and numbers only, comments dropped. The
blocks themselves are in include/{fab,rab,nam,xab}.h, written by hand."""
import pathlib, re

here = pathlib.Path(__file__).parent
inc = here.parent.parent / "include"
defs = {}
for line in (here / "recorded/rmsdef.log").read_text().splitlines():
    m = re.match(r"#define ((?:FAB|RAB|NAM|XAB|RMS)\$\w+)\s+(-?(?:0x[0-9A-Fa-f]+|\d+))\b", line)
    if m:
        defs.setdefault(m.group(1), m.group(2))

def header(name, prefix):
    guard = f"__{name.upper()}_LOADED"
    lines = [f"/* {name}.h: values recorded on OpenVMS (fixtures/rmsdef), made by to_headers.py. */",
             f"#ifndef {guard}", f"#define {guard} 1", ""]
    lines += [f"#define {k} {v}" for k, v in defs.items() if k.startswith(prefix) and k != "RMS$_FACILITY"]
    lines += ["", "#endif", ""]
    (inc / f"{name}.h").write_text("\n".join(lines))

for name, prefix in [("rmsdef", "RMS$_"), ("fabdef", "FAB$"), ("rabdef", "RAB$"),
                     ("namdef", "NAM$"), ("xabdef", "XAB$")]:
    header(name, prefix)
print("headers written")
