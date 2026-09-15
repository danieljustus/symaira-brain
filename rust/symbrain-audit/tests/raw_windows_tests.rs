use symbrain_audit::RawJsonlAppender;
use tempfile::tempdir;

#[cfg(any(unix, windows))]
#[test]
fn raw_jsonl_appender_rejects_line_break_variants() {
    let dir = tempdir().expect("tempdir");
    let appender = RawJsonlAppender::new(dir.path().join("audit.log"));
    assert!(appender.append(b"{\n}").is_err());
    assert!(appender.append(b"{\r}").is_err());
    assert!(appender.append(b"{\r\n}").is_err());
}

#[cfg(any(unix, windows))]
#[test]
fn raw_jsonl_appender_rejects_parent_traversal() {
    let dir = tempdir().expect("tempdir");
    let appender = RawJsonlAppender::new(dir.path().join("../audit.log"));
    assert!(appender.append(br#"{"ok":true}"#).is_err());
}

#[cfg(windows)]
#[test]
fn windows_rejects_ads_device_namespaces_superscripts_and_trailing_aliases() {
    let bad_targets = [
        // Device namespaces
        r"\\.\COM1",
        r"\\.\pipe\audit",
        r"\\?\C:\audit.log",
        r"\??\C:\audit.log",
        r"//./C:/audit.log",
        r"//?/C:/audit.log",
        // Standard DOS reserved devices
        r"NUL",
        r"nul.txt",
        r"CON",
        r"con.log",
        r"AUX",
        r"aux.jsonl",
        r"COM1",
        r"com1.dat",
        r"COM9",
        r"LPT1",
        r"lpt1.txt",
        r"CONIN$",
        r"CONOUT$",
        // Superscript COM / LPT reserved names
        r"COM¹",
        r"com¹.txt",
        r"COM²",
        r"com².log",
        r"COM³",
        r"LPT¹",
        r"lpt².dat",
        r"LPT³",
        // Alternate Data Streams (ADS)
        r"audit.log:stream",
        r"audit.log::$DATA",
        r"logs:stream\audit.log",
        // Drive-relative paths
        r"C:audit.log",
        r"C:dir\audit.log",
        r"D:test.log",
        // Trailing dot and space aliases
        r"audit.log.",
        r"audit.log ",
        r"audit.log...",
        r"logs.\audit.log",
        r"logs \audit.log",
        // Directory traversal
        r"..\audit.log",
        r"logs\..\..\audit.log",
    ];
    for target in bad_targets {
        let appender = RawJsonlAppender::new(target);
        assert!(
            appender.append(br#"{"test":true}"#).is_err(),
            "expected target {target} to fail closed"
        );
    }
}

#[cfg(windows)]
#[test]
fn windows_validates_all_components_before_creating_directories() {
    let temp = tempdir().expect("tempdir");
    let never_created = temp.path().join("stage1_dir");
    let invalid_path = never_created.join("invalid:stream").join("audit.log");

    let appender = RawJsonlAppender::new(&invalid_path);
    let res = appender.append(br#"{"test":true}"#);
    assert!(res.is_err(), "appender must reject invalid component");
    assert!(
        !never_created.exists(),
        "directory must NOT be created if any component is invalid"
    );
}

#[cfg(windows)]
#[test]
fn windows_appends_to_nested_absolute_path() {
    use std::fs;
    let dir = tempdir().expect("tempdir");
    let target = dir.path().join("logs").join("deep").join("audit.log");
    let appender = RawJsonlAppender::new(&target);
    appender
        .append(br#"{"event":"windows_append_1"}"#)
        .expect("first append");
    appender
        .append(br#"{"event":"windows_append_2"}"#)
        .expect("second append");

    let content = fs::read_to_string(&target).expect("read audit log");
    let lines = content.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0], r#"{"event":"windows_append_1"}"#);
    assert_eq!(lines[1], r#"{"event":"windows_append_2"}"#);
}

#[cfg(windows)]
#[test]
fn windows_concurrent_appends_preserve_exact_records() {
    use std::collections::HashSet;
    use std::sync::Arc;
    use std::thread;

    let temp = tempdir().expect("tempdir");
    let log_path = temp.path().join("concurrent_windows_audit.log");
    let appender = Arc::new(RawJsonlAppender::new(&log_path));

    let mut expected_set = HashSet::new();
    let mut handles = Vec::new();
    for t in 0..8 {
        for i in 0..20 {
            expected_set.insert(format!(r#"{{"thread":{t},"seq":{i},"data":"win_append"}}"#));
        }
        let app = Arc::clone(&appender);
        handles.push(thread::spawn(move || {
            for i in 0..20 {
                let record = format!(r#"{{"thread":{t},"seq":{i},"data":"win_append"}}"#);
                app.append(record.as_bytes()).expect("append");
            }
        }));
    }
    for handle in handles {
        handle.join().expect("thread join");
    }

    let content = std::fs::read_to_string(&log_path).expect("read audit log");
    let lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines.len(), 8 * 20);

    let actual_set: HashSet<String> = lines.iter().map(|s| (*s).to_string()).collect();
    assert_eq!(actual_set.len(), 8 * 20);
    assert_eq!(actual_set, expected_set);
}

#[cfg(windows)]
#[test]
fn windows_rejects_directory_junctions() {
    let temp = tempdir().expect("tempdir");
    let target_dir = temp.path().join("real_target");
    std::fs::create_dir(&target_dir).expect("create target dir");
    let junction_path = temp.path().join("junction_link");

    let status = std::process::Command::new("cmd")
        .args([
            "/c",
            "mklink",
            "/J",
            junction_path.to_str().expect("junction path str"),
            target_dir.to_str().expect("target dir str"),
        ])
        .status()
        .expect("execute mklink /J");
    assert!(status.success(), "mklink /J must succeed");

    let appender = RawJsonlAppender::new(junction_path.join("audit.log"));
    let result = appender.append(br#"{"event":"junction_attack"}"#);
    assert!(
        result.is_err(),
        "appender must reject path containing directory junction"
    );
    assert!(
        !target_dir.join("audit.log").exists(),
        "outside target file must not be created through directory junction"
    );
}

#[cfg(windows)]
#[test]
fn windows_rejects_file_and_directory_symlinks() {
    let temp = tempdir().expect("tempdir");
    let real_dir = temp.path().join("real_dir");
    std::fs::create_dir(&real_dir).expect("create real dir");
    let real_file = real_dir.join("real_audit.log");
    std::fs::write(&real_file, b"original").expect("create real file");

    let sym_file = temp.path().join("sym_file.log");
    let sym_dir = temp.path().join("sym_dir");

    std::os::windows::fs::symlink_file(&real_file, &sym_file)
        .expect("symlink_file requires SeCreateSymbolicLinkPrivilege or Developer Mode");
    let file_appender = RawJsonlAppender::new(&sym_file);
    assert!(
        file_appender
            .append(br#"{"event":"file_symlink"}"#)
            .is_err(),
        "must reject file symlink target"
    );
    assert_eq!(
        std::fs::read(&real_file).expect("read real file"),
        b"original",
        "outside real file must remain unchanged after rejected file symlink append"
    );

    std::os::windows::fs::symlink_dir(&real_dir, &sym_dir)
        .expect("symlink_dir requires SeCreateSymbolicLinkPrivilege or Developer Mode");
    let dir_appender = RawJsonlAppender::new(sym_dir.join("audit.log"));
    assert!(
        dir_appender.append(br#"{"event":"dir_symlink"}"#).is_err(),
        "must reject directory symlink in path"
    );
    assert!(
        !real_dir.join("audit.log").exists(),
        "outside directory must not have file created through directory symlink"
    );
}
