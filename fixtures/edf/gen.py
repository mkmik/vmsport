#!/usr/bin/env python3
"""Writes the command files fixtures/vms/system.py runs for EDIT/FDL: each
case is an input FDL and an analysis FDL (a typed head, the FILE, RECORD,
AREA and KEY sections ANALYZE/RMS_FILE/FDL writes, plus ANALYSIS_OF_AREA
and ANALYSIS_OF_KEY sections the procedure writes from the case's numbers),
EDIT/FDL/NOINTERACTIVE/ANALYSIS on them, and what it made, TYPEd between
`@@ NAME ...` lines. Lines stay under the console's 132 columns.

usage: gen.py RUN...   (writes RUN.dcl: run2, run3, run4a ... run4d, run5)

run2 and run3 were recorded before the procedure's case subroutine said
SET NOON and could do a fourth key.
"""
import pathlib

HERE = pathlib.Path(__file__).resolve().parent


def keysec(n, pos, length, dups, changes, extra="", typ=None):
    s = f"KEY {n}\n"
    s += "".join(f"  SEG{i}_LENGTH {l}\n  SEG{i}_POSITION {p}\n" for i, (p, l) in enumerate(seg(pos, length)))
    if dups is not None:
        s += f"  DUPLICATES {'yes' if dups else 'no'}\n  CHANGES {'yes' if changes else 'no'}\n"
    if typ:
        s += f"  TYPE {typ}\n"
    return s + extra


def seg(pos, length):
    return list(zip(pos, length)) if isinstance(pos, tuple) else [(pos, length)]


def design(fmt, size, keys, file_extra="", areas=""):
    """An input FDL: keys are (pos, len, dups, changes[, extra lines])."""
    s = f"FILE\n  ORGANIZATION indexed\n{file_extra}RECORD\n  FORMAT {fmt}\n  SIZE {size}\n{areas}"
    for n, k in enumerate(keys):
        s += keysec(n, *k)
    return s.rstrip("\n")


def head(fmt, size, keys, cluster=16):
    """The FILE ... KEY sections of an analysis, as ANALYZE writes them."""
    s = f"""FILE
  ALLOCATION 128
  BEST_TRY_CONTIGUOUS no
  BUCKET_SIZE 2
  CLUSTER_SIZE {cluster}
  CONTIGUOUS no
  EXTENSION 0
  ORGANIZATION indexed
RECORD
  BLOCK_SPAN yes
  CARRIAGE_CONTROL carriage_return
  FORMAT {fmt}
  SIZE {size}
AREA 0
  ALLOCATION 128
  BUCKET_SIZE 2
  EXTENSION 0
"""
    for n, (pos, length, dups, changes, *rest) in enumerate(keys):
        typ = rest[1] if len(rest) > 1 else "string"
        s += f"KEY {n}\n  CHANGES {'yes' if changes else 'no'}\n  DATA_KEY_COMPRESSION yes\n"
        if n == 0:
            s += "  DATA_RECORD_COMPRESSION yes\n"
        s += f"""  DATA_AREA 0
  DATA_FILL 100
  DUPLICATES {'yes' if dups else 'no'}
  INDEX_AREA 0
  INDEX_COMPRESSION yes
  INDEX_FILL 100
  LEVEL1_INDEX_AREA 0
  NAME ""
  NULL_KEY no
"""
        if n == 0:
            s += "  PROLOG 3\n"
        s += "".join(f"  SEG{i}_LENGTH {l}\n  SEG{i}_POSITION {p}\n" for i, (p, l) in enumerate(seg(pos, length)))
        s += f"  TYPE {typ}\n"
    return s.rstrip("\n")


BASE = dict(n0=1000, df0=72, dkc0=49, drc0=73, dso0=94, dep0=1, ic0=28, if0=34, iso0=2, l10=47, mdl0=64, mil0=10,
            lrl0=64, n1=50, df1=66, dkc1=56, dso1=16, dep1=1, dps1=19, ic1=44, if1=6, iso1=2, l11=8, mdl1=106,
            mil1=12)
