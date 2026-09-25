use std::{fs, net::TcpListener, process::Command};

use sha2::{Digest, Sha256};

#[test]
fn native_upgrade_check_reads_fixture_cache_without_applying_or_networking() {
    let cache_root = tempfile::tempdir().expect("isolated XDG cache");
    let mut hash = Sha256::new();
    hash.update(b"danieljustus\0symaira-browse");
    let digest = hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let cache = cache_root
        .path()
        .join("symaira")
        .join("updatecheck")
        .join(format!("{digest}.json"));
    fs::create_dir_all(cache.parent().expect("cache parent")).expect("create isolated cache");
    fs::write(&cache, include_str!("fixtures/update-cache.json"))
        .expect("seed Go-compatible local cache fixture");
    let original_cache = fs::read(&cache).expect("read fixture cache");
    let proxy_listener = TcpListener::bind("127.0.0.1:0").expect("reserve local proxy port");
    let blocked_proxy = format!("http://{}", proxy_listener.local_addr().unwrap());
    drop(proxy_listener);

    let output = Command::new(env!("CARGO_BIN_EXE_symbrowse"))
        .args(["upgrade", "--check", "--json"])
        .env("XDG_CACHE_HOME", cache_root.path())
        .env("HTTPS_PROXY", &blocked_proxy)
        .env("https_proxy", &blocked_proxy)
        .env("ALL_PROXY", &blocked_proxy)
        .env("all_proxy", &blocked_proxy)
        .env("NO_PROXY", "")
        .env("no_proxy", "")
        .output()
        .expect("run native CLI executable");

    assert!(
        output.status.success(),
        "upgrade --check failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("JSON envelope");
    assert_eq!(envelope["success"], true);
    assert_eq!(envelope["data"]["up_to_date"], false);
    assert_eq!(envelope["data"]["current"], "0.0.0");
    assert_eq!(envelope["data"]["latest"], "v0.0.1");
    assert_eq!(
        envelope["data"]["hint"],
        "symbrowse v0.0.1 is available (current: 0.0.0); this build can check for updates but cannot apply them"
    );
    assert_eq!(
        fs::read(&cache).expect("check leaves cache intact"),
        original_cache
    );
}
