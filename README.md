# vmsport

The OpenVMS userland as plain processes and libraries on macOS and Linux:
see [docs/PRD.md](docs/PRD.md), and [docs/design/m1.md](docs/design/m1.md)
for how the pieces fit.

```sh
cargo build
target/debug/dcl                                  # a DCL session
target/debug/dcl -c 'DIRECTORY/SIZE [.crates]'    # one command
target/debug/dcl LOGIN.COM P1 P2                  # @LOGIN.COM
target/debug/directory /size '[.crates]'          # a utility, from zsh
target/debug/vmsport path SYS\$LOGIN:NOTES.TXT     # VMS spec to host path
target/debug/lnm DATA=HOST:[tmp.data.]            # a job logical name
```

The first program to need logical names starts `vmsportd`, the per-user
daemon holding the job, group and system tables (socket in
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
| `vms-rms` | record attributes, sequential record formats, carriage control |
| `vmsportd` | the daemon and its client |
| `libvms` | the host side: specs to paths, versions, `$SEARCH`, files, images |
| `vms-dcl` | DCL, and the `dcl` binary |
| `vms-utils` | `directory`, `type`, `copy`, `delete`, `purge`, `search` |
| `vmsport` | `vmsport path`/`spec`/`cdu`, and `lnm` |
| `vms-c` | the C ABI: `libvms` with `cli$present`, `lib$get_symbol`, ... and `include/` ([docs/design/m2.md](docs/design/m2.md)) |
| `vms-examples` | `greet`: one CLD, a Rust and a C program (`examples/greet`) |

The `vms-*` cores are pure (no host I/O). `sys/` holds what VMS keeps in
SYS$SYSROOT: the command tables (`SYSLIB/DCLTABLES`) and the system messages
(`SYSMSG`).

`cargo test` checks the crates against [fixtures](fixtures/README.md)
recorded on OpenVMS Alpha V8.4-2L1, among them DCL procedures whose output
vmsport must reproduce exactly.
