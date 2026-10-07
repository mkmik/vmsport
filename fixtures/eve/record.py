#!/usr/bin/env python3
"""Records EVE on an installed OpenVMS Alpha V8.4-2L1 (the install CD has
no EDIT/TPU): the batch cases of batch.txt into recorded/batch-console.log
and the keyed sessions of sessions.txt into recorded/sessions-console.log;
screens.py turns those into recorded/batch.log and recorded/screens.txt.

usage: record.py [batch|sessions]   (default: both, one boot each)
       record.py sessions NAME...   (just those, added to recorded/again-console.log)

It boots vaxpunk's installed system image the way fixtures/vms/system.py
does (run-vms.py --system: a copy-on-write clone, thrown away after), and
adds what EVE needs: after a session's EDIT command, its keys are typed one
by one (escape sequences whole), each once the screen has settled (TPU
doesn't paint while keys wait), a \\x1e marking in the log where each
key's output ends; a session stuck at a prompt gets Ctrl/Z until DCL's
prompt is back. AXPbox now and then crashes an image with an ACCVIO
(any image: EVE, SET TERMINAL, PURGE): such a command, or session, runs
again. Lines of the console log keep their CRs.

Environment: RUNVMS (vaxpunk's run-vms.py), VMS_SYSTEM (the system image),
AXPBOX (the emulator), AXPBOX_ROM (its ROM directory).
"""
import importlib.util, os, pathlib, shutil, signal, sys, tempfile, time

HERE = pathlib.Path(__file__).resolve().parent
home = os.path.expanduser
RUNVMS = home(os.environ.get("RUNVMS", "~/p/vaxpunk/ods/vms/run-vms.py"))
SYSTEM = home(os.environ.get("VMS_SYSTEM", "~/Library/Caches/vaxpunk/bliss-oracle/golden-sys.img"))
AXPBOX = home(os.environ.get("AXPBOX", "~/p/vaxpunk/real_vms_playground/axpbox"))
ROM = home(os.environ.get("AXPBOX_ROM", "~/p/vaxpunk/real_vms_playground/rom"))
QUIET = 1.0


def rows(name):
    return [l.split("\t") for l in (HERE / name).read_text().splitlines() if l and not l.startswith("#")]


def commands(part, names=()):
    c = ["SET DEFAULT SYS$SYSDEVICE:[000000]", "CREATE/DIRECTORY [EVE]", "SET DEFAULT SYS$SYSDEVICE:[EVE]"]
    for f in sorted((HERE / "files").iterdir()):
        if f.name == "L.TXT":
            continue
        c += [f"CREATE {f.stem}.ORI", *f.read_text().splitlines(), "@@CTRLZ"]
    batch = rows("batch.txt")
    for i, (name, cmd, init, _) in enumerate(batch):
        if init != "-":
            c += [f"CREATE I{i}.EVE", *[l.strip() for l in init.split(" | ")], "@@CTRLZ"]
    # L.TXT's 60 lines, and the batch cases, from procedures (the last
    # file created may end in a $ line).
    c += ["CREATE MKL.COM", "$ OPEN/WRITE f L.ORI", "$ n = 1", "$ l:", '$ WRITE f "line ", n',
          "$ n = n + 1", "$ IF n .LE. 60 THEN GOTO l", "$ CLOSE f", "@@CTRLZ", "@MKL"]
    if part == "sessions":
        c.append("SET TERMINAL/DEVICE=VT200/NOEIGHTBIT/WIDTH=80/PAGE=24")
        c += [f"@@SESSION {name}\t{cmd}\t{keys}" for name, cmd, keys in rows("sessions.txt")
              if not names or name in names]
        c.append('WRITE SYS$OUTPUT "@@ end"')
        return c
    c += ["CREATE B.COM", "$ SET NOON"]
    for i, (name, cmd, init, show) in enumerate(batch):
        c += ["$ DELETE/NOLOG *.TXT;*", "$ COPY/NOLOG *.ORI *.TXT", f'$ WRITE SYS$OUTPUT "@@ {name}"']
        if "/INITIALIZATION=" not in cmd and init != "-":
            c.append(f"$ DEFINE EVE$INIT I{i}.EVE")
        c += [f"$ {cmd.replace('I.EVE', f'I{i}.EVE')}", "$ SHOW SYMBOL $STATUS"]
        if "/INITIALIZATION=" not in cmd and init != "-":
            c.append("$ DEASSIGN EVE$INIT")
        if show != "-":
            c += ["$ DIRECTORY/NOHEADING/NOTRAILING *.TXT;*", f"$ TYPE {show}"]
    c += ['$ WRITE SYS$OUTPUT "@@ end"', "@@CTRLZ", "@B"]
    return c


