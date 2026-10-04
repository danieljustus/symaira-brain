//! Repairs run without Go; origin corruption and source builds remain untouched.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[test]
fn missing_managed_home_fails_before_config_probes_or_publication() {
    let root = tempfile::tempdir().unwrap();
    let fallback = root.path().join("fallback-owner");
    fs::create_dir(&fallback).unwrap();
    let config = root.path().join("config/symbrain");
    fs::create_dir_all(&config).unwrap();
    fs::write(config.join("config.toml"), b"[bad config").unwrap();
    let before = snapshot(root.path());
    #[cfg(windows)]
    let (home_key, label) = ("USERPROFILE", "%userprofile%");
    #[cfg(not(windows))]
    let (home_key, label) = ("HOME", "$HOME");
    for empty in [false, true] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_symbrain"));
        command
            .args(["doctor", "--fix"])
            .env_clear()
            .current_dir(root.path())
            .env("XDG_CONFIG_HOME", root.path().join("config"))
            .env("PATH", root.path().join("no-tools"))
            .env("SYMBRAIN_GO_BINARY", root.path().join("absent-go"));
        if empty {
            command.env(home_key, "");
        }
        #[cfg(windows)]
        {
            let owner = fallback.to_str().unwrap();
            let (drive, path) = owner.split_at(2);
            assert!(drive.ends_with(':') && path.starts_with('\\'));
            command.env("HOMEDRIVE", drive).env("HOMEPATH", path);
            for key in ["SystemRoot", "windir", "ComSpec"] {
                if let Some(value) = std::env::var_os(key) {
                    command.env(key, value);
                }
            }
        }
        let output = command.output().unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert_eq!(
            output.stderr,
            format!("symbrain doctor --fix: managed: cannot determine home directory: {label} is not defined\n").as_bytes()
        );
        assert_eq!(snapshot(root.path()), before);
        assert!(!fallback.join(".symaira").exists());
    }
}

#[test]
fn doctor_repair_preserves_source_and_corruption_and_continues_failures() {
    let root = tempfile::tempdir().unwrap();
    let child = root
        .path()
        .join(format!("doctor-child{}", std::env::consts::EXE_SUFFIX));
    let output = Command::new("rustc")
        .arg("--edition=2024")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/setup_child.rs"))
        .arg("-o")
        .arg(&child)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let manifest = symbrain_managed::Manifest::load_embedded().unwrap();
    let platform = symbrain_managed::Platform::current().unwrap();
    let attempted = manifest
        .cores
        .values()
        .filter(|core| core.supports_platform(platform.os))
        .count();
    for (index, provenance) in [
        br#"{"source":"brain-source","receiver_commit":"fixture commit=123"}"#.as_slice(),
        b"{bad".as_slice(),
        br#"{"source":"brain-source","receiver_commit":42}"#.as_slice(),
    ]
    .into_iter()
    .enumerate()
    {
        let case = root.path().join(format!("preserve-{index}"));
        let bin = prepare(&case, &child, provenance);
        let before = snapshot(&bin);
        let result = run(
            &case,
            &["--fix=TRUE", "--json", "--force-release=FALSE"],
            None,
        );
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let text = String::from_utf8(result.stderr).unwrap();
        assert!(!text.contains("cannot probe"), "{text}");
        assert!(text.contains(&format!(
            "doctor --fix complete repaired=0 skipped={attempted} failed=0"
        )));
        assert!(
            text.contains("binary=symbrowse"),
            "optional core was omitted: {text}"
        );
        if index == 0 {
            assert!(text.contains("receiver_commit=\"fixture commit=123\""));
        } else {
            assert!(text.contains("cannot read provenance; leaving binary untouched"));
        }
        assert_eq!(snapshot(&bin), before, "repair changed protected bytes");

        let forced = run(&case, &["--fix", "--force-release"], None);
        assert_eq!(forced.status.code(), Some(1));
        let text = String::from_utf8(forced.stderr).unwrap();
        assert!(text.contains(&format!(
            "doctor --fix complete repaired=0 skipped=0 failed={attempted}"
        )));
        assert!(text.contains(&format!(
            "managed: {attempted}/{attempted} cores failed to repair"
        )));
        assert_eq!(
            text.matches("ERROR repair failed binary=").count(),
            attempted
        );
        assert!(!text.contains("cannot read provenance"));
        assert_eq!(
            snapshot(&bin),
            before,
            "failed downloads changed original bytes"
        );
    }
    config_failures_keep_go_boundary(root.path(), &child);
}

