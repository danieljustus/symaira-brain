#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Output};

use serde_json::{Value, json};
use tempfile::TempDir;

struct DoctorFixture {
    root: TempDir,
    home: std::path::PathBuf,
    bin: std::path::PathBuf,
    empty_path: std::path::PathBuf,
}

impl DoctorFixture {
    fn new() -> Self {
        let root = TempDir::new().expect("temporary doctor root");
        let home = root.path().join("home");
        let bin = home.join(".symaira/bin");
        let empty_path = root.path().join("empty-path");
        for path in [
            &home,
            &bin,
            &empty_path,
            &root.path().join("config"),
            &root.path().join("data"),
            &root.path().join("cache"),
            &root.path().join("project"),
        ] {
            fs::create_dir_all(path).expect("create isolated doctor root");
        }
        Self {
            root,
            home,
            bin,
            empty_path,
        }
    }

    fn install_binary(&self, binary: &str) -> std::path::PathBuf {
        let path = self.bin.join(binary);
        fs::write(&path, b"#!/bin/sh\nexit 0\n").expect("write test module binary");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
            .expect("make test module executable");
        path
    }

    fn sidecar_path(&self, binary: &str) -> std::path::PathBuf {
        self.bin.join(format!("{binary}.provenance.json"))
    }

    fn doctor(&self) -> Output {
        Command::new(env!("CARGO_BIN_EXE_symbrain"))
            .env_clear()
            .env("HOME", &self.home)
            .env("USERPROFILE", &self.home)
            .env("XDG_CONFIG_HOME", self.root.path().join("config"))
            .env("XDG_DATA_HOME", self.root.path().join("data"))
            .env("XDG_CACHE_HOME", self.root.path().join("cache"))
            .env("PATH", &self.empty_path)
            .env("LANG", "C.UTF-8")
            .env("LC_ALL", "C.UTF-8")
            .env("TZ", "UTC")
            .current_dir(self.root.path().join("project"))
            .args(["doctor", "--json"])
            .output()
            .expect("run isolated native doctor")
    }

    fn report(&self) -> Value {
        let output = self.doctor();
        assert!(
            output.status.success(),
            "doctor stderr: {:?}",
            output.stderr
        );
        assert!(
            output.stderr.is_empty(),
            "doctor stderr: {:?}",
            output.stderr
        );
        serde_json::from_slice(&output.stdout).expect("doctor JSON")
    }
}

#[test]
fn doctor_reports_installed_optional_modules_and_exact_sidecar_provenance() {
    let fixture = DoctorFixture::new();
    let marker = fixture.root.path().join("module-was-executed");
    for (module, binary) in [
        ("browse", "symbrowse"),
        ("operate", "symoperate"),
        ("scope", "symscope"),
    ] {
        let path = fixture.install_binary(binary);
        fs::write(
            &path,
            format!(
                "#!/bin/sh\nprintf '%s\\n' '{binary}' >> '{}'\nexit 0\n",
                marker.display()
            ),
        )
        .expect("write non-running module executable");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
            .expect("make module executable");
        let sidecar = json!({
            "binary": binary,
            "source": "brain-source",
            "version": "",
            "receiver_commit": "0123456789abcdef0123456789abcdef01234567",
            "module_dir": module,
            "builder": "fixture-builder",
            "built_at": "2026-10-02T10:00:00Z",
            "binary_sha256": format!("{}-{module}", "a".repeat(56)),
        });
        fs::write(
            fixture.sidecar_path(binary),
            serde_json::to_vec_pretty(&sidecar).unwrap(),
        )
        .expect("write provenance fixture");
    }

    let report = fixture.report();
    let modules = report["managed_modules"]
        .as_array()
        .expect("module entries");
    assert_eq!(modules.len(), 3);
    for (index, (module, binary)) in [
        ("browse", "symbrowse"),
        ("operate", "symoperate"),
        ("scope", "symscope"),
    ]
    .into_iter()
    .enumerate()
    {
        let entry = &modules[index];
        assert_eq!(entry["module"], module);
        assert_eq!(entry["binary"], binary);
        assert_eq!(entry["installed"], true);
        assert_eq!(
            entry["path"],
            fixture.bin.join(binary).display().to_string()
        );
        assert_eq!(
            entry["provenance"],
            json!({
                "binary": binary,
                "source": "brain-source",
                "version": "",
                "receiver_commit": "0123456789abcdef0123456789abcdef01234567",
                "module_dir": module,
                "builder": "fixture-builder",
                "built_at": "2026-10-02T10:00:00Z",
                "binary_sha256": format!("{}-{module}", "a".repeat(56)),
            })
        );
    }
    let invoked = fs::read_to_string(marker).unwrap_or_default();
    assert!(
        !invoked
            .lines()
            .any(|binary| matches!(binary, "symoperate" | "symscope")),
        "doctor must not run Operate or Scope binaries: {invoked}"
    );
}

