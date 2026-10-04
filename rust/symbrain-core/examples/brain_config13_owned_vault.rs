//! Owned no-service child: records the actually selected executable and argv.
use std::io::Write;

fn main() {
    let root = std::path::PathBuf::from(std::env::var_os("CONFIG13_CONTROL_ROOT").unwrap());
    let capture = std::path::PathBuf::from(std::env::var_os("CONFIG13_VAULT_CAPTURE").unwrap());
    assert!(root.is_absolute() && capture.starts_with(&root));
    let executable = std::env::current_exe().unwrap();
    let label = executable.file_stem().unwrap().to_str().unwrap();
    let argv: Vec<Vec<u8>> = std::env::args_os()
        .skip(1)
        .map(|arg| symbrain_core::go_path::os_bytes(&arg))
        .collect();
    std::fs::write(capture, serde_json::to_vec(&(label, argv)).unwrap()).unwrap();
    writeln!(std::io::stdout(), "owned-vault:{label}").unwrap();
}