fn prepare(case: &Path, child: &Path, provenance: &[u8]) -> PathBuf {
    let config = case.join("config/symbrain");
    fs::create_dir_all(&config).unwrap();
    fs::write(config.join("config.toml"), b"[modules]\nbrowse = true\n").unwrap();
    let bin = case.join("home/.symaira/bin");
    fs::create_dir_all(&bin).unwrap();
    for core in symbrain_managed::Manifest::load_embedded()
        .unwrap()
        .cores
        .values()
    {
        let path = bin.join(&core.binary_name);
        fs::copy(child, &path).unwrap();
        #[cfg(windows)]
        fs::copy(child, path.with_extension("exe")).unwrap();
        fs::write(
            bin.join(format!("{}.provenance.json", core.binary_name)),
            provenance,
        )
        .unwrap();
    }
    bin
}

fn run(case: &Path, flags: &[&str], fallback: Option<&Path>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_symbrain"));
    command
        .arg("doctor")
        .args(flags)
        .env_clear()
        .current_dir(case)
        .env("HOME", case.join("home"))
        .env("USERPROFILE", case.join("home"))
        .env("XDG_CONFIG_HOME", case.join("config"))
        .env("XDG_DATA_HOME", case.join("data"))
        .env("XDG_CACHE_HOME", case.join("cache"))
        .env("PATH", case.join("no-tools"))
        .env(
            "SYMBRAIN_GO_BINARY",
            fallback.map_or_else(|| case.join("no-go"), Path::to_path_buf),
        )
        .env("SETUP_FALLBACK_RECEIPT", case.join("go-call"))
        .env("SYMBRAIN_RELEASE_BASE_URL", "http://127.0.0.1:1");
    for variable in ["SystemRoot", "WINDIR", "PATHEXT", "LLVM_PROFILE_FILE"] {
        if let Some(value) = std::env::var_os(variable) {
            command.env(variable, value);
        }
    }
    command.output().unwrap()
}

fn config_failures_keep_go_boundary(root: &Path, child: &Path) {
    for (index, text) in [
        "[audit]\nenabled = []",
        "[modules]\nbrowse = 1",
        "invalid = [",
    ]
    .into_iter()
    .enumerate()
    {
        let case = root.join(format!("bad-config-{index}"));
        let config = case.join("config/symbrain");
        fs::create_dir_all(&config).unwrap();
        fs::write(config.join("config.toml"), text).unwrap();
        let before = snapshot(&case);
        let output = run(&case, &["--fix", "--force-release"], Some(child));
        assert_eq!(output.status.code(), Some(2));
        assert_eq!(output.stdout, b"symbrain doctor --fix\n\n");
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(
            stderr.starts_with("  ✗  config: failed to load "),
            "{stderr}"
        );
        assert!(
            stderr.contains(": global config error: failed to "),
            "{stderr}"
        );
        let detail = match index {
            0 => "field \"audit\": field \"enabled\": cannot convert []interface {} to bool",
            1 => "field \"modules\": field \"browse\": cannot convert int64 to bool",
            _ => "unclosed array",
        };
        assert!(stderr.contains(detail), "{stderr}");
        assert!(!case.join("go-call").exists(), "Go fallback was invoked");
        assert_eq!(snapshot(&case), before);
        assert!(!case.join("home/.symaira/bin").exists());
    }
}

fn snapshot(directory: &Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    fn visit(
        root: &Path,
        directory: &Path,
        entries: &mut std::collections::BTreeMap<String, Vec<u8>>,
    ) {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            let name = path
                .strip_prefix(root)
                .unwrap()
                .to_str()
                .unwrap()
                .to_owned();
            if path.is_dir() {
                entries.insert(format!("{name}/"), Vec::new());
                visit(root, &path, entries);
            } else {
                entries.insert(name, fs::read(&path).unwrap());
            }
        }
    }
    let mut entries = std::collections::BTreeMap::new();
    visit(directory, directory, &mut entries);
    entries
}
