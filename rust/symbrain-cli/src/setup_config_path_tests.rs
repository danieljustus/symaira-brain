use super::printed;
use std::path::Path;

#[test]
fn printed_leaves_unix_path_bytes_unchanged() {
    let unclean = Path::new("/tmp//a/./b/../c/symbrain/config.toml");
    assert_eq!(printed(unclean), b"/tmp//a/./b/../c/symbrain/config.toml");
}
