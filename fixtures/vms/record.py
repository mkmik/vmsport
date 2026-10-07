#!/usr/bin/env python3
"""Records fixtures on real OpenVMS Alpha V8.4-2L1, booted from its install
CD in AXPbox ("Execute DCL commands", no licence needed).

usage: record.py [--harvest] [--keep] [RUNDIR] AREA...

Each AREA is a directory under fixtures/ with a vms.txt that says what to do:

    in FILE              copy fixtures/AREA/FILE in, as [T.AREA]FILE (upcased)
    import DIR           copy the files of fixtures/AREA/DIR in as they are, with
                         the attributes its ods-manifest.json gives them
    run DCL-LINE         a line of DCL, run in DKA200:[T.AREA]
    text NAME DEST       copy [T.AREA]NAME out as text lines to fixtures/AREA/DEST
    blocks NAME DEST     copy it out as raw blocks, up to its highwater mark
    manifest GLOB DEST   write the record attributes of the matching files

The inputs go onto an ODS-5 volume `ods` makes (DKA100). VMS BACKUPs them to a
volume it initializes itself (DKA200), runs every area's lines there, and the
results come back out of it. Only one emulator runs at a time: record.py waits
for /tmp/vmsport-axpbox.lock.

--harvest skips the VMS run and only copies results out of RUNDIR/out.img.

This borrows a real-VMS setup from the vaxpunk project: its AXPbox emulator,
SRM firmware, the OpenVMS CD and its console script (run-vms.py), and the
`ods` tool for Files-11 images. Environment: VAXPUNK (default ~/p/vaxpunk),
ODS (the ods binary, default `ods` on PATH).
"""
import fcntl, fnmatch, json, os, pathlib, re, shutil, subprocess, sys, tempfile

FIX = pathlib.Path(__file__).resolve().parent.parent
VAXPUNK = pathlib.Path(os.environ.get("VAXPUNK", "~/p/vaxpunk")).expanduser()
PLAY = VAXPUNK / "real_vms_playground"
ODS = os.environ.get("ODS", "ods")

CONSOLE = """\
MOUNT/OVERRIDE=IDENTIFICATION/NOWRITE DKA100:
INITIALIZE/STRUCTURE_LEVEL=5 DKA200: VPTOUT
MOUNT DKA200: VPTOUT
BACKUP DKA100:[T...]*.*;* DKA200:[T...]*.*;*
@DKA200:[T]RECORD.COM
DISMOUNT DKA200:
DISMOUNT DKA100:
"""

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


def area_steps(area):
    """The (verb, rest) lines of fixtures/AREA/vms.txt."""
    steps = []
    for line in (FIX / area / "vms.txt").read_text().splitlines():
        line = line.strip()
        if line and not line.startswith("#"):
            verb, _, rest = line.partition(" ")
            steps.append((verb, rest.strip()))
    return steps


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


def record(run, areas):
    shutil.copytree(PLAY / "rom", run / "rom", dirs_exist_ok=True)
    (run / "es40.cfg").write_text(CFG.format(run=run, iso=(PLAY / "alpha.iso").resolve()))
    img = run / "in.img"
    img.unlink(missing_ok=True)
    ods("init", img, "--size", "100M", "--label", "VPTIN", "--ods5")
    ods("mkdir", img, "[T]")
    com = ["$ SET NOON"]
    for area in areas:
        d = area.upper()
        ods("mkdir", img, f"[T.{d}]")
        com.append(f"$ SET DEFAULT DKA200:[T.{d}]")
        for verb, rest in area_steps(area):
            if verb == "in":
                ods("copy-in", img, FIX / area / rest, f"[T.{d}]{rest.upper()}", "--mode", "lines-to-records")
            elif verb == "import":
                ods("import", img, FIX / area / rest, f"[T.{d}]")
            elif verb == "run":
                com.append(rest if rest.startswith("$") else "$ " + rest)
    com.append("$ DIRECTORY/SIZE DKA200:[T...]")
    (run / "RECORD.COM").write_text("\n".join(com) + "\n")
    ods("copy-in", img, run / "RECORD.COM", "[T]RECORD.COM", "--mode", "lines-to-records")
    with open(run / "out.img", "wb") as f:
        f.truncate(200 << 20)
    (run / "console.cmd").write_text(CONSOLE)
    env = dict(os.environ, AXPBOX=str(PLAY / "axpbox"), AXPBOX_CFG=str(run / "es40.cfg"), AXPBOX_BOOT="dka400")
    with open("/tmp/vmsport-axpbox.lock", "w") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        subprocess.run([sys.executable, VAXPUNK / "ods/vms/run-vms.py", run / "console.cmd", run / "console.log"],
                       env=env, check=True)


def harvest(run, areas):
    out = run / "out.img"
    shutil.rmtree(run / "export", ignore_errors=True)
    ods("export", out, "[T]", run / "export")
    manifest = json.loads((run / "export/ods-manifest.json").read_text())
    for area in areas:
        d = area.upper()
        for verb, rest in area_steps(area):
            if verb not in ("text", "blocks", "manifest"):
                continue
            name, dest = rest.split()
            dest = FIX / area / dest
            dest.parent.mkdir(parents=True, exist_ok=True)
            if verb == "text":
                ods("copy-out", out, f"[T.{d}]{name}", dest, "--mode", "records-to-lines")
            elif verb == "blocks":
                dest.write_bytes(blocks(out, f"[T.{d}]{name}"))
            else:
                entries = [e for e in manifest["entries"]
                           if e["path"].upper().startswith(f"{d}/") and fnmatch.fnmatch(e["name"], name)]
                m = dict(manifest, entries=entries)
                dest.write_text(json.dumps(m, indent=1) + "\n")
    print(f"recorded {', '.join(areas)}; run directory {run}")


def main():
    args = sys.argv[1:]
    only_harvest = "--harvest" in args
    args = [a for a in args if a != "--harvest"]
    run = None
    if args and not (FIX / args[0] / "vms.txt").exists():
        run = pathlib.Path(args.pop(0))
    areas = args
    if not areas:
        sys.exit(__doc__)
    run = (run or pathlib.Path(tempfile.mkdtemp(prefix="vpt-"))).resolve()
    run.mkdir(parents=True, exist_ok=True)
    if not only_harvest:
        record(run, areas)
    harvest(run, areas)


if __name__ == "__main__":
    main()
