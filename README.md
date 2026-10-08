# vmsport

The OpenVMS userland as plain processes and libraries on macOS and Linux:
see [docs/PRD.md](docs/PRD.md), and [docs/design/m1.md](docs/design/m1.md)
for how the pieces fit ([m2.md](docs/design/m2.md): images and the C ABI;
[m3.md](docs/design/m3.md): RMS, sharing and images as devices).

```sh
cargo build                                       # or just `cargo run --bin dcl`: it builds the rest
target/debug/dcl                                  # a DCL session
target/debug/dcl -c 'DIRECTORY/SIZE [.crates]'    # one command
target/debug/dcl LOGIN.COM P1 P2                  # @LOGIN.COM
target/debug/directory /size '[.crates]'          # a utility, from zsh
target/debug/vmsport path SYS\$LOGIN:NOTES.TXT     # VMS spec to host path
target/debug/lnm DATA=HOST:[tmp.data.]            # a job logical name
target/debug/help directory /size                 # HELP, from zsh too
target/debug/directory --help                     # any command's /HELP
target/debug/dcl -c 'MOUNT disk.img DKA100:'      # a Files-11 image as a device
```

The first program to need logical names starts `vmsportd`, the per-user
daemon holding the job, group and system tables and the lock manager (socket in
`${TMPDIR:-/tmp}/vmsport-$UID`, or `$VMSPORT_RUN`). The host's `/` is the
device `HOST:`; `SYS$LOGIN` is your home directory.

## Crates

| Crate | What |
| --- | --- |
| `vms-filespec` | `NODE::DEV:[DIR]NAME.TYP;VER` with ODS-5 extended names, `$PARSE` defaulting |
| `vms-cond` | 32-bit condition values |
| `vms-fao` | `$FAO` |
| `vms-msg` | `.MSG` compiler, message files, `$GETMSG`, `$PUTMSG` |
| `vms-cld` | CLD compiler, DCL command-line parsing, `CLI$PRESENT`, `CLI$GET_VALUE` |
| `vms-time` | VMS times, `$ASCTIM`, `$BINTIM`, `F$CVTIME` |
| `vms-lnm` | logical name tables, translation, rooted and concealed devices |
| `vms-rms` | record attributes and formats; relative and indexed files, FDL, ANALYZE/RMS_FILE's reports, on any block store |
| `vmsportd` | the daemon and its client: shared logical name tables, the lock manager |
| `libvms` | the host side: specs to paths, versions, `$SEARCH`, files, images; RMS record streams shared through the lock manager; images mounted as devices |
| `vms-dcl` | DCL, and the `dcl` binary |
| `vms-help` | help libraries (`.HLP`), HELP's lookups, pages and prompts |
| `vms-utils` | `directory`, `type`, `copy`, `delete`, `purge`, `search`, `help`, `create`, `analyze` (/RMS_FILE), `tpu` (EDIT: EVE), `edt` (EDIT/EDT), `edf` (EDIT/FDL), `hostedit` (EDIT/HOST: your $EDITOR), `convert`, `sort` (and MERGE), `mount`, `dismount` |
| `vms-eve`, `vms-edt` | the EVE and EDT editors' cores: buffers, commands, keypads, screens |
| `vmsport` | `vmsport path`/`spec`/`cdu`, and `lnm` |
| `vms-c` | the C ABI: `libvms` with `cli$present`, `lib$get_symbol`, ..., RMS (`sys$open` ... with FAB, RAB, XABs, NAM) and `sys$enqw`/`sys$deq`, and `include/` |
| `vms-examples` | `greet`: one CLD, a Rust and a C program (`examples/greet`); `examples/rms`: RMS and locks from C |

The `vms-*` cores are pure (no host I/O). `sys/` holds what VMS keeps in
SYS$SYSROOT: the command tables (`SYSLIB/DCLTABLES`), the system messages
(`SYSMSG`) and the help library (`SYSHLP/HELPLIB.HLP`).

`cargo test` checks the crates against [fixtures](fixtures/README.md)
recorded on OpenVMS Alpha V8.4-2L1, among them DCL procedures whose output
vmsport must reproduce exactly.
