#![cfg(unix)]
use symbrain_usage::{Cancellation, scan_codex_history};
#[test]
fn codex_history_native_bounds() {
    if std::env::var_os("CODEX_HISTORY_SOCKET_CHILD").is_some() {
        drop(std::os::unix::net::UnixListener::bind("session.jsonl").unwrap());
        return;
    }

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
        "comma",
        "socket",
    ] {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("sessions");
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
                let lines=(1..=10001).map(|i| serde_json::json!({"type":"event_msg","timestamp":"2026-10-20T10:00:00Z","payload":{"type":"token_count","model":"gpt-5.4","info":{"last_token_usage":{"input_tokens":i,"output_tokens":1}}}}).to_string()+"\n").collect::<String>();
                std::fs::write(&path, lines).unwrap();
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
            "comma" => dir = std::path::PathBuf::from(format!("{},other", dir.display())),
            "socket" => {
                assert!(
                    std::process::Command::new(std::env::current_exe().unwrap())
                        .current_dir(&project)
                        .env("CODEX_HISTORY_SOCKET_CHILD", "1")
                        .args(["codex_history_native_bounds", "--exact"])
                        .status()
                        .unwrap()
                        .success()
                );
            }
            "dotdot" => {
                dir = root
                    .path()
                    .join("../")
                    .join(root.path().file_name().unwrap());
            }
            _ => unreachable!(),
        }
        let h = scan_codex_history(&dir, &chrono::Utc::now(), "fixture", &cancel);
        assert_eq!(h.status, "unavailable", "{mode}");
        assert!(h.daily.is_empty());
        assert!(h.periods.is_empty());
    }
}
