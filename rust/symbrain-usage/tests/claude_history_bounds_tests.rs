#![cfg(unix)]
use symbrain_usage::{Cancellation, scan_claude_history};
#[test]
fn claude_history_native_bounds() {
    for mode in [
        "file-size",
        "line-size",
        "entry-count",
        "file-count",
        "line-count",
        "event-count",
        "depth",
        "symlink-file",
        "symlink-ancestor",
        "fifo",
        "cancel",
        "relative",
        "dotdot",
    ] {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("projects");
        std::fs::create_dir(&project).unwrap();
        let path = project.join("session.jsonl");
        let mut dir = root.path().to_owned();
        let cancel = Cancellation::default();
        match mode {
            "file-size" => std::fs::File::create(&path)
                .unwrap()
                .set_len((4 << 20) + 1)
                .unwrap(),
            "line-size" => std::fs::write(&path, "x".repeat((256 << 10) + 1) + "\n").unwrap(),
            "entry-count" => {
                for i in 0..4097 {
                    std::fs::write(project.join(i.to_string()), "").unwrap();
                }
            }
            "file-count" => {
                for i in 0..129 {
                    std::fs::write(project.join(format!("{i:03}.jsonl")), "").unwrap();
                }
            }
            "line-count" => std::fs::write(&path, "{}\n".repeat(20001)).unwrap(),
            "event-count" => {
                let cases: serde_json::Value =
                    serde_json::from_str(include_str!("fixtures/claude-history-cases.json"))
                        .unwrap();
                std::fs::write(&path, cases[0]["source"].as_str().unwrap().repeat(10001)).unwrap();
            }
            "depth" => std::fs::create_dir_all(project.join("sub/".repeat(13))).unwrap(),
            "symlink-file" => std::os::unix::fs::symlink("absent", &path).unwrap(),
            "symlink-ancestor" => {
                dir = root.path().join("link");
                std::os::unix::fs::symlink(root.path(), &dir).unwrap();
            }
            "fifo" => assert!(
                std::process::Command::new("/usr/bin/mkfifo")
                    .arg(&path)
                    .status()
                    .unwrap()
                    .success()
            ),
            "cancel" => cancel.cancel(),
            "relative" => dir = "relative".into(),
            "dotdot" => {
                dir = root
                    .path()
                    .join("../")
                    .join(root.path().file_name().unwrap());
            }
            _ => unreachable!(),
        }
        let h = scan_claude_history(&dir, &chrono::Utc::now(), "fixture", &cancel);
        assert_eq!(h.status, "unavailable", "{mode}");
        assert!(h.daily.is_empty());
        assert!(h.periods.is_empty());
    }
}
