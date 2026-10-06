#!/usr/bin/env python3
"""Records the fixtures on real OpenVMS Alpha V8.4-2L1, booted from its install
CD in AXPbox ("Execute DCL commands", no licence needed).

usage: record.py [--harvest] [RUNDIR]

--harvest skips the VMS run and only copies results out of RUNDIR/out.img.

The inputs (fixtures/{msg,fao,cld,rms,dcl}) go onto an ODS-5 volume that
`ods` makes (DKA100). VMS copies them to a volume it initializes itself
(DKA200), runs RECORD.COM there, and `ods` copies the results back into
fixtures/*/recorded/.

Needs vaxpunk next to this repo for the emulator, its firmware, the CD and
the console harness (run-vms.py). Environment: VAXPUNK (default ~/p/vaxpunk),
ODS (the ods binary, default `ods` on PATH).
"""
import json, os, pathlib, re, shutil, subprocess, sys, tempfile

FIX = pathlib.Path(__file__).resolve().parent.parent
VAXPUNK = pathlib.Path(os.environ.get("VAXPUNK", "~/p/vaxpunk")).expanduser()
PLAY = VAXPUNK / "real_vms_playground"
ODS = os.environ.get("ODS", "ods")

# Input files, copied to [T] under their upper-cased names.
INPUTS = {
    "msg": ["TESTMSG.MSG", "msg.com", "EXITWITH.COM", "SYSMSG.COM"],
    "fao": ["fao.com"],
    "cld": ["VPTEST.CLD", "CLIDUMP.MAR", "cld.com"],
    "rms": [p.name for p in sorted((FIX / "rms").glob("*.FDL"))] + ["rms.com"],
    "dcl": ["SYMBOLS.COM", "LEXICALS.COM", "CONTROL.COM", "NESTED.COM", "ERRORS.COM"],
}

RECORD_COM = """\
$ SET NOON
$ SET DEFAULT DKA200:[T]
$ @MSG.COM/OUTPUT=MSG.LOG
$ @SYSMSG.COM/OUTPUT=SYSMSG.LOG
$ @FAO.COM/OUTPUT=FAO.LOG
$ MACRO CLIDUMP.MAR
$ LINK CLIDUMP
$ @CLD.COM/OUTPUT=CLD.LOG
$ @RMS.COM/OUTPUT=RMS.LOG
$ @SYMBOLS.COM/OUTPUT=SYMBOLS.LOG
$ @LEXICALS.COM/OUTPUT=LEXICALS.LOG
$ @CONTROL.COM/OUTPUT=CONTROL.LOG
$ @ERRORS.COM/OUTPUT=ERRORS.LOG
$ DIRECTORY/SIZE DKA200:[T]
"""

CONSOLE = """\
MOUNT/OVERRIDE=IDENTIFICATION/NOWRITE DKA100:
INITIALIZE/STRUCTURE_LEVEL=5 DKA200: VPTOUT
MOUNT DKA200: VPTOUT
CREATE/DIRECTORY DKA200:[T]
COPY DKA100:[T]*.* DKA200:[T]
@DKA200:[T]RECORD.COM
DISMOUNT DKA200:
DISMOUNT DKA100:
"""

# Results: VMS name -> fixtures path. Text files come out one line per record.
TEXT_OUT = {
    "MSG.LOG": "msg/recorded/msg.log",
    "SYSMSG.LOG": "msg/recorded/sysmsg.log",
    "TESTMSG.SDL": "msg/recorded/TESTMSG.SDL",
    "TESTMSG.LIS": "msg/recorded/TESTMSG.LIS",
    "FAO.LOG": "fao/recorded/fao.log",
    "CLD.LOG": "cld/recorded/cld.log",
    "RMS.LOG": "rms/recorded/rms.log",
    "SYMBOLS.LOG": "dcl/recorded/SYMBOLS.log",
    "LEXICALS.LOG": "dcl/recorded/LEXICALS.log",
    "CONTROL.LOG": "dcl/recorded/CONTROL.log",
    "ERRORS.LOG": "dcl/recorded/ERRORS.log",
}
RMS_FILES = ["SEQVAR", "SEQVFC", "SEQSTM", "SEQLFSTM", "SEQCRSTM", "SEQFIX", "REL", "IDX", "IDXVAR"]

