use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use symbrowse_daemon::{SessionRegistry, SessionRegistryOptions};

fn timestamp_state(started_at: &str, last_activity: &str) -> (bool, bool) {
    let started =
        time::OffsetDateTime::parse(started_at, &time::format_description::well_known::Rfc3339);
    let activity = time::OffsetDateTime::parse(
        last_activity,
        &time::format_description::well_known::Rfc3339,
    );
    match (started, activity) {
        (Ok(started), Ok(activity)) => (true, activity >= started),
        _ => (false, false),
    }
}

fn main() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time after epoch")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "symbrowse-session-oracle-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir(&root).expect("create isolated registry root");
    let unclean_root = root.join("unused").join("..");
    let registry = SessionRegistry::new(SessionRegistryOptions {
        user_data_root: unclean_root,
        pid: 4242,
        scope: "worktree".into(),
        origin_path: "/workspace/project".into(),
    });

    let invalid_name_error = registry
        .ensure("../escape")
        .expect_err("invalid session name rejected")
        .to_string();
    registry.ensure("beta").expect("ensure beta");
    let alpha = registry.ensure("alpha").expect("ensure alpha");
    let again = registry.ensure("alpha").expect("ensure alpha again");
    let ensure_idempotent = again.name == alpha.name
        && again.started_at == alpha.started_at
        && again.user_data_dir == alpha.user_data_dir
        && again.browser_context_id == alpha.browser_context_id;
    registry
        .set_active_tabs("alpha", 3)
        .expect("set active tabs");
    registry
        .set_ref("alpha", "selector", "@element-1")
        .expect("set ref");
    let reference = registry.reference("alpha", "selector").expect("read ref");
    registry.touch("alpha").expect("touch alpha");

    let data = registry.list_data();
    let sessions: Vec<_> = data
        .sessions
        .iter()
        .map(|info| {
            let (timestamp_format_valid, last_activity_not_earlier) =
                timestamp_state(&info.started_at, &info.last_activity);
            serde_json::json!({
                "name": info.name,
                "pid": info.pid,
                "active_tabs": info.active_tabs,
                "user_data_dir_basename": PathBuf::from(&info.user_data_dir)
                    .file_name().and_then(|name| name.to_str()).unwrap_or_default(),
                "user_data_path_is_clean": PathBuf::from(&info.user_data_dir)
                    == registry.user_data_root().join(&info.name),
                "browser_context_id": info.browser_context_id,
                "ref_count": info.ref_count,
                "scope": info.scope,
                "origin_path": info.origin_path,
                "timestamp_format_valid": timestamp_format_valid,
                "last_activity_not_earlier": last_activity_not_earlier,
            })
        })
        .collect();
    let missing_get_error = registry.get("missing").unwrap_err().to_string();
    let missing_touch_error = registry.touch("missing").unwrap_err().to_string();
    let empty_ref_error = registry
        .set_ref("alpha", "", "value")
        .unwrap_err()
        .to_string();
    registry.clear();
    let profiles_preserved = root.join("alpha").is_dir() && root.join("beta").is_dir();
    assert!(
        profiles_preserved,
        "registry clear must preserve browser profile directories"
    );
    let result = serde_json::json!({
        "schema_version": data.schema_version,
        "sessions": sessions,
        "invalid_name_error": invalid_name_error,
        "missing_get_error": missing_get_error,
        "missing_touch_error": missing_touch_error,
        "empty_ref_error": empty_ref_error,
        "reference": reference,
        "ensure_idempotent": ensure_idempotent,
        "cleared_entries": registry.list().len(),
        "profiles_preserved": profiles_preserved,
    });
    println!("{result}");
    fs::remove_dir_all(root).expect("remove only this oracle's temporary registry root");
}
