# PRD — VMS from User Space (vmsport)

Oct 6, 2026 · @Marko Mikulicic

## Summary

vmsport rebuilds the OpenVMS userland as plain processes and libraries on macOS and Linux, the way plan9port did for Plan 9. Unix stays the kernel. On top of it you get DCL, command tables (CLD), logical names, versioned files, RMS (including indexed files), condition values and message files, CDD, DECforms, and the core utilities.

Everything lives under one install tree (`$VMSPORT`, like `$PLAN9`). The pieces are written in Rust and exposed both as Rust crates and as a C ABI, so any Unix program can open an RMS indexed file or translate a logical name.

The point is to live in the VMS working world (DCL sessions, command procedures, forms over RMS files) every day on a Mac, without booting anything.

## Goals and non-goals

The goal is VMS behavior at the userland level, faithful where users can see it. Kernel-level fidelity is vaxpunk's job, not this project's.

**Goals**

- A DCL session on macOS or Linux that runs real-world command procedures: symbols, lexical functions, `ON ERROR`, `$STATUS`, `@` procedures, `SET DEFAULT`, `SPAWN`.
- Command tables: a CDU-compatible CLD compiler and the `CLI$` routines, so any program (Rust, C, Go) gets typed, validated parse results instead of raw argv.
- Logical names with the full semantics: process / job / group / system tables, search lists, iterative translation, rooted and concealed devices.
- VMS file specs and file versions on top of the host file system, including `PURGE` and `;-1`.
- RMS: sequential, relative and indexed files, with multiple keys, duplicates and record locking between processes.
- Indexed files that can move to and from real VMS (via ODS images) without conversion.
- Condition values, `.MSG` message files, `SET MESSAGE`, `LIB$SIGNAL` and handlers.
- CDD-lite and DECforms (IFDL compiler plus a character-cell runtime on any VT-compatible terminal).
- The everyday utilities: `DIRECTORY`, `TYPE`, `COPY`, `RENAME`, `DELETE`, `PURGE`, `SEARCH`, `DIFFERENCES`, `SORT`, `CONVERT`, `ANALYZE/RMS_FILE`, an EDT/EVE-style keypad editor.
- Everything usable from zsh too, not only from inside DCL.

**Non-goals**

- Running VMS images (Alpha or vaxpunk ARM64). No image loader, no binary translation.
- The VMS security model: UAF, UICs as real access control, privileges, ACLs. Host permissions apply.
- Clustering, DECnet, batch/print queues (until a later phase), DECwindows.
- Full `SYS$` coverage. Only the services the userland above actually needs.
- Frame-based condition unwinding across mixed languages; vmsport emulates handlers at the library level.
- ACMS. It gets its own PRD once DECforms and RMS work.

## Relationship to vaxpunk and the ODS crate

vmsport is the host-side twin of vaxpunk: same userland semantics, different floor. vaxpunk keeps its own MACRO-32/BLISS implementations; vmsport supplies Rust reference versions to test them against.

