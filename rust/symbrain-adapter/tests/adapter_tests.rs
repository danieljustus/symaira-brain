#![allow(missing_docs)]

use serde::Deserialize;
use sha2::{Digest, Sha256};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use symbrain_adapter::{
    all_targets, render_for_harness, target_for_harness, validate_relative_target_path,
};

#[derive(Debug, Deserialize)]
struct Oracle {
    schema_version: u32,
    rollback_baseline: String,
    go_toolchain: String,
    generator_sha256: String,
    provenance: Vec<Provenance>,
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
struct Provenance {
    path: String,
    sha256: String,
    rollback_sha256: String,
    rollback_present: bool,
}

#[derive(Debug, Deserialize)]
struct Case {
    id: String,
    harness: String,
    operation: String,
    #[serde(default)]
    project_dir: String,
    #[serde(default)]
    target_path: String,
    #[serde(default)]
    existing: String,
    #[serde(default)]
    content: String,
    #[serde(default)]
    expected: String,
    #[serde(default)]
    skipped: bool,
}

fn oracle() -> Oracle {
    serde_json::from_slice(include_bytes!("fixtures/oracle_expectations.json"))
        .expect("adapter oracle fixture must be valid JSON")
}

fn decode(value: &str) -> Vec<u8> {
    let mut output = Vec::new();
    let mut accumulator = 0_u32;
    let mut bits = 0_u8;
    for byte in value.bytes() {
        if byte == b'=' {
            break;
        }
        let six = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'\n' | b'\r' => continue,
            _ => panic!("invalid base64 byte {byte}"),
        };
        accumulator = (accumulator << 6) | u32::from(six);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            output.push(u8::try_from(accumulator >> bits).expect("base64 byte fits"));
            accumulator &= (1 << bits) - 1;
        }
    }
    output
}

fn sha256(path: &Path) -> String {
    let bytes = std::fs::read(path).expect("provenance source exists");
    let digest = Sha256::digest(bytes);
    format!("{digest:x}")
}

#[test]
fn oracle_is_pinned_to_current_go_sources_and_generator() {
    let suite = oracle();
    assert_eq!(suite.schema_version, 1);
    assert_eq!(suite.rollback_baseline.len(), 40);
    assert_eq!(suite.go_toolchain, "go1.26.7");
    assert_eq!(suite.provenance.len(), 7);

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repository = root
        .parent()
        .and_then(Path::parent)
        .expect("repository root");
    for source in &suite.provenance {
        assert!(source.rollback_present);
        assert_eq!(source.sha256.len(), 64);
        assert_eq!(source.rollback_sha256.len(), 64);
        assert_eq!(
            sha256(&repository.join(&source.path)),
            source.sha256,
            "source drift in {}",
            source.path
        );
    }
    assert_eq!(
        sha256(&repository.join("scripts/adapters-oracle/main.go")),
        suite.generator_sha256
    );
}

#[test]
fn registry_derives_exactly_four_targets_and_five_skips() {
    let suite = oracle();
    let targets = all_targets();
    assert_eq!(targets.len(), 4);
    assert_eq!(
        targets
            .iter()
            .map(|target| target.harness().as_str())
            .collect::<Vec<_>>(),
        ["claude", "cursor", "antigravity", "agents"]
    );

    let skipped = suite
        .cases
        .iter()
        .filter(|case| case.operation == "skip")
        .collect::<Vec<_>>();
    assert_eq!(skipped.len(), 5);
    assert!(skipped.iter().all(|case| case.skipped));
    assert_eq!(
        skipped
            .iter()
            .map(|case| case.harness.as_str())
            .collect::<Vec<_>>(),
        ["claude-desktop", "opencode", "codex", "hermes", "openclaw"]
    );
    for case in skipped {
        assert!(
            target_for_harness(&case.harness)
                .expect("registered harness")
                .is_none()
        );
    }
}

#[test]
fn oracle_render_cases_match_go_byte_for_byte() {
    let suite = oracle();
    let render_cases = suite
        .cases
        .iter()
        .filter(|case| case.operation == "render")
        .collect::<Vec<_>>();
    assert_eq!(render_cases.len(), 20);
    for case in render_cases {
        let rendered = render_for_harness(
            &case.harness,
            &decode(&case.existing),
            &decode(&case.content),
            Path::new(&case.project_dir),
        )
        .expect("registered adapter")
        .expect("adapter exists");
        assert_eq!(
            rendered.path.to_string_lossy(),
            case.target_path,
            "path {}",
            case.id
        );
        assert_eq!(
            rendered.output,
            decode(&case.expected),
            "output {}",
            case.id
        );
    }
}

#[test]
fn rendering_is_idempotent_and_preserves_user_prefix_and_suffix() {
    let suite = oracle();
    for case in suite.cases.iter().filter(|case| case.operation == "render") {
        let existing = decode(&case.existing);
        let content = decode(&case.content);
        let first = render_for_harness(
            &case.harness,
            &existing,
            &content,
            Path::new(&case.project_dir),
        )
        .expect("registered adapter")
        .expect("adapter exists")
        .output;
        let second = render_for_harness(
            &case.harness,
            &first,
            &content,
            Path::new(&case.project_dir),
        )
        .expect("registered adapter")
        .expect("adapter exists")
        .output;
        assert_eq!(second, first, "case {} is not idempotent", case.id);
        if case.id.ends_with("existing_user_prefix_suffix") {
            assert!(
                first.starts_with(b"user prefix\n"),
                "prefix lost in {}",
                case.id
            );
            assert!(
                first.ends_with(b"user suffix\n"),
                "suffix lost in {}",
                case.id
            );
        }
    }
}