BASE4 = dict(BASE, **{f"{k}{i}": v for i in (2, 3) for k, v in
                      dict(n=50, df=66, dkc=56, dso=16, dep=1, dps=19, ic=44, if_=6, iso=2, l1=8, mdl=106,
                           mil=12).items()})
BASE4 = {k.replace("_", ""): v for k, v in BASE4.items()}

SUBS = """$case: SUBROUTINE
$ SET NOON
$ OPEN/WRITE t T.TMP
$ WRITE t ""
$ WRITE t "ANALYSIS_OF_AREA 0"
$ WRITE t "  RECLAIMED_SPACE 0"
$ CALL aok 0
$ IF P5 .GT. 1 THEN CALL aok 1
$ IF P5 .GT. 2 THEN CALL aok 2
$ IF P5 .GT. 3 THEN CALL aok 3
$ CLOSE t
$ COPY 'P2'+T.TMP 'P1'A.FDL
$ DELETE T.TMP;*
$ EDIT/FDL/NOINTERACTIVE/ANALYSIS='P1'A.FDL/OUTPUT='P1'O.FDL'P4' 'P3'
$ st = $STATUS
$ WRITE SYS$OUTPUT "@@ ", P1, " ", P3, " ", P4, " ", F$FAO("!XL", F$INTEGER(st))
$ TYPE 'P1'A.FDL
$ WRITE SYS$OUTPUT "@@ ", P1, " output"
$ TYPE 'P1'O.FDL
$ ENDSUBROUTINE
$aok: SUBROUTINE
$ WRITE t ""
$ WRITE t "ANALYSIS_OF_KEY ", P1
$ WRITE t "  DATA_FILL ", df'P1'
$ WRITE t "  DATA_KEY_COMPRESSION ", dkc'P1'
$ IF P1 .EQ. 0 THEN WRITE t "  DATA_RECORD_COMPRESSION ", drc0
$ WRITE t "  DATA_RECORD_COUNT ", n'P1'
$ WRITE t "  DATA_SPACE_OCCUPIED ", dso'P1'
$ WRITE t "  DEPTH ", dep'P1'
$ IF P1 .GT. 0 THEN WRITE t "  DUPLICATES_PER_SIDR ", dps'P1'
$ WRITE t "  INDEX_COMPRESSION ", ic'P1'
$ WRITE t "  INDEX_FILL ", if'P1'
$ WRITE t "  INDEX_SPACE_OCCUPIED ", iso'P1'
$ WRITE t "  LEVEL1_RECORD_COUNT ", l1'P1'
$ WRITE t "  MEAN_DATA_LENGTH ", mdl'P1'
$ WRITE t "  MEAN_INDEX_LENGTH ", mil'P1'
$ IF P1 .EQ. 0 THEN WRITE t "  LONGEST_RECORD_LENGTH ", lrl0
$ ENDSUBROUTINE"""


def command_file(dir, files, cases, base=BASE):
    """cases: (name, head file, input file, qualifiers, number of keys, {symbol: value})."""
    run = ["$ SET NOON", "$ SET TERMINAL/TAB"]
    for name, hd, inp, quals, keys, sets in cases:
        run.append("$ GOSUB base")
        run += [f"$ {k} = {v}" for k, v in sets.items()]
        run.append(f'$ CALL case {name} {hd} {inp} "{quals}" {keys}')
    run += ['$ WRITE SYS$OUTPUT "@@ end"', "$ EXIT", "$base:"]
    run += [f"$ {k} = {v}" for k, v in base.items()] + ["$ RETURN"]
    dcl = ["SET DEFAULT SYS$LOGIN", f"CREATE/DIRECTORY [.{dir}]", f"SET DEFAULT [.{dir}]"]
    for name, text in files.items():
        dcl += [f"CREATE {name}", *text.splitlines(), "@@CTRLZ"]
    dcl += ["CREATE RUN.COM", *run, SUBS, "@@CTRLZ", "@RUN"]
    text = "\n".join(dcl) + "\n"
    assert max(map(len, text.splitlines())) < 118
    return text


K0 = (0, 8, False, False)
K1 = (8, 10, True, True)


