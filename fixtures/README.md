# Fixtures recorded on real OpenVMS

What VMS does, recorded on OpenVMS Alpha V8.4-2L1 in the AXPbox emulator.
The vmsport crates are tested against these. The inputs were written for vmsport;
`recorded/` holds what VMS made of them.

| Dir | Input | Recorded |
| --- | --- | --- |
| `msg/` | `TESTMSG.MSG`, `msg.com` | MESSAGE's listing and SDL, `F$MESSAGE` for every code, how DCL shows exit statuses |
| `fao/` | `cases.txt` (`gen.py` makes `fao.com`) | `F$FAO` results |
| `cld/` | `VPTEST.CLD`, `cases.txt` (`gen.py` makes `cld.com`), `CLIDUMP.MAR` | what `CLI$PRESENT`/`CLI$GET_VALUE` return per command line, or DCL's error |
| `rms/` | `*.FDL`, `rms.com` | one RMS file per organization and record format (`.DAT`, raw blocks), `ANALYZE/RMS_FILE` output, record attributes in `ods-manifest.json` |
| `rmsblk/` | `*.FDL`, `rmsblk.com` | sequential files whose records meet block boundaries (`BLOCK_SPAN no`, records longer than a block) and a FORTRAN carriage-control file |
| `dcl/` | `*.COM` | the procedures' output |
| `utils/` | `setup.txt`, `cases.txt` (`gen.py` makes `UTILS.COM`) | DIRECTORY, TYPE, COPY, DELETE, PURGE and SEARCH output and `$STATUS`, case by case |
| `cabi/` | `CODES.COM` | the `#define` lines of the LIB$, SS$, CLI$, STS$ and DSC$ values vmsport's C headers use, from DEC C's SYS$STARLET_C.TLB (`to_headers.py` writes `include/`) |
| `lnm/` | `LNM.COM` | logical names: SHOW LOGICAL, F$TRNLNM items, rooted and concealed devices through F$PARSE |

Each area's `vms.txt` says what goes to VMS, what runs there and what comes
back. To record areas again (about 5 minutes; it borrows the real-VMS setup of
the vaxpunk project at `$VAXPUNK`, default `~/p/vaxpunk`, and its `ods` tool):

```sh
ODS=path/to/ods fixtures/vms/record.py msg cld
```

Statuses in `cld/recorded/cld.log`: 3FD19 PRESENT, 3FD21 DEFAULTED, 3FD29 CONCAT,
3FD31 LOCPRES, 3FD39 COMMA, 381F0 ABSENT, 381F8 NEGATED, 38230 LOCNEG, and 310FC for
an entity that the current syntax doesn't define.
