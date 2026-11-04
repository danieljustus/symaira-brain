//! Independently usable Guard entrypoint; no Brain gateway or Go fallback.
use std::io;

fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if !symaira_cutover::run_for_binary("symguard", |homes| {
        vec![
            symaira_cutover::Root {
                label: "config",
                path: homes.config.join("symguard"),
            },
            symaira_cutover::Root {
                label: "data",
                path: homes.data.join("symguard"),
            },
        ]
    }) {
        std::process::exit(1);
    }
    let code =
        symguard_cli::run_standalone(&args, &mut io::stdout().lock(), &mut io::stderr().lock());
    std::process::exit(i32::from(code));
}