def run2():
    files = {
        "H1.FDL": "FILE\n  ORGANIZATION indexed\nRECORD\n  FORMAT fixed\n  SIZE 64\nKEY 0\n  SEG0_LENGTH 8\n  SEG0_POSITION 0",
        "H2.FDL": design("fixed", 64, [(0, 8, False, False), K1]),
        "H2F.FDL": design("fixed", 64, [(0, 8, False, False, "  DATA_FILL 80\n  INDEX_FILL 70\n"),
                                        (8, 10, True, True, "  DATA_FILL 60\n")]),
        "H2B.FDL": design("fixed", 64, [K0, K1], file_extra="  BUCKET_SIZE 5\n"),
        "H2A.FDL": design("fixed", 64, [(0, 8, False, False, "  DATA_AREA 0\n  INDEX_AREA 0\n"),
                                        (8, 10, True, True, "  DATA_AREA 0\n  INDEX_AREA 0\n")],
                          areas="AREA 0\n  ALLOCATION 100\n  BUCKET_SIZE 4\n  EXTENSION 20\n"),
        "H2C.FDL": design("fixed", 64, [(0, 8, False, False, "  DATA_KEY_COMPRESSION no\n  DATA_RECORD_COMPRESSION no\n"
                                                           "  INDEX_COMPRESSION yes\n"),
                                        (8, 10, True, True, "  DATA_KEY_COMPRESSION no\n")]),
        "H2V.FDL": design("variable", 200, [K0, K1]),
        "A1HEAD.FDL": head("fixed", 64, [K0]),
        "A2HEAD.FDL": head("fixed", 64, [K0, K1]),
    }
    cases = []
    c = lambda name, hd, inp, quals="", keys=1, **sets: cases.append((name, hd, inp, quals, keys, sets))
    for i, n in enumerate([1, 10, 30, 100, 300, 1000, 3000, 10000, 30000, 100000, 1000000]):
        c(f"N{i:02}", "A1HEAD.FDL", "H1.FDL", n0=n)
    for i, kv in enumerate([dict(dkc0=0), dict(dkc0=90), dict(drc0=0), dict(drc0=90), dict(ic0=0), dict(ic0=90),
                            dict(mil0=4), dict(mil0=60), dict(df0=50), dict(if0=50), dict(dep0=3), dict(dso0=9999),
                            dict(l10=9999), dict(mdl0=32), dict(lrl0=200), dict(iso0=999)]):
        c(f"P{i:02}", "A1HEAD.FDL", "H1.FDL", n0=10000, **kv)
    c("T00", "A2HEAD.FDL", "H2.FDL", keys=2)
    for i, kv in enumerate([dict(n0=100000), dict(n1=1000, dps1=0, mdl1=20), dict(n1=10000),
                            dict(dps1=100, mdl1=500), dict(mdl1=5000), dict(mil1=40), dict(dkc1=0), dict(ic1=90),
                            dict(n1=1, dps1=999, mdl1=9999)]):
        c(f"T{i + 1:02}", "A2HEAD.FDL", "H2.FDL", keys=2, **kv)
    for i, q in enumerate(["/GRANULARITY=1", "/GRANULARITY=2", "/GRANULARITY=4", "/GRANULARITY=5",
                           "/EMPHASIS=FLATTER_FILES", "/EMPHASIS=SMALLER_BUFFERS"]):
        c(f"G{i:02}", "A2HEAD.FDL", "H2.FDL", quals=q, keys=2, n0=100000)
    for i, inp in enumerate(["H2F.FDL", "H2B.FDL", "H2A.FDL", "H2C.FDL", "H2V.FDL"]):
        c(f"I{i:02}", "A2HEAD.FDL", inp, keys=2, n0=10000)
    return command_file("EDF2", files, cases)


