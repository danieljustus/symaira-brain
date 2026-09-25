use std::{
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde_json::Value;
use symbrowse_engine_firefox::{FirefoxSession, resolve_firefox_executable};

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn create() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "symbrowse-firefox-interaction-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("create owned Firefox test directory");
        Self(path)
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct LoginFixture {
    address: std::net::SocketAddr,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl LoginFixture {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind login fixture loopback");
        listener
            .set_nonblocking(true)
            .expect("make login fixture nonblocking");
        let address = listener.local_addr().expect("login fixture address");
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let worker = thread::spawn(move || {
            while !worker_stop.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut stream, _)) => serve(&mut stream),
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            address,
            stop,
            worker: Some(worker),
        }
    }

    fn url(&self) -> String {
        format!("http://{}/login", self.address)
    }
}

impl Drop for LoginFixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn serve(stream: &mut TcpStream) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let mut request = [0; 4096];
    let _ = stream.read(&mut request);
    let body = br#"<!doctype html><title>Firefox login fixture</title>
<form id="login"><input id="username"><input id="password" type="password"><button id="submit">Sign in</button></form>
<script>
window.proof = {usernameTrusted: false, passwordTrusted: false, clickTrusted: false, submitTrusted: false};
for (const id of ["username", "password"]) document.getElementById(id).addEventListener("input", event => { if (event.isTrusted) window.proof[id + "Trusted"] = true; });
document.getElementById("submit").addEventListener("click", event => { window.proof.clickTrusted = event.isTrusted; });
document.getElementById("login").addEventListener("submit", event => { event.preventDefault(); window.proof.submitTrusted = event.isTrusted; document.title = "Firefox login completed"; });
</script>"#;
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(body);
}

#[tokio::test]
#[ignore = "native target-contract gate; requires Firefox Nightly and SYMBROWSE_E2E=1"]
async fn firefox_login_uses_trusted_bidi_input_actions() {
    assert_eq!(
        std::env::var("SYMBROWSE_E2E").as_deref(),
        Ok("1"),
        "set SYMBROWSE_E2E=1 to run the native Firefox interaction gate"
    );
    let executable = resolve_firefox_executable(
        std::env::var_os("SYMBROWSE_FIREFOX_EXECUTABLE")
            .as_deref()
            .map(PathBuf::from)
            .as_deref(),
    )
    .expect("native Firefox must be installed or explicitly configured");
    let temp = TestDirectory::create();
    let profile = temp.path().join("profile");
    fs::create_dir(&profile).expect("create isolated Firefox profile");
    fs::write(
        profile.join("user.js"),
        b"user_pref(\"remote.experimental.enabled\", true);\n",
    )
    .expect("enable BiDi for the isolated Firefox test profile");
    let fixture = LoginFixture::start();
    let mut session = FirefoxSession::launch(executable, profile, Duration::from_secs(25))
        .await
        .expect("launch Firefox with isolated profile");

    let result = async {
        session.navigate(&fixture.url()).await?;
        session
            .interact("fill", "#username", Some("fixture-user"))
            .await?;
        session
            .interact("fill", "#password", Some("fixture-only-password"))
            .await?;
        session.interact("click", "#submit", None).await?;
        let proof = session
            .evaluate("JSON.stringify(window.proof)")
            .await?
            .value
            .and_then(|value| value.as_str().map(str::to_owned))
            .ok_or_else(|| {
                symbrowse_engine_firefox::FirefoxError::Driver(
                    "Firefox login fixture returned no interaction proof".into(),
                )
            })?;
        let proof: Value = serde_json::from_str(&proof)
            .map_err(|error| symbrowse_engine_firefox::FirefoxError::Driver(error.to_string()))?;
        Ok::<_, symbrowse_engine_firefox::FirefoxError>(proof)
    }
    .await;
    let endpoint = session.remote_endpoint();
    session.close().await.expect("close owned Firefox session");
    wait_for_endpoint_closed(endpoint).await;
    drop(fixture);

    let proof = result.expect("complete local Firefox login interaction");
    assert_eq!(
        proof["usernameTrusted"], true,
        "username input was not browser-generated"
    );
    assert_eq!(
        proof["passwordTrusted"], true,
        "password input was not browser-generated"
    );
    assert_eq!(
        proof["clickTrusted"], true,
        "click was not browser-generated"
    );
    assert_eq!(proof["submitTrusted"], true, "form submit was not trusted");
}

async fn wait_for_endpoint_closed(endpoint: std::net::SocketAddr) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        if TcpStream::connect_timeout(&endpoint, Duration::from_millis(100)).is_err() {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "Firefox BiDi endpoint remained open after session cleanup"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}