CFG = """\
sys0 = tsunami
{{
  memory.bits = 29;
  rom.srm = "{run}/rom/cl67srmrom.exe";
  rom.decompressed = "{run}/rom/decompressed.rom";
  rom.flash = "{run}/rom/flash.rom";
  rom.dpr = "{run}/rom/dpr.rom";
  cpu0 = ev68cb {{ speed = 800M; skip_memtest_hack = true; }}
  serial0 = serial {{ address = "127.0.0.1"; port = 21264; }}
  pci0.7 = ali {{ vga_console = false; }}
  pci0.15 = ali_ide {{}}
  pci0.19 = ali_usb {{}}
  pci0.3 = sym53c810
  {{
    disk0.1 = file {{ file = "{run}/in.img"; }}
    disk0.2 = file {{ file = "{run}/out.img"; }}
    disk0.4 = file {{ file = "{iso}"; cdrom = true; read_only = true; }}
  }}
}}
"""


def ods(*args, capture=False):
    r = subprocess.run([ODS, *map(str, args)], check=True, capture_output=capture, text=capture)
    return r.stdout


def blocks(img, spec):
    """A file's blocks up to its highwater mark, read through its map: VMS
    leaves the end of file at 0 in some indexed files, so `ods export`,
    which stops there, would copy nothing."""
    dump = ods("dump", img, spec, capture=True)
    high = int(re.search(r"^\s+highwater\s+(\d+)", dump, re.M).group(1)) - 1
    data = b""
    with open(img, "rb") as f:
        for count, lbn in re.findall(r"count=(\d+) format=\d+ lbn=(\d+)", dump):
            f.seek(int(lbn) * 512)
            data += f.read(int(count) * 512)
    return data[: high * 512]


def record(run):
    shutil.copytree(PLAY / "rom", run / "rom", dirs_exist_ok=True)
    (run / "es40.cfg").write_text(CFG.format(run=run, iso=(PLAY / "alpha.iso").resolve()))

    img = run / "in.img"
    img.unlink(missing_ok=True)
    ods("init", img, "--size", "100M", "--label", "VPTIN", "--ods5")
    ods("mkdir", img, "[T]")
    (run / "RECORD.COM").write_text(RECORD_COM)
    staged = [(run / "RECORD.COM", "RECORD.COM")]
    staged += [(FIX / d / f, f.upper()) for d, fs in INPUTS.items() for f in fs]
    for host, name in staged:
        ods("copy-in", img, host, f"[T]{name}", "--mode", "lines-to-records")
    with open(run / "out.img", "wb") as f:
        f.truncate(200 << 20)

    (run / "console.cmd").write_text(CONSOLE)
    env = dict(os.environ, AXPBOX=str(PLAY / "axpbox"), AXPBOX_CFG=str(run / "es40.cfg"), AXPBOX_BOOT="dka400")
    subprocess.run([sys.executable, VAXPUNK / "ods/vms/run-vms.py", run / "console.cmd", run / "console.log"],
                   env=env, check=True)


def harvest(run):
    out = run / "out.img"
    for name, dest in TEXT_OUT.items():
        (FIX / dest).parent.mkdir(parents=True, exist_ok=True)
        ods("copy-out", out, f"[T]{name}", FIX / dest, "--mode", "records-to-lines")
    shutil.rmtree(run / "export", ignore_errors=True)
    ods("export", out, "[T]", run / "export")
    rec = FIX / "rms/recorded"
    rec.mkdir(parents=True, exist_ok=True)
    for f in RMS_FILES:
        if f.startswith(("IDX", "REL")):
            (rec / f"{f}.DAT").write_bytes(blocks(out, f"[T]{f}.DAT"))
        else:
            shutil.copy(run / "export" / f"{f}.DAT;1", rec / f"{f}.DAT")
        for ext in ("ANL", "CHK"):
            ods("copy-out", out, f"[T]{f}.{ext}", rec / f"{f}.{ext}", "--mode", "records-to-lines")
    # The record attributes of the .DAT files, as `ods export` reports them.
    manifest = json.loads((run / "export/ods-manifest.json").read_text())
    manifest["entries"] = [e for e in manifest["entries"] if e["name"].endswith(".DAT")]
    (rec / "ods-manifest.json").write_text(json.dumps(manifest, indent=1) + "\n")
    print(f"recorded; run directory {run}")


def main():
    args = sys.argv[1:]
    only_harvest = "--harvest" in args
    args = [a for a in args if a != "--harvest"]
    run = pathlib.Path(args[0] if args else tempfile.mkdtemp(prefix="vpt-")).resolve()
    run.mkdir(parents=True, exist_ok=True)
    if not only_harvest:
        record(run)
    harvest(run)


if __name__ == "__main__":
    main()
