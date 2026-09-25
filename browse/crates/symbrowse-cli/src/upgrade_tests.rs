use super::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
};

#[test]
fn stable_versions_and_major_zero_guard_match_go() {
    assert_eq!(parse_stable_version("v1.2.3"), Some(StableVersion(1, 2, 3)));
    assert_eq!(parse_stable_version("1.2.3-rc.1"), None);
    assert_eq!(parse_stable_version("1.2"), None);
    let stable = Release {
        tag_name: "v1.0.0".into(),
        body: String::new(),
        html_url: String::new(),
        assets: Vec::new(),
    };
    assert!(!release_is_newer(&stable, StableVersion(0, 9, 9)));
}

#[test]
fn update_hint_does_not_recommend_unimplemented_apply() {
    let hint = upgrade_hint("v0.8.0", "v0.9.0");
    assert!(hint.contains("v0.9.0 is available (current: v0.8.0)"));
    assert!(hint.contains("cannot apply them"));
    assert!(!hint.contains("run `symbrowse upgrade`"));
}

#[test]
fn local_fake_release_is_fetched_then_reused_from_go_compatible_cache() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}/releases/latest", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0; 2048];
        let _ = stream.read(&mut request).unwrap();
        assert!(
            String::from_utf8_lossy(&request)
                .to_ascii_lowercase()
                .contains("accept: application/vnd.github+json")
        );
        let body = include_str!("../tests/fixtures/latest-release.json");
        write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
    });
    let directory = tempfile::tempdir().unwrap();
    let cache = directory.path().join("updatecheck.json");
    let first = check_release("v0.0.0", &endpoint, &cache).unwrap().unwrap();
    assert_eq!(first.tag_name, "v0.0.1");
    server.join().unwrap();
    let second = check_release("v0.0.0", &endpoint, &cache).unwrap().unwrap();
    assert_eq!(second.tag_name, "v0.0.1");
}
