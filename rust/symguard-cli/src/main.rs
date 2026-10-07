//! Independently usable Guard entrypoint; no Brain gateway or Go fallback.
use std::io;

fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let code =
        symguard_cli::run_standalone(&args, &mut io::stdout().lock(), &mut io::stderr().lock());
    std::process::exit(i32::from(code));
}