def run3():
    """With a cluster of 1 the allocations aren't rounded: sweeps of the
    record count, compression, key and record sizes, and alternate keys."""
    nocomp = "  DATA_KEY_COMPRESSION no\n  DATA_RECORD_COMPRESSION no\n  INDEX_COMPRESSION no\n"
    designs = {
        "D1": ("fixed", 64, [K0]),
        "D2": ("fixed", 64, [(0, 4, False, False)]),
        "D3": ("fixed", 64, [(0, 32, False, False)]),
        "D4": ("fixed", 200, [(0, 100, False, False)]),
        "D5": ("fixed", 16, [K0]),
        "D6": ("fixed", 1000, [K0]),
        "D7": ("fixed", 4000, [K0]),
        "D8": ("variable", 200, [K0]),
        "D9": ("fixed", 64, [(0, 8, False, False, nocomp)]),
        "E1": ("fixed", 64, [K0, K1]),
    }
    files = {}
    for d, (fmt, size, keys) in designs.items():
        files[f"{d}.FDL"] = design(fmt, size, keys)
        if d != "D9":
            files[f"{d}A.FDL"] = head(fmt, size, keys, cluster=1)
    files["D1C3A.FDL"] = head("fixed", 64, [K0], cluster=3)
    cases = []
    c = lambda name, d, hd=None, quals="", keys=1, **sets: cases.append(
        (name, f"{hd or d}A.FDL", f"{d}.FDL", quals, keys, sets))
    for i, n in enumerate([1, 50, 100, 200, 300, 500, 700, 1000, 1500, 2000, 3000, 5000, 7000, 10000, 15000, 20000,
                           30000, 50000, 70000, 100000, 150000, 200000, 300000, 500000, 1000000, 2000000, 5000000,
                           10000000]):
        c(f"S{i:02}", "D1", n0=n)
    for i, (k, r) in enumerate([(0, 73), (25, 73), (75, 73), (100, 73), (49, 0), (49, 25), (49, 50), (49, 100)]):
        c(f"C{i:02}", "D1", n0=100000, dkc0=k, drc0=r)
    for i, n in enumerate([1000, 10000, 100000, 1000000]):
        c(f"U{i:02}", "D9", hd="D1", n0=n)
    for i, (d, n) in enumerate((d, n) for d in ["D2", "D3", "D4"] for n in [10000, 100000, 1000000]):
        c(f"K{i:02}", d, n0=n)
    for i, (d, n) in enumerate((d, n) for d in ["D5", "D6", "D7"] for n in [10000, 100000]):
        c(f"R{i:02}", d, n0=n)
    for i, (n, m) in enumerate([(10000, 30), (10000, 100), (10000, 190), (100000, 100)]):
        c(f"V{i:02}", "D8", n0=n, mdl0=m, lrl0=200)
    for i, (n, d, m) in enumerate([(100, 0, 20), (1000, 0, 20), (10000, 0, 20), (1000, 9, 106), (1000, 99, 1000),
                                   (10000, 0, 106), (10000, 9, 20), (3000, 2, 20)]):
        c(f"A{i:02}", "E1", keys=2, n0=10000, n1=n, dps1=d, mdl1=m)
    c("Q00", "D1", quals="/EMPHASIS=SMALLER_BUFFERS", n0=1000000)
    c("Q01", "D1", quals="/EMPHASIS=FLATTER_FILES", n0=1000000)
    c("Q02", "D1", hd="D1C3", n0=10000)
    c("Q03", "D1", hd="D1C3", n0=100000)
    return command_file("EDF3", files, cases)