#[test]
fn target_path_validation_rejects_absolute_and_traversal_forms() {
    for path in [
        "",
        "/tmp/AGENTS.md",
        "\\\\server\\share\\AGENTS.md",
        "../AGENTS.md",
        "nested/../../AGENTS.md",
        r"..\AGENTS.md",
        r"nested\..\AGENTS.md",
        r"C:\Users\ada\AGENTS.md",
        "C:/Users/ada/AGENTS.md",
    ] {
        assert!(
            validate_relative_target_path(Path::new(path)).is_err(),
            "accepted unsafe target {path:?}"
        );
    }
    for path in [
        "AGENTS.md",
        ".cursor/rules/symbrain.mdc",
        "nested/AGENTS.md",
    ] {
        assert!(validate_relative_target_path(Path::new(path)).is_ok());
    }
}

#[test]
fn atomic_write_preserves_mode_and_rejects_symlink_parents() {
    let temp = tempfile::tempdir().expect("temporary project");
    let target = temp.path().join("AGENTS.md");
    std::fs::write(&target, b"old").expect("existing target");
    #[cfg(unix)]
    {
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o640))
            .expect("existing mode");
    }
    symbrain_adapter::write_atomic(temp.path(), Path::new("AGENTS.md"), b"new")
        .expect("atomic replacement");
    assert_eq!(std::fs::read(&target).expect("target bytes"), b"new");
    #[cfg(unix)]
    assert_eq!(
        std::fs::metadata(&target)
            .expect("target metadata")
            .permissions()
            .mode()
            & 0o777,
        0o640
    );

    #[cfg(unix)]
    let fresh = temp.path().join("fresh.md");
    symbrain_adapter::write_atomic(temp.path(), Path::new("fresh.md"), b"fresh")
        .expect("atomic creation");
    #[cfg(unix)]
    assert_eq!(
        std::fs::metadata(&fresh)
            .expect("fresh metadata")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );

    let nested_project = temp.path().join("nested-project");
    let cursor = target_for_harness("cursor")
        .expect("registered harness")
        .expect("cursor adapter");
    let nested_rendered = cursor
        .write(b"", b"nested content\n", &nested_project)
        .expect("create adapter parent directories");
    assert_eq!(
        std::fs::read(&nested_rendered.path).expect("nested target bytes"),
        nested_rendered.output
    );
    #[cfg(unix)]
    {
        let cursor_dir = nested_project.join(".cursor");
        let rules_dir = cursor_dir.join("rules");
        for directory in [
            nested_project.as_path(),
            cursor_dir.as_path(),
            rules_dir.as_path(),
        ] {
            assert_eq!(
                std::fs::metadata(directory)
                    .expect("created parent metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
        }
    }

    #[cfg(unix)]
    {
        let outside = tempfile::tempdir().expect("outside directory");
        let link = temp.path().join("rules");
        std::os::unix::fs::symlink(outside.path(), &link).expect("symlink parent");

        assert!(
            symbrain_adapter::write_atomic(temp.path(), Path::new("rules/escape.md"), b"no escape")
                .is_err()
        );
        assert!(!outside.path().join("escape.md").exists());
    }
}

#[cfg(unix)]
#[test]
fn atomic_write_retains_parent_capability_after_replacement() {
    let temp = tempfile::tempdir().expect("temporary project");
    let parent = temp.path().join("rules");
    let moved = temp.path().join("moved-rules");
    let outside = tempfile::tempdir().expect("outside directory");
    std::fs::create_dir(&parent).expect("parent directory");
    let atomic =
        symbrain_adapter::AtomicFile::open(temp.path(), Path::new("rules/instructions.md"), false)
            .expect("retained parent capability");
    std::fs::rename(&parent, &moved).expect("replace parent path");
    std::os::unix::fs::symlink(outside.path(), &parent).expect("replacement symlink");
    atomic
        .write(b"retained")
        .expect("write through retained capability");
    assert_eq!(
        std::fs::read(moved.join("instructions.md")).unwrap(),
        b"retained"
    );
    assert!(!outside.path().join("instructions.md").exists());
}

#[test]
fn unknown_harness_is_not_silently_skipped() {
    assert!(target_for_harness("toolbox").is_err());
    assert!(render_for_harness("toolbox", b"", b"content", Path::new("/project")).is_err());
}

#[cfg(unix)]
#[test]
fn atomic_write_preserves_special_mode_and_extended_attribute() {
    use rustix::fs::{self, XattrFlags};
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;

    let temp = tempfile::tempdir().expect("temporary project");
    let target = temp.path().join("metadata.md");
    std::fs::write(&target, b"old").expect("existing target");
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o4751))
        .expect("special mode");
    let name = if cfg!(target_os = "macos") {
        OsStr::from_bytes(b"com.apple.metadata:_kMDItemUserTags")
    } else {
        OsStr::from_bytes(b"user.symbrain.rust-test")
    };
    if let Err(error) = fs::setxattr(&target, name, b"phase6", XattrFlags::empty()) {
        eprintln!("skipping xattr preservation test: {error}");
        return;
    }

    symbrain_adapter::write_atomic(temp.path(), Path::new("metadata.md"), b"new")
        .expect("atomic metadata replacement");
    assert_eq!(
        std::fs::metadata(&target).expect("target metadata").mode() & 0o7777,
        0o4751
    );
    let mut value = [0_u8; 64];
    let length = fs::getxattr(&target, name, &mut value).expect("copied xattr");
    assert_eq!(&value[..length], b"phase6");
}