#[test]
fn doctor_omits_missing_modules_and_safely_ignores_missing_malformed_or_special_sidecars() {
    let fixture = DoctorFixture::new();

    // A provenance record alone must never make a missing binary look installed.
    fs::write(
        fixture.sidecar_path("symbrowse"),
        br#"{"binary":"symbrowse","source":"brain-source","version":"","binary_sha256":"deadbeef"}"#,
    )
    .expect("write stray sidecar");

    // An installed binary remains visible without a sidecar, but has no provenance.
    fixture.install_binary("symoperate");

    // Malformed JSON is ignored rather than reported as provenance.
    fixture.install_binary("symscope");
    fs::write(fixture.sidecar_path("symscope"), b"{not json").expect("write malformed sidecar");

    // A FIFO is non-regular; inspection must return without opening/blocking on it.
    let fifo = fixture.sidecar_path("symoperate");
    let status = Command::new("/usr/bin/mkfifo")
        .arg(&fifo)
        .status()
        .expect("run mkfifo fixture helper");
    assert!(status.success(), "mkfifo failed: {status}");

    let report = fixture.report();
    let modules = report["managed_modules"]
        .as_array()
        .expect("installed module entries");
    assert_eq!(modules.len(), 2);
    assert!(!modules.iter().any(|entry| entry["binary"] == "symbrowse"));
    let operate = modules
        .iter()
        .find(|entry| entry["binary"] == "symoperate")
        .expect("installed operate binary");
    assert_eq!(
        operate["path"],
        fixture.bin.join("symoperate").display().to_string()
    );
    assert!(operate.get("provenance").is_none());
    let scope = modules
        .iter()
        .find(|entry| entry["binary"] == "symscope")
        .expect("installed scope binary");
    assert!(scope.get("provenance").is_none());
}

#[test]
fn doctor_omits_empty_optional_module_extension_for_old_json_parity() {
    let fixture = DoctorFixture::new();
    let report = fixture.report();
    assert!(report.get("managed_modules").is_none());
}

#[test]
fn doctor_bounds_provenance_sidecar_reads() {
    let fixture = DoctorFixture::new();
    fixture.install_binary("symbrowse");
    fs::write(fixture.sidecar_path("symbrowse"), vec![b' '; 16 * 1024 + 1])
        .expect("write oversized sidecar");

    let report = fixture.report();
    let modules = report["managed_modules"]
        .as_array()
        .expect("module entries");
    assert_eq!(modules.len(), 1);
    assert!(modules[0].get("provenance").is_none());
}

#[test]
fn doctor_preserves_release_version_repo_and_hash_without_source_build_fields() {
    let fixture = DoctorFixture::new();
    fixture.install_binary("symbrowse");
    let sidecar = json!({
        "binary": "symbrowse",
        "source": "release",
        "version": "v1.2.3",
        "repo": "owner/symbrowse",
        "built_at": "2026-10-02T10:00:00Z",
        "binary_sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
    });
    fs::write(
        fixture.sidecar_path("symbrowse"),
        serde_json::to_vec_pretty(&sidecar).unwrap(),
    )
    .expect("write release provenance");

    let report = fixture.report();
    let module = &report["managed_modules"][0];
    assert_eq!(
        module["path"],
        fixture.bin.join("symbrowse").display().to_string()
    );
    assert_eq!(module["provenance"], sidecar);
}
