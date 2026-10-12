//! Actual private filesystem regressions; no operator paths or writes by scans.
use std::fs;
use symbrain_skills::{discover, library};

#[test]
fn explicit_regular_missing_directory_and_dangling_link_are_classified() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    fs::create_dir(&home).unwrap();
    let file = root.path().join("regular");
    fs::write(&file, b"not a directory").unwrap();
    let missing = root.path().join("missing");
    let folder = root.path().join("empty");
    fs::create_dir(&folder).unwrap();
    let mut paths = vec![
        file.display().to_string(),
        missing.display().to_string(),
        folder.display().to_string(),
    ];
    let dangling = root.path().join("dangling");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("absent", &dangling).unwrap();
        paths.push(dangling.display().to_string());
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_dir("absent", &dangling).unwrap();
        paths.push(dangling.display().to_string());
    }
    let rows = discover::scanned(&home, Some(root.path()), "user", &paths).unwrap();
    assert!(
        !rows
            .iter()
            .any(|row| row.location == file || row.location == folder)
    );
    let row = rows.iter().find(|row| row.location == missing).unwrap();
    assert_eq!(row.status, "unreadable");
    assert!(!row.valid && !row.managed);
    #[cfg(unix)]
    assert_eq!(
        row.diagnostics,
        vec![format!(
            "path not accessible: lstat {}: no such file or directory",
            missing.display()
        )]
    );
    #[cfg(windows)]
    assert_eq!(
        row.diagnostics,
        vec![format!(
            "path not accessible: GetFileAttributesEx {}: The system cannot find the file specified.",
            missing.display()
        )]
    );
    {
        let row = rows.iter().find(|row| row.location == dangling).unwrap();
        assert_eq!(row.status, "unreadable");
        #[cfg(unix)]
        assert_eq!(
            row.diagnostics,
            vec![format!(
                "read directory: open {}: no such file or directory",
                dangling.display()
            )]
        );
        #[cfg(windows)]
        assert_eq!(
            row.diagnostics,
            vec![format!(
                "read directory: open {}: The system cannot find the file specified.",
                dangling.display()
            )]
        );
        assert!(
            fs::symlink_metadata(&dangling)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }
    assert_eq!(fs::read(file).unwrap(), b"not a directory");
    assert!(!missing.exists());
}

#[test]
fn missing_document_issue_keeps_bundle_relative_label_and_good_sibling() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("empty")).unwrap();
    fs::create_dir(root.path().join("good")).unwrap();
    fs::write(
        root.path().join("good/SKILL.md"),
        "---\nname: good\ndescription: valid\n---\nbody\n",
    )
    .unwrap();
    let (loaded, issues) = library::list_library(root.path());
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].name, "good");
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].code, "skill_load");
    assert_eq!(issues[0].severity, "error");
    assert_eq!(issues[0].path, "empty");
    #[cfg(unix)]
    assert_eq!(
        issues[0].message,
        "read SKILL.md: openat SKILL.md: no such file or directory"
    );
    #[cfg(windows)]
    assert_eq!(
        issues[0].message,
        "read SKILL.md: openat SKILL.md: The system cannot find the file specified."
    );
    assert_eq!(fs::read_dir(root.path().join("empty")).unwrap().count(), 0);
}
