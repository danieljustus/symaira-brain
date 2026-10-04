//! Prepared real Windows checks for the inherited shared lossy endpoint relation.
use super::*;
use std::{
    ffi::OsString,
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::PathBuf,
};

// The public handler ABI returns DaemonError by value, as in platform.rs.
#[allow(clippy::result_large_err)]
pub(super) fn run(base: &Path, session: &str, scenario: &str) {
    let mut units = base.as_os_str().encode_wide().collect::<Vec<_>>();
    match scenario {
        "endpoint-ascii" => units.push(b'A' as u16),
        "endpoint-unicode" => units.extend([0x00e9, 0xd83d, 0xde00]),
        "endpoint-lossy" => units.push(0xd800),
        _ => panic!("unexpected endpoint relation scenario"),
    }
    let original = PathBuf::from(OsString::from_wide(&units));
    let spelled = PathBuf::from(original.to_string_lossy().as_ref());
    let lossy_units = spelled.as_os_str().encode_wide().collect::<Vec<_>>();
    assert_eq!(original == spelled, scenario != "endpoint-lossy");
    if scenario == "endpoint-lossy" {
        assert_eq!(lossy_units.last(), Some(&0xfffd));
        assert_eq!(units.last(), Some(&0xd800));
    }
    eprintln!(
        "phase=endpoint-pair scenario={scenario} original_utf16={units:?} shared_utf16={lossy_units:?}"
    );
    let (dispatch, dispatched) = mpsc::channel();
    let marker = format!("owned-endpoint-{scenario}");
    let expected_marker = marker.clone();
    let server = Arc::new(
        symbrowse_daemon::Server::new(symbrowse_daemon::ServerOptions {
            socket_path: original.clone(),
            session: session.into(),
            idle_timeout: None,
            handler: Some(Arc::new(move |frame, _| {
                dispatch.send(frame.request_id.clone()).unwrap();
                Ok((
                    Some(serde_json::json!({"owner": marker, "request": frame.request_id})),
                    Vec::new(),
                ))
            })),
            ..Default::default()
        })
        .unwrap(),
    );
    let owner = server.clone();
    let serving = thread::spawn(move || owner.listen_and_serve());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let entered = runtime.enter();
        let opened = PipeClientOptions::new().open(&spelled);
        drop(entered);
        if let Ok(stream) = opened {
            drop(stream);
            break;
        }
        assert!(
            !serving.is_finished() && Instant::now() < deadline,
            "shared endpoint not ready"
        );
        thread::sleep(Duration::from_millis(10));
    }
    // Both configured paths must reach exactly this owner. The original OsStr
    // client-only mutant selects a different path in the lossy pair and fails.
    for (label, path) in [("configured", &original), ("shared-spelling", &spelled)] {
        let id = format!("{scenario}-{label}");
        let response = Client::new(options(path, session, Duration::from_secs(2)))
            .request_without_autostart(Frame {
                cmd: "owned.endpoint".into(),
                request_id: id.clone(),
                ..Frame::default()
            })
            .unwrap();
        assert!(response.success);
        assert_eq!(
            response.data,
            Some(serde_json::json!({"owner": expected_marker, "request": id}))
        );
        assert_eq!(dispatched.recv_timeout(Duration::from_secs(2)).unwrap(), id);
    }
    assert!(
        dispatched.try_recv().is_err(),
        "no unrelated dispatch admitted"
    );
    server.stop(); // Wake must use the same spelling as listen and both clients.
    assert!(serving.join().unwrap().is_ok());
    drop(runtime);
    eprintln!("phase=endpoint-pair-complete scenario={scenario} dispatches=2 stop_joined=true");
}
