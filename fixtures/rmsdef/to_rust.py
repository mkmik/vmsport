"""Writes crates/vms-rms/src/status.rs, RMS's condition values, from what
OpenVMS's headers define (recorded/rmsdef.log)."""
import pathlib, re

here = pathlib.Path(__file__).parent
out = here.parent.parent / "crates/vms-rms/src/status.rs"
lines = ["//! RMS's condition values, as OpenVMS defines them (fixtures/rmsdef).",
         "//! Made by fixtures/rmsdef/to_rust.py.", "", "use vms_cond::Cond;", ""]
seen = set()
for line in (here / "recorded/rmsdef.log").read_text().splitlines():
    m = re.match(r"#define RMS\$_(\w+)\s+(-?\d+)\s*$", line)
    if m and m.group(1) not in seen and m.group(1) != "FACILITY":
        seen.add(m.group(1))
        lines.append(f"pub const {m.group(1)}: Cond = Cond({int(m.group(2)) & 0xFFFFFFFF});")
out.write_text("\n".join(lines) + "\n")
print(f"{out}: {len(seen)} codes")
