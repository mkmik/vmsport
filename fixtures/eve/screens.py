#!/usr/bin/env python3
"""recorded/sessions-console.log (then again-console.log, sessions
recorded anew) as recorded/screens.txt (each EVE session's last screen:
its 24 rows as a VT100 shows them, the cursor, $STATUS; a session
recorded again, its last time) and
recorded/batch-console.log as recorded/batch.log (the NODISPLAY cases'
output, CRs dropped).

usage: screens.py [-k NAME]   (-k: NAME's screen after each key, to read)
"""
import pathlib, re, sys

HERE = pathlib.Path(__file__).resolve().parent


class Screen:
    """A VT100, as much of one as EVE uses: cursor moves, erasing, the
    scrolling region, insert mode, line and character insert/delete."""

    def __init__(self, rows=24, cols=80):
        self.rows, self.cols = rows, cols
        self.g = [[" "] * cols for _ in range(rows)]
        self.r = self.c = 0
        self.top, self.bot = 0, rows - 1
        self.saved = (0, 0)
        self.wrap = True
        self.insert = False
        self.graphics = False

    def blank(self):
        return [" "] * self.cols

    def lf(self):
        if self.r == self.bot:
            del self.g[self.top]
            self.g.insert(self.bot, self.blank())
        elif self.r < self.rows - 1:
            self.r += 1

    def put(self, ch):
        if self.c >= self.cols:
            if not self.wrap:
                self.c = self.cols - 1
            else:
                self.c = 0
                self.lf()
        if self.graphics:
            # DEC special graphics: the diamond, and the symbols TPU shows
            # FF, CR, LF and VT as.
            ch = {"`": "◆", "c": "␌", "d": "␍", "e": "␊", "i": "␋"}.get(ch, ch)
        row = self.g[self.r]
        if self.insert:
            row.insert(self.c, " ")
            del row[self.cols:]
        row[self.c] = ch
        self.c += 1

    def csi(self, priv, params, final):
        ps = [int(p) if p else 0 for p in params.split(";")] if params else []
        p = lambda i, d=1: ps[i] if len(ps) > i and ps[i] else d
        if priv:
            if final in "hl" and params == "7":
                self.wrap = final == "h"
            return
        if final in "hl":
            if params == "4":
                self.insert = final == "h"
        elif final in "Hf":
            self.r, self.c = min(p(0), self.rows) - 1, min(p(1), self.cols) - 1
        elif final == "A":
            self.r = max(self.r - p(0), 0)
        elif final == "B":
            self.r = min(self.r + p(0), self.rows - 1)
        elif final == "C":
            self.c = min(self.c + p(0), self.cols - 1)
        elif final == "D":
            self.c = max(min(self.c, self.cols - 1) - p(0), 0)
        elif final == "J":
            m, here = p(0, 0), self.r * self.cols + min(self.c, self.cols - 1)
            for r in range(self.rows):
                for c in range(self.cols):
                    i = r * self.cols + c
                    if m == 2 or (m == 0 and i >= here) or (m == 1 and i <= here):
                        self.g[r][c] = " "
        elif final == "K":
            m, c0 = p(0, 0), min(self.c, self.cols - 1)
            for c in range(self.cols):
                if m == 2 or (m == 0 and c >= c0) or (m == 1 and c <= c0):
                    self.g[self.r][c] = " "
        elif final == "r":
            self.top, self.bot = p(0) - 1, p(1, self.rows) - 1
            self.r = self.c = 0
        elif final in "LM" and self.top <= self.r <= self.bot:
            for _ in range(p(0)):
                if final == "L":
                    del self.g[self.bot]
                    self.g.insert(self.r, self.blank())
                else:
                    del self.g[self.r]
                    self.g.insert(self.bot, self.blank())
        elif final == "P":
            row = self.g[self.r]
            del row[self.c:self.c + p(0)]
            row.extend([" "] * (self.cols - len(row)))
        elif final == "@":
            row = self.g[self.r]
            for _ in range(p(0)):
                row.insert(self.c, " ")
            del row[self.cols:]

    def feed(self, s):
        i = 0
        while i < len(s):
            ch = s[i]
            if ch == "\x1b":
                m = re.match(r"\[([?>]?)([\d;]*)([ -/]*)([@-~])", s[i + 1:])
                if m:
                    if not m.group(3):
                        self.csi(m.group(1), m.group(2), m.group(4))
                    i += 1 + m.end()
                    continue
                nxt = s[i + 1:i + 2]
                if nxt in "()":
                    i += 3
                    continue
                if nxt == "7":
                    self.saved = (self.r, self.c)
                elif nxt == "8":
                    self.r, self.c = self.saved
                elif nxt == "M":
                    if self.r == self.top:
                        del self.g[self.bot]
                        self.g.insert(self.top, self.blank())
                    elif self.r > 0:
                        self.r -= 1
                elif nxt == "D":
                    self.lf()
                elif nxt == "E":
                    self.lf()
                    self.c = 0
                i += 2
                continue
            if ch == "\n":
                self.lf()
            elif ch == "\r":
                self.c = 0
            elif ch == "\b":
                self.c = max(self.c - 1, 0)
            elif ch == "\x0e":
                self.graphics = True
            elif ch == "\x0f":
                self.graphics = False
            elif ch >= " ":
                self.put(ch)
            i += 1

    def dump(self):
        rows = "".join(f"{n:2}|{''.join(r).rstrip()}\n" for n, r in enumerate(self.g, 1) if "".join(r).strip())
        return rows + f"cursor {self.r + 1},{self.c + 1}\n"


SESSION = re.compile(r"@@ (?!end\r)([^\r\n\"]*)\r?\n\r?\$ ([^\r\n]*)\r?\n(.*?)[\r\n\x1e\x1f]*\$ \x1e?SHOW SYMBOL \$STATUS\r?\n\r?  \$STATUS == \"([^\"]*)\"", re.S)


def main():
    # Sessions recorded again (record.py sessions NAME...) replace the first.
    logs = [HERE / "recorded/sessions-console.log", HERE / "recorded/again-console.log"]
    texts = [p.read_text(encoding="utf-8", newline="") for p in logs if p.exists()]
    matches = [m for t in texts for m in SESSION.finditer(t)]
    if sys.argv[1:2] == ["-k"]:
        for m in matches:
            if m.group(1) == sys.argv[2]:
                s = Screen()
                for n, part in enumerate(m.group(3).split("\x1e")):
                    s.feed(part)
                    print(f"-- after key {n}\n{s.dump()}", end="")
        return
    out = {}
    for m in matches:
        name, cmd, stream, status = m.groups()
        s = Screen()
        s.feed(stream.replace("\x1e", "").replace("\x1f", "").rstrip("\r\n"))
        out[name] = f"@@ {name}\nstatus {status}\n{s.dump()}"
    (HERE / "recorded/screens.txt").write_text("".join(out.values()))
    log = (HERE / "recorded/batch-console.log").read_text(encoding="utf-8", newline="")
    a = log.find('"@@ init_exit"')
    b = log.find("\n@@ end", a)
    batch = log[log.find("@@ init_exit", a + 20):b].replace("\r", "")
    (HERE / "recorded/batch.log").write_text(batch + "\n")
    print(f"{len(out)} sessions")


if __name__ == "__main__":
    main()
