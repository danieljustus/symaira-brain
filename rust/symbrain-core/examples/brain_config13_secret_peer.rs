//! Actual owned credential subprocess; all response bytes come from its fixture.
use std::{io::Write, path::PathBuf};

fn main() {
    let root = PathBuf::from(std::env::var_os("OWNED_JWT_PEER_ROOT").expect("owned root"));
    assert!(root.is_absolute() && root.is_dir());
    let args: Vec<_> = std::env::args_os()
        .skip(1)
        .map(|arg| symbrain_core::go_path::os_bytes(&arg))
        .collect();
    std::fs::write(
        root.join("actual-argv.json"),
        serde_json::to_vec(&args).unwrap(),
    )
    .unwrap();
    let code: i32 = std::fs::read_to_string(root.join("exit"))
        .unwrap()
        .parse()
        .unwrap();
    std::io::stdout()
        .write_all(&std::fs::read(root.join("stdout")).unwrap())
        .unwrap();
    std::io::stderr()
        .write_all(&std::fs::read(root.join("stderr")).unwrap())
        .unwrap();
    std::process::exit(code);
}
