use std::ffi::OsString;
use std::fs;
use std::path::Path;
use std::process::Command;

#[test]
fn source_installs_are_preserved_without_a_go_binary_or_release_host() {
    let root = tempfile::tempdir().unwrap();
    let child = root
        .path()
        .join(format!("setup-child{}", std::env::consts::EXE_SUFFIX));
    let compiled = Command::new("rustc")
        .args([
            OsString::from("--edition=2024"),
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/support/setup_child.rs")
                .into_os_string(),
            OsString::from("-o"),
            child.as_os_str().to_owned(),
        ])
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    let manifest = symbrain_managed::Manifest::load_embedded().unwrap();
    for (index, flags) in [
        vec!["--fix", "--json"],
        vec!["--fix=TRUE", "--json=1", "--force-release=FALSE"],
        vec!["--fix=t", "--allow-unsigned=T", "--json"],
        vec!["--fix"],
        vec!["--fix", "--json", "positional", "--force-release"],
    ]
    .into_iter()
    .enumerate()
    {
        let case_root = root.path().join(format!("case-{index}"));
        let home = case_root.join("home");
        let bin = home.join(".symaira/bin");
        fs::create_dir_all(&bin).unwrap();
        for core in manifest.cores.values().filter(|core| !core.optional) {
            fs::copy(&child, bin.join(&core.binary_name)).unwrap();
            fs::write(
                bin.join(format!("{}.provenance.json", core.binary_name)),
                b"{\"source\":\"brain-source\",\"receiver_commit\":\"fixture\"}\n",
            )
            .unwrap();
        }
        let before = snapshot(&bin);
        let mut command = Command::new(env!("CARGO_BIN_EXE_symbrain"));
        command
            .arg("setup")
            .args(&flags)
            .env_clear()
            .current_dir(&case_root)
            .env("HOME", &home)
            .env("USERPROFILE", &home)
            .env("XDG_CONFIG_HOME", case_root.join("config"))
            .env("XDG_DATA_HOME", case_root.join("data"))
            .env("XDG_CACHE_HOME", case_root.join("cache"))
            .env("PATH", case_root.join("no-tools"))
            .env("SYMBRAIN_GO_BINARY", case_root.join("no-go"))
            .env("SYMBRAIN_RELEASE_BASE_URL", "http://127.0.0.1:1");
        for variable in ["SystemRoot", "WINDIR", "LLVM_PROFILE_FILE"] {
            if let Some(value) = std::env::var_os(variable) {
                command.env(variable, value);
            }
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{flags:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
        if flags.iter().any(|flag| flag.starts_with("--json")) {
            let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(report["results"].as_array().unwrap().len(), 3);
            assert!(
                report["results"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|row| row["status"] == "skipped")
            );
            assert!(report.get("errors").is_none());
        } else {
            let text = String::from_utf8(output.stdout).unwrap();
            assert!(text.contains("brain-source build; use --force-release to replace"));
            assert!(text.ends_with("\n0 fixed, 3 already correct\n"));
        }
        assert_eq!(
            snapshot(&bin),
            before,
            "repair changed an intentional build"
        );
    }
    invalid_configuration_keeps_the_go_loader_boundary(&child, root.path());
}

fn invalid_configuration_keeps_the_go_loader_boundary(child: &Path, root: &Path) {
    for (index, config) in [
        "[audit]\nenabled = []\n",
        "[modules]\nbrowse = 1\n",
        "invalid = [",
    ]
    .into_iter()
    .enumerate()
    {
        let case = root.join(format!("config-error-{index}"));
        let config_dir = case.join("config/symbrain");
        fs::create_dir_all(&config_dir).unwrap();
        fs::write(config_dir.join("config.toml"), config).unwrap();
        let receipt = case.join("go-call");
        let mut command = Command::new(env!("CARGO_BIN_EXE_symbrain"));
        command
            .args(["setup", "--fix", "--json"])
            .env_clear()
            .current_dir(&case)
            .env("HOME", case.join("home"))
            .env("USERPROFILE", case.join("home"))
            .env("XDG_CONFIG_HOME", case.join("config"))
            .env("XDG_DATA_HOME", case.join("data"))
            .env("XDG_CACHE_HOME", case.join("cache"))
            .env("PATH", case.join("no-tools"))
            .env("SYMBRAIN_GO_BINARY", child)
            .env("SETUP_FALLBACK_RECEIPT", &receipt);
        for variable in ["SystemRoot", "WINDIR", "LLVM_PROFILE_FILE"] {
            if let Some(value) = std::env::var_os(variable) {
                command.env(variable, value);
            }
        }
        assert_eq!(command.output().unwrap().status.code(), Some(42));
        assert_eq!(fs::read_to_string(receipt).unwrap(), "setup\n--fix\n--json");
        assert!(!case.join("home/.symaira/bin").exists());
    }
}

fn snapshot(directory: &Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    fs::read_dir(directory)
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            (
                path.file_name().unwrap().to_string_lossy().into_owned(),
                fs::read(path).unwrap(),
            )
        })
        .collect()
}
