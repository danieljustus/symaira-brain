//! Actual Windows PE execution/refusal, independent of Go and ambient tools.
#![cfg(windows)]

use std::path::Path;
use std::process::Command;
use symbrain_managed::installed_version;

fn fixture(root: &Path) -> std::path::PathBuf {
    let source = root.join("probe.rs");
    let executable = root.join("fixture.exe");
    std::fs::write(
        &source,
        r#"
fn main() {
    let executable = std::env::current_exe().unwrap();
    let marker = executable.parent().unwrap().join("executed.txt");
    std::fs::write(marker, executable.file_name().unwrap().to_string_lossy().as_bytes()).unwrap();
    println!("{{\"version\":\"0.22.1\"}}");
}
"#,
    )
    .expect("fixture source");
    let compiler = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    assert!(
        Command::new(compiler)
            .arg("--edition=2024")
            .arg(&source)
            .arg("-o")
            .arg(&executable)
            .status()
            .expect("compile fixture")
            .success()
    );
    executable
}

#[test]
fn managed_version_checks_original_name_before_windows_extension_lookup() {
    let root = tempfile::tempdir().expect("fixture root");
    let executable = fixture(root.path());
    let marker = root.path().join("executed.txt");
    let raw = root.path().join("probe");
    let exe = root.path().join("probe.exe");

    // The shipped Go first stats the original name. An .exe alone is absent.
    std::fs::copy(&executable, &exe).expect("exe copy");
    assert_eq!(
        installed_version(root.path(), "probe").expect("absent original"),
        ""
    );
    assert!(!marker.exists());
    std::fs::remove_file(&exe).expect("remove candidate");

    // An extensionless PE is runnable by CreateProcess but not Go's default
    // PATHEXT lookup. It must not be mistaken for an already-correct install.
    std::fs::copy(&executable, &raw).expect("extensionless copy");
    assert!(installed_version(root.path(), "probe").is_err());
    assert!(!marker.exists());

    // With both names present the executable candidate is the actual child.
    std::fs::copy(&executable, &exe).expect("candidate copy");
    assert_eq!(
        installed_version(root.path(), "probe").expect("resolved version"),
        "0.22.1"
    );
    assert_eq!(
        std::fs::read(&marker).expect("execution receipt"),
        b"probe.exe"
    );
    std::fs::remove_file(&marker).expect("clear marker");
    assert_eq!(
        installed_version(root.path(), "probe.exe").expect("explicit extension"),
        "0.22.1"
    );
    assert_eq!(
        std::fs::read(&marker).expect("explicit execution receipt"),
        b"probe.exe"
    );
}
