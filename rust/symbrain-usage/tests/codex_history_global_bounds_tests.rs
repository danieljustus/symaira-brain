#![cfg(unix)]
use symbrain_usage::{Cancellation, scan_codex_history};

#[test]
fn codex_history_global_bounds() {
    for mode in ["combined-events", "subtree-bytes"] {
        let root = tempfile::tempdir().unwrap();
        for (index, sub) in ["sessions", "archived_sessions"].iter().enumerate() {
            let path = root.path().join(sub);
            std::fs::create_dir(&path).unwrap();
            if mode == "combined-events" {
                let data = (1..=6000).map(|i| serde_json::json!({"type":"event_msg","timestamp":"2026-10-20T10:00:00Z","payload":{"type":"token_count","model":"gpt-5.4","info":{"last_token_usage":{"input_tokens":i+6000*index,"output_tokens":1}}}}).to_string()+"\n").collect::<String>();
                std::fs::write(path.join(format!("{index}.jsonl")), data).unwrap();
            } else if index == 0 {
                for i in 0..9 {
                    std::fs::File::create(path.join(format!("{i}.jsonl")))
                        .unwrap()
                        .set_len(4 << 20)
                        .unwrap();
                }
            }
        }
        let h = scan_codex_history(
            root.path(),
            &chrono::Utc::now(),
            "fixture",
            &Cancellation::default(),
        );
        assert_eq!(h.status, "unavailable", "{mode}");
        assert!(h.daily.is_empty());
        assert!(h.periods.is_empty());
    }
}
