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
| `rmsrel/` | `*.FDL`, `rmsrel.com` | relative files of several shapes (FIX, VAR, VFC; MRN set and not; bucket sizes; records deleted, updated, appended; a file that extends; an empty one), `ANALYZE/RMS_FILE` output, DCL READ results, CREATE/FDL's error for SIZE 0 |
| `rmsback/` | `ours/` (made by `crates/vms-rms/tests/back.rs`), `rmsback.com` | what VMS makes of files vms-rms wrote: `ANALYZE/RMS_FILE` /CHECK and /FDL, DCL READ and READ/KEY, and one of them after DCL changed it |
| `rmsblk/` | `*.FDL`, `rmsblk.com` | sequential files whose records meet block boundaries (`BLOCK_SPAN no`, records longer than a block) and a FORTRAN carriage-control file |
| `dcl/` | `*.COM` | the procedures' output |
| `utils/` | `setup.txt`, `cases.txt` (`gen.py` makes `UTILS.COM`) | DIRECTORY, TYPE, COPY, DELETE, PURGE and SEARCH output and `$STATUS`, case by case |
| `cabi/` | `CODES.COM` | the `#define` lines of the LIB$, SS$, CLI$, STS$ and DSC$ values vmsport's C headers use, from DEC C's SYS$STARLET_C.TLB (`to_headers.py` writes `include/`) |
| `lnm/` | `LNM.COM` | logical names: SHOW LOGICAL, F$TRNLNM items, rooted and concealed devices through F$PARSE |
| `edt/` | `run2.dcl`, `run3.dcl` (typed at an installed system's console by `fixtures/vms/system.py`) | EDT in line mode from a procedure's data lines: ranges and line numbers, every command's output and messages, FILL, RESEQUENCE, buffers, SET and SHOW, PRINT's records (DUMP), journals and /RECOVER, the qualifiers. Keypad mode couldn't be recorded at the console: it follows EDT's documented keypad (crates/vms-edt/tests/keypad.rs) |
| `time/` | `time.com` | `F$CVTIME`, `F$TIME` and the time formats |
| `spawn/` | `SPAWN.COM`, `PIPE.COM` | SPAWN and PIPE: their messages, statuses and symbols |
| `help/` | `TEST.HLP`, `help.com` | HELP: a topic, and a prompting session fed by data lines |
| `rmsdef/` | `RMSDEF.COM` | the `#define` lines of RMSDEF, FABDEF, RABDEF, NAMDEF and the XAB headers (`to_rust.py` writes `vms_rms::status`, `to_headers.py` the C headers) |
| `idx/` | `*.FDL`, `BUILD.COM`, `READS.COM` | indexed files of many shapes (empty, one put, ascending and descending loads, keys of every type, segments, duplicates, compression on and off, multi-level indexes, areas) with ANALYZE/RMS_FILE output and every key's order |
| `idxw/`, `idxp/`, `idxv/` | `*.FDL`, `BUILD.COM` | indexed files after puts, updates and deletes (splits, RRVs, SIDR chains); how full a bucket gets; VMS doing to its files what vms-rms does to its own |
| `idxrt/` | indexed files vms-rms wrote | VMS's verdict on them: ANALYZE/RMS_FILE, every key's order, READ/KEY, and VMS writing on into one |
| `dclrms/` | `DCLRMS.COM` | DCL's OPEN, READ (/KEY /INDEX /MATCH /DELETE /NOLOCK), WRITE (/UPDATE) and CLOSE on indexed and relative files, record locks between two streams, file sharing, and TYPE, SEARCH, DIRECTORY/FULL, F$FILE_ATTRIBUTES on them |
| `fdlutil/`, `fdlutil2/` | FDLs, procedures | CREATE (plain, /DIRECTORY, /FDL), ANALYZE/RMS_FILE (/CHECK, /FDL, /OUTPUT), DIRECTORY/FULL and F$FILE_ATTRIBUTES on every organization |
| `sort/` | `*.FDL`, `SORT.COM`, `CONVERT.COM` | SORT, MERGE and CONVERT: keys, orders, formats and organizations, exceptions, statistics, errors, and the output files' blocks |
| `sortback/` | indexed files SORT and CONVERT made here | VMS's ANALYZE/RMS_FILE/CHECK and reads of them |
| `cabiback/` | files `examples/rms/rmsdemo.c` wrote through the C ABI | VMS's ANALYZE/RMS_FILE/CHECK and DCL READ of them |
| `mount/` | `mount.com` | MOUNT and DISMOUNT: messages, statuses, the DISK$label logical name |
| `accept/` | `ORDERS.FDL`, `PARTS.FDL`, `MAKE.COM`, `UPDATE.COM` | M3's acceptance, part one: indexed files VMS makes, and the same files after UPDATE.COM's puts, updates and deletes on VMS |
| `acceptback/` | `VOLUME.IMG.gz` (written by `crates/vms-utils/tests/accept.rs`) | part two: the image vmsport wrote back after UPDATE.COM, as VMS's input volume: ANALYZE/RMS_FILE/CHECK, every key's order, then VMS's own puts and deletes checked again |
| `edf/` | `run*.dcl` (`gen.py` makes all but `run1.dcl`); `probe3`, `seqrel`, `seqrel2`, `menus`, `menus2`, `indexed`, `others`.dcl typed at the console | EDIT/FDL/NOINTERACTIVE/ANALYSIS on an installed system (below): `run1` analyzes real files of each organization and gives the statuses and errors; the others sweep synthetic analyses (record counts, compression, key and record sizes, fills, clusters, duplicates, three and four keys, granularity), each case's analysis and the FDL the editor wrote. The typed ones are the editor at its terminal: the main menu's functions and their tables, SET, every script with its questions, plots, mnemonics and summaries, and what each wrote (crates/vms-utils/tests/fdl_editor.rs replays them) |

Each area's `vms.txt` says what goes to VMS, what runs there and what comes
back. To record areas again (about 5 minutes; it borrows the real-VMS setup of
the vaxpunk project at `$VAXPUNK`, default `~/p/vaxpunk`, and its `ods` tool):

```sh
ODS=path/to/ods fixtures/vms/record.py msg cld
```

Besides `in`, `run`, `text`, `blocks` and `manifest`, an area's `vms.txt` can
bring binary files in with their attributes (`import DIR`, `binary FILE
ATTRS`) and give VMS a volume vmsport wrote as its input disk (`volume
IMAGE`). MOUNT on VMS needs `/NOASSIST`, or it waits for an operator.

What the install CD can't run (EDIT/FDL) is recorded on an installed VMS
system instead, booted from a copy-on-write clone of its disk image by
vaxpunk's `run-vms.py --system` (`$VAXPUNK`, `$RUNVMS`, `$VMS_SYSTEM`), whose console it
types the area's `NAME.dcl` into; the log goes to `recorded/NAME.log`:

```sh
fixtures/vms/system.py edf run1
```

A program that wants a terminal (the FDL editor's dialogue) is typed at
the console with `@@KEY` lines, its questions answered with `@@AUTO`
(see system.py).

Statuses in `cld/recorded/cld.log`: 3FD19 PRESENT, 3FD21 DEFAULTED, 3FD29 CONCAT,
3FD31 LOCPRES, 3FD39 COMMA, 381F0 ABSENT, 381F8 NEGATED, 38230 LOCNEG, and 310FC for
an entity that the current syntax doesn't define.