- **Shared I/O-agnostic core crates.** The format and language pieces (RMS bucket and key layouts, file spec parsing, CLD language, `.MSG` language, IFDL, CDD record definitions) are pure Rust with no host I/O. This follows the same pattern as the [ODS PRD](https://claude.ai/code/artifact/51f57b99-6b1c-46d4-8ebf-a3fbc8619c62).
- **Test oracles.** When vaxpunk implements RMS or the CLI in MACRO-32, the same inputs run through the vmsport crates and the outputs are diffed: parse results, record streams, on-disk bytes.
- **Fast iteration.** Userland behavior (DCL edge cases, CLD rules, DECforms panel logic) gets worked out on the Mac in seconds, before it is ported to the slower, more constrained vaxpunk environment.
- **ODS images as devices.** vmsport can mount a Files-11 image through the ODS image library and present it as a device (`DKA100:`). That is how files move between vmsport, vaxpunk and real VMS.
- **Shared applications.** The VMS-alive home-office inventory app should build from one source tree for both targets, minus ACMS on vmsport.

The language rule differs on purpose. vaxpunk wants epoch-appropriate languages; vmsport is host tooling, so Rust everywhere is fine.

## Architecture

Three layers sit on top of Unix: front ends, one host library (libvms), and pure core crates. State that VMS shares between processes lives in one small per-user daemon.

Every front end, DCL included, is just a client of libvms. Only vmsportd holds state that outlives a single process.

**Process model**

- A VMS process is a DCL session. Each image DCL runs is a child Unix process that inherits the session context: process logical names, default directory, the parsed command line, and access to DCL symbols.
- Context travels over an inherited file descriptor, not environment variables. It carries binary parse results and comes back changed (symbols set, logicals defined with `/PROCESS`).
- The image's full 32-bit condition value returns over the same channel, because a Unix exit code holds only 8 bits. DCL sets `$STATUS` from it.
- vmsportd holds job, group and system logical name tables, the lock manager and mailboxes. It talks over a Unix socket and starts on first use.
- Install tree: `$VMSPORT/bin`, `lib`, `include`, and `sys` for `SYS$SYSTEM`, `SYS$MESSAGE`, `SYS$HELP` and `DCLTABLES`.

## Components

Each component has a pure core crate plus a host layer. Behavior is checked against real VMS running in the Alpha emulator.

### File specs and versions

`DEV:[DIR.SUB]NAME.TYP;VER` maps onto host paths. Devices are rooted logical names pointing at host directories: `SYS$LOGIN` → `$HOME`, `SYS$SYSDEVICE` → `$VMSPORT/sys`, user-defined ones via `DEFINE/TRANSLATION=CONCEALED`.

- Versions are stored as literal host file names, `REPORT.TXT;3`. Both macOS and Linux allow `;`. Unix tools see the versions, which is honest and debuggable.
- Lookups are case-insensitive. On Linux this means a directory scan, cached per directory.
- A file with no version suffix counts as version 1, so ordinary Unix files are visible from DCL.
- Record attributes (RFM, RAT, MRS, organization) live in an extended attribute (`vms.fab`). No attribute means stream\_lf, so plain text files just work in both directions.

### Logical names

All four standard tables, plus `LNM$FILE_DEV` and friends, with search lists, iterative translation up to 10 levels, and the access-mode tags kept as data even though they guard nothing. Process tables live in the process; job, group and system tables live in the daemon (see Architecture). `$CRELNM`, `$TRNLNM`, `$DELLNM` and `F$TRNLNM` all go through one library.

### DCL

A from-scratch DCL interpreter: local and global symbols, `=` / `==` / `:=`, all `F$` lexical functions that make sense on a host, labels, `GOTO`, `GOSUB`, `CALL`, `ON ERROR` / `ON CONTROL_Y`, `$STATUS` and `$SEVERITY`, `SET VERIFY`, `INQUIRE`, `READ` / `WRITE` / `OPEN` on RMS files, `PIPE`, command recall and line editing. Verbs dispatch through `DCLTABLES`. Foreign commands (`$ X :== $path`) run Unix binaries with Unix argv.

### Command tables (CLD / CDU)

A compiler for the CLD language (`SET COMMAND`) producing compiled tables, and the `CLI$PRESENT`, `CLI$GET_VALUE`, `CLI$DCL_PARSE` routines. DCL parses the command line against the table and hands the image a typed parse result. This is the piece most likely to be useful outside VMS nostalgia: one argument-passing convention that every language shares.

### RMS

- Sequential: fixed, variable, VFC, stream, stream\_lf, stream\_cr.
- Relative: fixed-size cells, by record number.
- Indexed: prologue 3, primary plus alternate keys, segmented keys, duplicates, key compression, bucket splits.
- Access by key, by RFA, sequential; `$OPEN`, `$CONNECT`, `$GET`, `$FIND`, `$PUT`, `$UPDATE`, `$DELETE`, with FAB / RAB / XAB / NAM structures in the C ABI.
- Sharing and record locks via the lock manager.
- FDL: `CREATE/FDL`, `ANALYZE/RMS_FILE/FDL`, and a non-interactive `EDIT/FDL` subset.

On-disk layout of indexed and relative files matches VMS byte for byte. Files created on real VMS and copied through an ODS image must open unchanged, and the reverse.

### Messages and conditions

32-bit condition values (facility, number, severity). A `.MSG` compiler producing message files, `SET MESSAGE`, `$GETMSG`, `$PUTMSG`, `%FACIL-S-IDENT, text` formatting. `LIB$SIGNAL`, `LIB$STOP`, `LIB$ESTABLISH` work as a per-thread handler stack inside the library. Unix signals like SIGINT turn into conditions such as `SS$_CONTROLC`.

### System services subset

Only what the userland needs: time (`$GETTIM`, `$ASCTIM`, `$BINTIM`, VMS 64-bit time), `$GETJPI` / `$GETSYI` subsets, event flags, ASTs delivered at library call points, mailboxes, `$QIO` on terminals and mailboxes, and a lock manager (`$ENQ` / `$DEQ`) good enough for RMS sharing.

### CDD-lite

A small record-definition repository located by `CDD$DEFAULT`. Field types, arrays, variants, using CDO-like syntax. It feeds RMS (generate FDL), DECforms (record binding) and language includes (generated Rust structs and C headers).

### DECforms

An IFDL compiler producing form files, and a runtime with the `FORMS$ENABLE`, `SEND`, `RECEIVE`, `TRANSCEIVE`, `DISABLE` calls. Panels, fields, validation, function-key responses and records bound to CDD definitions. Output is VT100/VT220 escape sequences, so it runs in any modern terminal. Target: compile real IFDL sources from DEC documentation and examples without edits.

### Utilities

First wave: `DIRECTORY`, `TYPE`, `COPY`, `RENAME`, `DELETE`, `PURGE`, `CREATE`, `APPEND`, `SEARCH`, `DIFFERENCES`, `SORT` / `MERGE` with RMS key specs, `CONVERT`, `ANALYZE/RMS_FILE`, `SHOW` / `SET` basics. Second wave: an EDT/EVE-style keypad editor, `MAIL` and `PHONE` between local sessions, `SUBMIT` with a simple batch queue. Every utility is a normal binary with a CLD, so it works from DCL and from zsh.

## Interop with Unix

vmsport is a set of libraries first and a DCL second. Nothing should require being inside DCL.

- **C ABI.** Headers under `$VMSPORT/include` use the real names and structures: `sys$open`, `struct FAB`, `lib$signal`, string descriptors (`struct dsc$descriptor_s`). Old DEC C examples should compile with few changes.
- **Rust crates.** Safe wrappers over the same code, published from one workspace. Go and others go through the C ABI.
- **From zsh.** `dcl -c 'DIRECTORY/SIZE [.SRC]'` runs one command. Every utility binary also accepts its CLD syntax directly (`directory /size '[.src]'`). An `lnm` tool defines and shows logical names.
- **Logical names for Unix tools.** `vmsport path SYS$LOGIN:NOTES.TXT` prints the host path. An optional `LD_PRELOAD` / `DYLD_INSERT_LIBRARIES` shim that translates VMS specs in `open()` is a later experiment, not a goal.
- **Text files.** stream\_lf is the default for new text files, so `cat`, `grep` and editors work on DCL output.
- **Pipes.** DCL `PIPE` connects images with host pipes; `SYS$INPUT` and `SYS$OUTPUT` are ordinary file descriptors underneath.

## Milestones and acceptance tests

Each milestone ends with a test that a VMS user would recognize, checked against real VMS in the Alpha emulator where possible.

1. **M0 — Cores and fixtures.** File spec parser, condition values, `.MSG` compiler, CLD compiler as pure crates. A fixture set pulled from real VMS: RMS files of every organization, CLD sources, message files, DCL procedures with recorded output.
   - Accept: CLD tables and message files compiled by vmsport parse the same command lines and print the same text as VMS.
2. **M1 — A usable DCL.** Logical names (all tables, daemon running), versioned files, sequential RMS, DCL core language, `DIRECTORY`, `TYPE`, `COPY`, `DELETE`, `PURGE`, `SEARCH`.
   - Accept: a recorded set of real command procedures runs with matching output and `$STATUS`.
3. **M2 — Command tables for everyone.** `SET COMMAND`, `CLI$` routines in Rust and C, foreign commands, `SPAWN`, `PIPE`, `dcl -c` from zsh.
   - Accept: a new utility written in Rust and one written in C both get typed qualifiers from the same CLD.
4. **M3 — RMS complete.** Relative and indexed files, lock manager, sharing between processes, FDL, `CONVERT`, `ANALYZE/RMS_FILE`, `SORT` / `MERGE`.
   - Accept: indexed files made on VMS open through an ODS image, survive updates and splits, go back, and pass `ANALYZE/RMS_FILE` on real VMS.
5. **M4 — Forms.** CDD-lite, IFDL compiler, DECforms runtime on a VT220-compatible terminal.
   - Accept: an IFDL form taken from DEC examples compiles unchanged and runs a send/receive loop against an RMS file.
6. **M5 — The inventory app.** The home-office inventory app (DECforms over RMS indexed files, CDD records, no ACMS) runs on vmsport from the same sources that target vaxpunk.
   - Accept: daily use for a week on the Mac without dropping to zsh.

## Open questions

- [ ] **Image model.** fork/exec plus a context channel (default in this PRD), or native images as shared libraries loaded into the DCL process, which is closer to VMS image activation and rundown but lets one bad image kill the session?
- [ ] **Versions on disk.** Literal `NAME.TYP;3` host files, or a hidden per-directory version store that keeps Unix listings clean?
- [ ] **Record attributes.** Extended attributes are lost by some tools (`git`, some `cp`, archives). Add a sidecar fallback, or accept the loss and default to stream\_lf?
- [ ] **ASTs.** Delivered only at library call points, or on a dedicated thread? The second is closer to VMS but makes handlers harder to reason about.
- [ ] **Terminal fidelity.** How much of the VT220 keypad and `$QIO` terminal modifiers (no-echo, purge type-ahead, read with terminators) do the editor and DECforms need on a modern terminal emulator?
- [ ] **Name.** vmsport, or something else.
