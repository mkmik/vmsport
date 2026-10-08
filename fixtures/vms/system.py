#!/usr/bin/env python3
"""Records on an installed OpenVMS Alpha V8.4-2L1 system, for what the
install CD that record.py boots can't do (EDIT/FDL).

usage: system.py AREA NAME...

Each NAME.dcl in fixtures/AREA is a command file for vaxpunk's run-vms.py
--system: one line typed per DCL prompt, and after a `CREATE file` line
the file's text up to `@@CTRLZ` (so a CREATE/FDL, say, must be in a
procedure; and a file whose last line starts with `$` must be the last one
created, or the next CREATE is taken for a failed one; and lines are
shorter than the 132-column console, or its echo falls behind).

For programs that want a terminal (the FDL editor refuses anything else),
`@@KEY text` types the text and a Return wherever the console is, and
`@@KEY ^Z` types Ctrl/Z; each waits until nothing has come out for a
while. The program is started the same way (`@@KEY EDIT/FDL X.FDL`), and
the next plain line goes to DCL again. `@@AUTO [VALUE...]` answers the FDL
editor's questions up to its main menu: the VALUEs (then 1000) where there
is no default, FD where a design asks for a parameter, Return elsewhere;
with stop=WORD it stops at the first question with WORD in it. `@@END`
types Ctrl/Z until DCL's prompt shows.

The console log from the first command on goes to
fixtures/AREA/recorded/NAME.log. run-vms.py boots a copy-on-write clone of
the system image, logs in as SYSTEM, and throws the clone away.

Environment: VAXPUNK (default ~/p/vaxpunk), RUNVMS (its run-vms.py, which
has --system), VMS_SYSTEM (the installed system's disk image), AXPBOX (the
emulator; default vaxpunk's real_vms_playground one, whose SRM ROMs are
copied beside each run so that runs can go side by side).
"""
import importlib.util, os, pathlib, re, shutil, signal, subprocess, sys, tempfile, time

FIX = pathlib.Path(__file__).resolve().parent.parent
VAXPUNK = pathlib.Path(os.environ.get("VAXPUNK", "~/p/vaxpunk")).expanduser()
PLAY = VAXPUNK / "real_vms_playground"
RUNVMS = pathlib.Path(os.environ.get("RUNVMS", VAXPUNK / "ods/vms/run-vms.py")).expanduser()
SYSTEM = os.environ.get("VMS_SYSTEM", "~/Library/Caches/vaxpunk/bliss-oracle/golden-sys.img")
QUIET = 2.0  # seconds of silence that end a @@KEY


def drive(runvms, system, cmdfile, log):
    """run-vms.py's run(), its console taught @@KEY."""
    spec = importlib.util.spec_from_file_location("runvms", runvms)
    m = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(m)
    dcl = m.Console.dcl

    def typed(self, text):
        self.send("\x1a" if text == "^Z" else text + "\r")
        last, quiet_since, end = len(self.buf), time.time(), time.time() + m.CMD_TIMEOUT
        while time.time() - quiet_since < QUIET:
            if time.time() > end:
                raise TimeoutError(f"no quiet after {text!r}")
            self.pump(0.25)
            if len(self.buf) != last:
                last, quiet_since = len(self.buf), time.time()

    def key(self, line):
        if line.startswith("@@KEY "):
            return typed(self, line[6:])
        if line == "@@END":
            # Ctrl/Z until DCL's prompt shows.
            for _ in range(6):
                if self.buf.rstrip(" ").endswith("\n$"):
                    return
                typed(self, "^Z")
            raise TimeoutError("the program would not end")
        if not line.startswith("@@AUTO"):
            return dcl(self, line)
        # The FDL editor's questions up to its main menu: for one with no
        # default, the value given for a word in it (Length=8), else 1000
        # within its range or the first keyword listed before it; FD to
        # finish a design; Return else. A question asked a third time in a
        # row gets Ctrl/Z.
        given = dict(v.split("=", 1) for v in line[6:].split())
        stop = given.pop("stop", None)
        seen = []
        for _ in range(200):
            lines = self.buf.rstrip(" ").split("\n")
            prompt = lines[-1]
            if "Main Editor Function" in prompt or prompt.endswith("$") or stop and stop in prompt:
                return
            seen = (seen + [prompt])[-3:]
            if len(seen) == 3 and len(set(seen)) == 1:
                typed(self, "^Z")
                continue
            word = next((v for k, v in given.items() if k in prompt), None)
            rng = re.search(r"\((\d+)-(\d+|2Giga)\)\[-\]", prompt)
            if "Mnemonic" in prompt:
                typed(self, "FD")
            elif "[-]" not in prompt:
                typed(self, "")
            elif word is not None:
                typed(self, word)
            elif rng:
                hi = 2**31 if rng[2] == "2Giga" else int(rng[2])
                typed(self, str(min(max(1000, int(rng[1])), hi)))
            else:
                listed = re.search(r"\(\s*(\w+)", lines[-2] if len(lines) > 1 else "")
                typed(self, listed[1] if listed else "1000")

    m.Console.dcl = key
    # A console that stops echoing (AXPbox's does now and then) fails the
    # run in minutes, not an hour, and main() tries again.
    m.CMD_TIMEOUT = 300
    # As run-vms.py's own main: a kill still stops the emulator and its clone.
    signal.signal(signal.SIGTERM, lambda *_: sys.exit("terminated"))
    m.run(pathlib.Path(cmdfile).read_text(encoding="utf-8").splitlines(), log, system)


def main():
    if sys.argv[1:2] == ["--drive"]:
        return drive(*sys.argv[2:])
    if len(sys.argv) < 3:
        sys.exit(__doc__)
    area = FIX / sys.argv[1]
    (area / "recorded").mkdir(exist_ok=True)
    for name in sys.argv[2:]:
        with tempfile.TemporaryDirectory() as tmp:
            log = pathlib.Path(tmp) / "console.log"
            # run-vms.py runs AXPbox in its own directory, from the ROMs there.
            runvms = pathlib.Path(tmp) / "run-vms.py"
            shutil.copy(RUNVMS, runvms)
            shutil.copytree(PLAY / "rom", pathlib.Path(tmp) / "rom")
            env = dict(os.environ, AXPBOX=os.environ.get("AXPBOX", str(PLAY / "axpbox")))
            # AXPbox's console now and then stops echoing what is typed: try again.
            for attempt in range(3):
                r = subprocess.run([sys.executable, __file__, "--drive", runvms,
                                    os.path.expanduser(SYSTEM), area / f"{name}.dcl", log], env=env)
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
