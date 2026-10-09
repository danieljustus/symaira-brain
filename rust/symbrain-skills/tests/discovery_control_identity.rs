//! Prepared whole-row identity checks for rejected final Unix control links.
#![cfg(unix)]
use sha2::{Digest, Sha256};
use std::{fs, os::unix::fs::symlink};
use symbrain_skills::{discover, library};

#[test]
fn invalid_link_bundle_uses_confined_original_content_for_full_discovery_identity() {
    let owned = tempfile::tempdir().unwrap();
    let bundle = owned.path().join("demo");
    fs::create_dir(&bundle).unwrap();
    let doc = b"---\nname: demo\ndescription: source\n---\nbody\n";
    fs::write(bundle.join("document.md"), doc).unwrap();
    symlink("document.md", bundle.join("SKILL.md")).unwrap();
    let canonical = fs::canonicalize(&bundle).unwrap();
    let mut hash = Sha256::new();
    hash.update(canonical.as_os_str().as_encoded_bytes());
    hash.update([0]);
    hash.update(doc);
    let id = format!("sha256:{:x}", hash.finalize())[..23].to_owned();
    let rows = discover::scanned(
        &owned.path().join("home"),
        None,
        "user",
        &[bundle.to_str().unwrap().into()],
    )
    .unwrap();
    let row = rows.iter().find(|row| row.location == bundle).unwrap();
    assert_eq!(row.source_id, id);
    assert!(!row.valid && !row.managed);
    assert_eq!(row.status, "invalid");
    assert_eq!(row.display_name, "demo");
    assert_eq!(row.kind, "skill_bundle");
    assert_eq!(row.source, "explicit-path");
    assert_eq!(row.target, "");
    assert_eq!(
        row.diagnostics,
        vec!["read SKILL.md: openat SKILL.md: too many levels of symbolic links"]
    );
    let (loaded, issues) = library::list_library(owned.path());
    assert!(loaded.is_empty());
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].code, "skill_load");
    assert_eq!(issues[0].path, "demo");
    assert_eq!(
        issues[0].message,
        "read SKILL.md: openat SKILL.md: too many levels of symbolic links"
    );
    assert_eq!(fs::read(bundle.join("document.md")).unwrap(), doc);
    assert!(
        fs::symlink_metadata(bundle.join("SKILL.md"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
}