def run4(part):
    """The bucket size cap and cluster rule, compression flags, fills, SIDR
    sizes (GRANULARITY=4 puts alternate data and index in areas of their
    own), three and four keys, VAR records, segmented and binary keys; in
    parts the console can type without losing its echo (a: also whether a
    head of FILE CLUSTER_SIZE and ORGANIZATION alone does)."""
    flags = lambda k, r, i: "".join(f"  {n} {'yes' if v else 'no'}\n" for n, v in
                                    [("DATA_KEY_COMPRESSION", k), ("DATA_RECORD_COMPRESSION", r),
                                     ("INDEX_COMPRESSION", i)])
    fills = ["  DATA_FILL 50\n", "  DATA_FILL 70\n", "  DATA_FILL 80\n", "  DATA_FILL 90\n", "  INDEX_FILL 70\n",
             "  DATA_FILL 80\n  INDEX_FILL 60\n"]
    K = lambda pos, length, dups=True, typ=None: (pos, length, dups, dups, "", typ) if typ else (pos, length, dups,
                                                                                                 dups)
    designs = {
        "D1": ("fixed", 64, [K0]),
        "E1": ("fixed", 64, [K0, K1]),
        "E2": ("fixed", 64, [K0, K(8, 4)]),
        "E3": ("fixed", 64, [K0, K(8, 32)]),
        "E4": ("fixed", 64, [K0, K(8, 10, False)]),
        "E5": ("fixed", 64, [K0, K1, K(18, 6)]),
        "E6": ("fixed", 64, [K0, K1, K(18, 6), K(24, 4, False)]),
        "D8": ("variable", 200, [K0]),
        "DS": ("fixed", 64, [((0, 20), (4, 4), False, False)]),
        "DB": ("fixed", 64, [(0, 4, False, False, "", "bin4")]),
    }
    files, cases = {}, []

    def use(d, cluster=1, minimal=None):
        minimal = part != "a" if minimal is None else minimal
        fmt, size, keys = designs[d]
        files.setdefault(f"{d}.FDL", design(fmt, size, keys))
        hd = f"{d}{'M' if minimal else ''}{'' if cluster == 1 else f'C{cluster}'}"
        files.setdefault(f"{hd}A.FDL", f"FILE\n  CLUSTER_SIZE {cluster}\n  ORGANIZATION indexed" if minimal
                         else head(fmt, size, keys, cluster=cluster))
        return hd

    def c(name, inp, hd, quals="", keys=1, **sets):
        cases.append((name, f"{hd}A.FDL", f"{inp}.FDL", quals, keys, sets))

    g4 = "/GRANULARITY=4"
    if part == "a":
        for i, n in enumerate([230000, 250000, 260000, 270000, 280000]):
            c(f"X{i:02}", "D1", use("D1"), n0=n)
        for i, (cl, n) in enumerate([(3, 1000), (3, 3000), (3, 5000), (4, 1000), (4, 3000), (4, 7000), (4, 30000),
                                     (7, 10000), (7, 100000), (16, 50000), (16, 70000), (16, 150000),
                                     (16, 200000)]):
            c(f"Y{i:02}", "D1", use("D1", cl), n0=n)
        for i, (cl, n) in enumerate([(1, 100000), (16, 100000), (16, 10000)]):
            c(f"M{i:02}", "D1", use("D1", cl, minimal=True), n0=n)
    if part == "b":
        use("D1")
        for i, (k, r, x) in enumerate((k, r, x) for k in (0, 1) for r in (0, 1) for x in (0, 1)):
            files[f"F{i}.FDL"] = design("fixed", 64, [(0, 8, False, False, flags(k, r, x))])
            c(f"F{i:02}", f"F{i}", "D1", n0=100000)
        for i, f in enumerate(fills):
            files[f"L{i}.FDL"] = design("fixed", 64, [(0, 8, False, False, f)])
            c(f"L{i:02}", f"L{i}", "D1", n0=100000)
        for i, m in enumerate([10, 30, 199]):
            c(f"V{i:02}", "D8", use("D8"), n0=10000, mdl0=m, lrl0=200)
        c("O00", "DS", use("DS"), n0=100000)
        c("O01", "DB", use("DB"), n0=100000)
        # What a cluster of 16 does to uncompressed and two-key designs.
        nocomp = "  DATA_KEY_COMPRESSION no\n  DATA_RECORD_COMPRESSION no\n  INDEX_COMPRESSION no\n"
        files["D9.FDL"] = design("fixed", 64, [(0, 8, False, False, nocomp)])
        files["E9.FDL"] = design("fixed", 64, [(0, 8, False, False, nocomp), K1])
        for i, (inp, keys, n) in enumerate([("D9", 1, 10000), ("D9", 1, 100000), ("E9", 2, 10000), ("E1", 2, 10000),
                                            ("F1", 1, 10000), ("F5", 1, 10000)]):
            c(f"U{i:02}", inp, use("D1" if keys == 1 else "E1", 16), "", keys, n0=n)
    if part == "c":
        for i, d in enumerate([0, 1, 2, 3, 5, 9, 20, 99]):
            c(f"B{i:02}", "E1", use("E1"), g4, 2, n0=100000, n1=100000, dps1=d)
        for i, (n1, d) in enumerate([(10000, 0), (10000, 9), (10000, 99), (1000, 99)]):
            c(f"H{i:02}", "E1", "E1", g4, 2, n0=100000, n1=n1, dps1=d)
        for i, k in enumerate([0, 25, 75]):
            c(f"J{i:02}", "E1", "E1", g4, 2, n0=100000, n1=100000, dps1=0, dkc1=k)
        for i, (d, dps) in enumerate([("E2", 0), ("E2", 9), ("E3", 0), ("E3", 9), ("E4", 0)]):
            c(f"N{i:02}", d, use(d), g4, 2, n0=100000, n1=100000, dps1=dps)
    if part == "d":
        for i, (d, keys, q) in enumerate([("E5", 3, ""), ("E5", 3, g4), ("E6", 4, ""), ("E6", 4, g4)]):
            c(f"W{i:02}", d, use(d), q, keys, n0=100000, n1=50000, dps1=1, n2=50000, dps2=1, n3=100000, dps3=0)
        for i, q in enumerate(["/GRANULARITY=1", "/GRANULARITY=2", "", g4]):
            c(f"Z{i:02}", "E1", use("E1"), q, 2, n0=100000, n1=10000, dps1=9)
    return command_file(f"EDF4{part.upper()}", files, cases, BASE4)


