//! CFG-006: the daemon status report identifies the selected transport mode and
//! engine for every explicit selection and never exposes browser profile data.
//! No browser is launched: `daemon.status` is answered by the server itself.
#![cfg(unix)]
#![deny(unsafe_code)]

mod common;

use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::Path,
    sync::Arc,
    thread,
    time::Duration,
};

use serde_json::Value;
use symbrowse_daemon::{Server, ServerOptions, SessionSpec};

fn status(socket: &Path) -> Value {
    let mut stream = UnixStream::connect(socket).expect("connect daemon");
    writeln!(stream, r#"{{"cmd":"daemon.status"}}"#).expect("write frame");
    let mut line = String::new();
    BufReader::new(stream)
        .read_line(&mut line)
        .expect("read response");
    serde_json::from_str(&line).expect("decode response")
}

#[test]
fn daemon_status_reports_each_selected_mode_and_engine_without_profile_data() {
    // The daemon derives its profile path from HOME/XDG: keep it isolated.
    let Some(root) = common::isolated_environment_root(
        "symbrowse-transport-status",
        "daemon_status_reports_each_selected_mode_and_engine_without_profile_data",
    ) else {
        return;
    };
    let selections = [
        ("static", "static"),
        ("browser", "chrome"),
        ("browser", "firefox"),
        ("browser", "safari-bidi"),
    ];
    for (index, (mode, engine)) in selections.into_iter().enumerate() {
        let root = root.join(format!("selection-{index}"));
        fs::create_dir_all(&root).expect("create selection root");
        let mut spec = SessionSpec::for_session("selection-status");
        spec.mode = mode.into();
        spec.engine = engine.into();
        spec.state_dir = root.join("state");
        spec.cache_dir = root.join("cache");
        spec.socket_path =
            std::path::PathBuf::from(format!("/tmp/sbts-{}-{index}.sock", std::process::id()));
        spec.idle_timeout = Some(Duration::from_secs(30));
        let profile = spec.user_data_dir();
        let server = Arc::new(
            Server::new(ServerOptions {
                session_spec: Some(spec.clone()),
                state_key_sources: Some(common::isolated_key_sources()),
                ..ServerOptions::default()
            })
            .expect("create daemon"),
        );
        let socket = spec.socket_path.clone();
        let serving = Arc::clone(&server);
        let thread = thread::spawn(move || serving.listen_and_serve().expect("serve daemon"));
        for _ in 0..200 {
            if socket.exists() {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        assert!(socket.exists(), "daemon socket did not appear");

        let response = status(&socket);
        assert_eq!(response["success"], true, "{response}");
        assert_eq!(response["data"]["mode"], mode, "{response}");
        assert_eq!(response["data"]["engine"], engine, "{response}");
        let rendered = response.to_string();
        for forbidden in [
            "user_data_dir",
            "browser_context_id",
            "profile",
            &*profile.to_string_lossy(),
        ] {
            assert!(
                !rendered.contains(forbidden),
                "status leaks {forbidden:?}: {rendered}"
            );
        }
        assert!(
            !profile.exists()
                || fs::read_dir(&profile).is_ok_and(|mut entries| entries.next().is_none()),
            "reporting status must not launch a browser or populate its profile"
        );
        server.stop();
        let _ = thread.join();
        let _ = fs::remove_dir_all(root);
    }
}
