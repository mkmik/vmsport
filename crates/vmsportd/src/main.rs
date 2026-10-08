//! vmsportd [RUNDIR]: the daemon. Clients start it; see the library.

fn main() {
    let dir = std::env::args_os()
        .nth(1)
        .map_or_else(vmsportd::run_dir, Into::into);
    if let Err(e) = vmsportd::serve(&dir) {
        eprintln!("vmsportd: {}: {e}", dir.display());
        std::process::exit(1);
    }
}