def main():
    signal.signal(signal.SIGTERM, lambda *_: sys.exit("terminated"))
    parts = [a for a in sys.argv[1:] if a in ("batch", "sessions")] or ["batch", "sessions"]
    names = [a for a in sys.argv[1:] if a not in parts]
    spec = importlib.util.spec_from_file_location("runvms", RUNVMS)
    m = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(m)
    m.CMD_TIMEOUT = 150
    m.EMU = AXPBOX
    run = pathlib.Path(tempfile.mkdtemp(prefix="vpt-eve-"))
    shutil.copytree(ROM, run / "rom")
    m.HERE = str(run)

    def command(self, line):
        """A DCL line, again after an ACCVIO; a Return wakes the echo."""
        for _ in range(3):
            self.send(line + "\r")
            for _ in range(3):
                if self.poll(self.at_prompt, 60):
                    break
                self.sock.sendall(b"\r")
            else:
                self.wait(self.at_prompt, m.CMD_TIMEOUT, repr(line))
            if "ACCVIO" not in self.since():
                return

    def session(self, name, cmd, keys):
        for _ in range(3):
            command(self, "DELETE/NOLOG *.TXT;*")
            command(self, "COPY/NOLOG *.ORI *.TXT")
            command(self, f'WRITE SYS$OUTPUT "@@ {name}"')
            out = len(self.buf)
            typed(self, cmd, keys)
            command(self, "SHOW SYMBOL $STATUS")
            if not any(e in self.buf[out:] for e in ("ACCVIO", "NONANSICRT")):
                return

    def dcl(self, line):
        if line.startswith("@@SESSION "):
            name, cmd, keys = line[10:].split("\t")
            return session(self, name, cmd, keys.encode("latin-1").decode("unicode_escape"))
        return command(self, line)

    def settle(self):
        """Waits for the screen to be painted: TPU doesn't paint while keys
        are waiting, so a key typed early leaves an old screen."""
        start = last = time.time()
        while time.time() - last < QUIET and time.time() - start < 10:
            n = len(self.buf)
            self.pump(0.05)
            if len(self.buf) != n:
                last = time.time()

    def typed(self, cmd, keys):
        self.mark = len(self.buf)
        self.sock.sendall((cmd + "\r").encode("latin-1"))
        self.poll(lambda: any(s in self.since() for s in ("Forward", "Reverse", "%TPU", "%DCL")) or self.at_prompt(), 60)
        settle(self)
        i = 0
        while i < len(keys):
            j = i + 1
            if keys[i] == "\x1b":
                j = i + 2
                if keys[i + 1] == "[":
                    while j < len(keys) and not 0x40 <= ord(keys[j]) <= 0x7E:
                        j += 1
                    j += 1
                elif keys[i + 1] == "O":
                    j = i + 3
            self.sock.sendall(keys[i:j].encode("latin-1"))
            i = j
            settle(self)
            self.log.write("\x1e")
        for _ in range(4):
            if self.poll(self.at_prompt, 25):
                break
            self.log.write("\x1f")
            self.sock.sendall(b"\x1a")
        self.wait(self.at_prompt, m.CMD_TIMEOUT, cmd)

    m.Console.dcl = dcl
    clean = m.Console.clean
    m.Console.clean = lambda self, data: clean(self, data.replace(b"\r", b"\x1d")).replace("\x1d", "\r")
    out = HERE / "recorded"
    out.mkdir(exist_ok=True)
    try:
        for part in parts:
            log = out / ("new-console.log" if names else f"{part}-console.log")
            m.run(commands(part, names), str(log), SYSTEM)
            pathlib.Path(f"{log}.emu").unlink(missing_ok=True)
            if names:
                # Added to the sessions recorded again before.
                with open(out / "again-console.log", "a", encoding="utf-8", newline="") as f:
                    f.write(log.read_text(encoding="utf-8", newline=""))
                log.unlink()
    finally:
        shutil.rmtree(run)
    print("recorded; now: screens.py")


if __name__ == "__main__":
    main()
