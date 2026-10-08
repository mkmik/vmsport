//! cldcheck FILE.CLD...: compiles each file and says what it defines.
fn main() {
    let mut bad = false;
    for f in std::env::args().skip(1) {
        let src = std::fs::read_to_string(&f).unwrap();
        match vms_cld::compile(&src) {
            Ok(t) => println!(
                "{f}: {} verbs, {} syntaxes, {} types",
                t.verbs.len(),
                t.syntaxes.len(),
                t.types.len()
            ),
            Err(e) => {
                println!("{f}: {e}");
                bad = true;
            }
        }
    }
    std::process::exit(bad as i32);
}
