#!/usr/bin/env python3
"""Records on an installed OpenVMS Alpha V8.4-2L1 system, for what the
install CD that record.py boots can't do (EDIT/FDL).

usage: system.py AREA NAME...

Each NAME.dcl in fixtures/AREA is a command file for vaxpunk's run-vms.py
--system: one line typed per DCL prompt, and after a `CREATE file` line
the file's text up to `@@CTRLZ` (so a CREATE/FDL, say, must be in a
procedure; and a file whose last line starts with `$` must be the last one
created, or the next CREATE is taken for a failed one; and lines are
shorter than the 132-column console, or its echo falls behind). The console log
from the first command on goes to fixtures/AREA/recorded/NAME.log. run-vms.py boots a copy-on-write clone of
the system image, logs in as SYSTEM, and throws the clone away.

Environment: RUNVMS (vaxpunk's run-vms.py with --system), VMS_SYSTEM (the
installed system's disk image).
"""
import os, pathlib, subprocess, sys, tempfile

FIX = pathlib.Path(__file__).resolve().parent.parent
RUNVMS = os.environ.get("RUNVMS", "~/p/vaxpunk/.claude/worktrees/wild-whistling-flask/ods/vms/run-vms.py")
SYSTEM = os.environ.get("VMS_SYSTEM", "~/Library/Caches/vaxpunk/bliss-oracle/golden-sys.img")


def main():
    if len(sys.argv) < 3:
        sys.exit(__doc__)
    area = FIX / sys.argv[1]
    (area / "recorded").mkdir(exist_ok=True)
    for name in sys.argv[2:]:
        with tempfile.TemporaryDirectory() as tmp:
            log = pathlib.Path(tmp) / "console.log"
            # AXPbox's console now and then stops echoing what is typed: try again.
            for attempt in range(3):
                r = subprocess.run([sys.executable, os.path.expanduser(RUNVMS), "--system",
                                    os.path.expanduser(SYSTEM), area / f"{name}.dcl", log])
                if r.returncode == 0:
                    break
            else:
                sys.exit(f"{name}: run-vms.py failed")
            lines = log.read_text(encoding="utf-8").splitlines(keepends=True)
            first = (area / f"{name}.dcl").read_text().splitlines()[0]
            start = next(i for i, l in enumerate(lines) if l.rstrip("\n") == f"$ {first}")
            (area / "recorded" / f"{name}.log").write_text("".join(lines[start:]), encoding="utf-8")
        print(f"recorded {area.name}/recorded/{name}.log")


if __name__ == "__main__":
    main()