def run4a(): return run4("a")
def run4b(): return run4("b")
def run4c(): return run4("c")
def run4d(): return run4("d")


def run5():
    """Uncompressed and key-only record sizes where they show, and the SIDR
    size: duplicates, MEAN_DATA_LENGTH, key length and compression, the
    primary's record count, fill."""
    flags = lambda k, r: f"  DATA_KEY_COMPRESSION {'yes' if k else 'no'}\n  DATA_RECORD_COMPRESSION {'yes' if r else 'no'}\n"
    K = lambda pos, length, dups=True, extra="": (pos, length, dups, dups, extra)
    files = {
        "D1A.FDL": "FILE\n  CLUSTER_SIZE 1\n  ORGANIZATION indexed",
        "FN.FDL": design("fixed", 64, [(0, 8, False, False, flags(0, 0))]),
        "FK.FDL": design("fixed", 64, [(0, 8, False, False, flags(1, 0))]),
        "E1.FDL": design("fixed", 64, [K0, K1]),
        "E2.FDL": design("fixed", 64, [K0, K(8, 4)]),
        "E3.FDL": design("fixed", 64, [K0, K(8, 32)]),
        "E4.FDL": design("fixed", 64, [K0, K(8, 10, False)]),
        "E7.FDL": design("fixed", 64, [K0, K(8, 10, True, "  DATA_FILL 70\n")]),
    }
    cases = []

    def c(name, inp, quals="", keys=1, **sets):
        cases.append((name, "D1A.FDL", f"{inp}.FDL", quals, keys, sets))

    for i, (inp, n) in enumerate([("FN", 23742), ("FN", 24016), ("FK", 2781), ("FK", 7439)]):
        c(f"R{i:02}", inp, n0=n)
    g4 = "/GRANULARITY=4"
    alt = [("E1", 0, 10, {}), ("E1", 0, 50, {}), ("E1", 0, 500, {}), ("E1", 9, 10, {}), ("E1", 9, 50, {}),
           ("E1", 9, 500, {}), ("E1", 1, 10, {}), ("E1", 1, 500, {}), ("E1", 9, 106, dict(n0=1000000)),
           ("E1", 9, 106, dict(n0=10000)), ("E2", 1, 106, {}), ("E2", 3, 106, {}), ("E3", 1, 106, {}),
           ("E3", 3, 106, {}), ("E1", 9, 106, dict(dkc1=0)), ("E1", 9, 106, dict(dkc1=75)), ("E4", 9, 106, {}),
           ("E7", 0, 106, {})]
    for i, (inp, dps, mdl, more) in enumerate(alt):
        c(f"S{i:02}", inp, g4, 2, **dict(dict(n0=100000, n1=100000, dps1=dps, mdl1=mdl), **more))
    return command_file("EDF5", files, cases, BASE4)


if __name__ == "__main__":
    import sys
    for run in sys.argv[1:]:
        (HERE / f"{run}.dcl").write_text(globals()[run]())
