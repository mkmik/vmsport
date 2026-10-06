# vmsport

The OpenVMS userland as plain processes and libraries on macOS and Linux:
see [docs/PRD.md](docs/PRD.md).

## Crates (M0: pure cores, no host I/O)

| Crate | What |
| --- | --- |
| `vms-filespec` | `NODE::DEV:[DIR]NAME.TYP;VER` with ODS-5 extended names |
| `vms-cond` | 32-bit condition values |
| `vms-fao` | `$FAO` |
| `vms-msg` | `.MSG` compiler, message files, `$GETMSG`, `$PUTMSG` |
| `vms-cld` | CLD compiler, DCL command-line parsing, `CLI$PRESENT`, `CLI$GET_VALUE` |

`cargo test` checks them against [fixtures](fixtures/README.md) recorded on
OpenVMS Alpha V8.4-2L1.
