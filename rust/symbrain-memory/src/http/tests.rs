//! In-process loopback checks of the owner's ordering, auth and Store effects.
//! The Go-paired wire proof stays in `scripts/memory-http-oracle`.

use super::{Options, Server};
use crate::{EmbeddingGenerator, Store};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::Arc,
};

const SECRET: &[u8] = b"owned-test-secret";

struct Owner {
    port: u16,
    store: Arc<Store>,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Owner {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            thread.join().expect("owner thread");
        }
    }
}

fn owner(options: Options) -> Owner {
    let store = Arc::new(Store::open_in_memory().expect("store"));
    // A refused loopback endpoint selects the deterministic hash embedding.
    let generator = EmbeddingGenerator::new("http://127.0.0.1:1", "test-model");
    let server = Arc::new(
        Server::new(Arc::clone(&store), SECRET.to_vec(), generator, options).expect("server"),
    );
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let thread = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("runtime");
        runtime
            .block_on(server.serve_until(listener, async {
                let _ = stopped.await;
            }))
            .expect("serve");
    });
    Owner {
        port,
        store,
        stop: Some(stop),
        thread: Some(thread),
    }
}

fn writer() -> Options {
    Options {
        direct_writes: true,
        ..Options::default()
    }
}

fn token(sub: &str, jti: &str, exp_offset: i64) -> String {
    let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"HS256","typ":"JWT"}"#);
    let claims = serde_json::json!({
        "iss": "symaira-memory",
        "sub": sub,
        "jti": jti,
        "exp": chrono::Utc::now().timestamp() + exp_offset,
    });
    let payload = URL_SAFE_NO_PAD.encode(claims.to_string());
    let mut mac = Hmac::<Sha256>::new_from_slice(SECRET).expect("mac");
    mac.update(format!("{header}.{payload}").as_bytes());
    let signature = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
    format!("{header}.{payload}.{signature}")
}

struct Reply {
    status: u16,
    head: String,
    body: String,
}

impl Reply {
    fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.body).expect("JSON body")
    }
}

/// Sends one literal request and reads the reply until the owner closes.
fn raw(port: u16, request: &str) -> Reply {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    // An early rejection may close before the whole body is written.
    let _ = stream.write_all(request.as_bytes());
    let mut response = Vec::new();
    stream.read_to_end(&mut response).expect("read");
    let response = String::from_utf8(response).expect("utf8");
    let (head, body) = response.split_once("\r\n\r\n").expect("head");
    Reply {
        status: head[9..12].parse().expect("status"),
        head: head.to_ascii_lowercase(),
        body: if head
            .to_ascii_lowercase()
            .contains("transfer-encoding: chunked")
        {
            dechunk(body)
        } else {
            body.to_owned()
        },
    }
}

fn dechunk(mut rest: &str) -> String {
    let mut body = String::new();
    while let Some((size, tail)) = rest.split_once("\r\n") {
        let size = usize::from_str_radix(size, 16).expect("chunk size");
        if size == 0 {
            break;
        }
        body.push_str(&tail[..size]);
        rest = &tail[size + 2..];
    }
    body
}

fn send(port: u16, method: &str, path: &str, headers: &[(&str, &str)], body: &str) -> Reply {
    let mut request = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\nContent-Length: {}\r\n",
        body.len()
    );
    for (name, value) in headers {
        request.push_str(&[name, ": ", value, "\r\n"].concat());
    }
    request.push_str("\r\n");
    request.push_str(body);
    raw(port, &request)
}

fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

#[test]
fn public_status_assets_and_security_headers() {
    let owner = owner(Options {
        version: "1.2.3".into(),
        ..Options::default()
    });
    let status = send(owner.port, "GET", "/api/status", &[], "");
    assert_eq!(status.status, 200);
    assert_eq!(status.json()["version"], "1.2.3");
    assert!(status.head.contains("x-frame-options: deny"));
    assert!(status.head.contains("x-content-type-options: nosniff"));

    let index = send(owner.port, "GET", "/", &[], "");
    assert_eq!(index.status, 200);
    assert!(index.head.contains("text/html"));
    assert!(!index.head.contains("accept-ranges"));
    let css = send(owner.port, "GET", "/style.css", &[], "");
    assert!(css.head.contains("accept-ranges: bytes"));
    assert_eq!(
        css.body.as_bytes(),
        include_bytes!("../../assets/web/style.css")
    );

    let head = send(owner.port, "HEAD", "/app.js", &[], "");
    assert_eq!(head.status, 200);
    assert!(head.body.is_empty());
    let length = include_bytes!("../../assets/web/app.js").len();
    assert!(head.head.contains(&format!("content-length: {length}")));

    assert_eq!(send(owner.port, "GET", "/missing", &[], "").status, 404);
    let post = send(
        owner.port,
        "POST",
        "/style.css",
        &[("X-Requested-With", "XMLHttpRequest")],
        "",
    );
    assert_eq!(post.status, 405);
    assert!(post.head.contains("allow: get, head"));
}

#[test]
fn host_csrf_and_origin_reject_before_authentication() {
    let owner = owner(writer());
    let port = owner.port;
    let duplicate = raw(
        port,
        &format!(
            "GET /api/list HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
        ),
    );
    assert_eq!(duplicate.status, 400);
    let missing = raw(
        port,
        "GET /api/status HTTP/1.1\r\nConnection: close\r\n\r\n",
    );
    assert_eq!(missing.status, 400);
    assert!(missing.body.contains("missing required Host header"));

    let foreign = raw(
        port,
        "GET /api/status HTTP/1.1\r\nHost: example.com\r\nConnection: close\r\n\r\n",
    );
    assert_eq!(foreign.status, 403);
    assert_eq!(foreign.json()["error"], "non-loopback Host header rejected");
    // Absolute request-target authority wins over a loopback Host header.
    let absolute = raw(
        port,
        &format!(
            "GET http://example.com/api/status HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
        ),
    );
    assert_eq!(absolute.status, 403);

    let csrf = send(port, "POST", "/api/set", &[], "{}");
    assert_eq!(csrf.status, 403);
    assert_eq!(csrf.json()["error"], "CSRF validation failed");
    let origin = send(
        port,
        "GET",
        "/api/list",
        &[("Origin", "http://evil.test")],
        "",
    );
    assert_eq!(origin.json()["error"], "origin not allowed");

    let extension = "chrome-extension://abc";
    let preflight = send(port, "OPTIONS", "/api/list", &[("Origin", extension)], "");
    assert_eq!(preflight.status, 200);
    assert!(
        preflight
            .head
            .contains(&format!("access-control-allow-origin: {extension}"))
    );
    // The exact bound listener origin is admitted; another port is not.
    let local = format!("http://127.0.0.1:{port}");
    assert_eq!(
        send(port, "OPTIONS", "/api/list", &[("Origin", &local)], "").status,
        200
    );
    let other = format!("http://127.0.0.1:{}", port.wrapping_add(1));
    assert_eq!(
        send(port, "OPTIONS", "/api/list", &[("Origin", &other)], "").status,
        403
    );
    assert_eq!(owner.store.list_lite("", 10).expect("list").len(), 0);
}

#[test]
fn bearer_tokens_are_verified_and_revocable() {
    let owner = owner(Options::default());
    let port = owner.port;
    assert_eq!(send(port, "GET", "/api/list", &[], "").status, 401);
    let forged = format!("{}x", token("alice", "j1", 60));
    let reply = send(
        port,
        "GET",
        "/api/list",
        &[("Authorization", &bearer(&forged))],
        "",
    );
    assert_eq!(reply.json()["error"], "invalid or expired token");
    let expired = token("alice", "j0", -60);
    assert_eq!(
        send(
            port,
            "GET",
            "/api/list",
            &[("Authorization", &bearer(&expired))],
            ""
        )
        .status,
        401
    );

    let valid = bearer(&token("alice", "j1", 60));
    let auth = [("Authorization", valid.as_str())];
    let list = send(port, "GET", "/api/list", &auth, "");
    assert_eq!((list.status, list.body.as_str()), (200, "null\n"));
    assert_eq!(
        send(port, "GET", "/api/rules", &auth, "").json(),
        serde_json::json!({"rules":null})
    );
    assert_eq!(
        send(port, "GET", "/api/entities", &auth, "").json(),
        serde_json::json!({"entities":null})
    );
    assert_eq!(send(port, "GET", "/api/search", &auth, "").status, 405);
    assert_eq!(send(port, "GET", "/api/stats", &auth, "").status, 501);
    assert_eq!(send(port, "GET", "/api/get", &auth, "").status, 400);

    assert_eq!(
        send(port, "POST", "/api/token/revoke", &auth, "{").status,
        400
    );
    assert_eq!(
        send(port, "POST", "/api/token/revoke", &auth, "{}").status,
        400
    );
    let other = token("bob", "j2", 60);
    let by_token = send(
        port,
        "POST",
        "/api/token/revoke",
        &auth,
        &format!(r#"{{"token":"{other}"}}"#),
    );
    assert_eq!(
        by_token.json(),
        serde_json::json!({"status":"revoked","jti":"j2"})
    );
    assert_eq!(
        send(
            port,
            "GET",
            "/api/list",
            &[("Authorization", &bearer(&other))],
            ""
        )
        .status,
        401
    );
    let revoked: i64 = owner
        .store
        .lock()
        .expect("lock")
        .query_row(
            "SELECT COUNT(*) FROM jwt_revocations WHERE jti='j2'",
            [],
            |row| row.get(0),
        )
        .expect("count");
    assert_eq!(revoked, 1);
}

#[test]
fn direct_writes_round_trip_through_the_shared_store() {
    let owner = owner(writer());
    let port = owner.port;
    let valid = bearer(&token("alice", "w1", 60));
    let auth = [("Authorization", valid.as_str())];
    let content = "blue widgets ship on tuesdays";
    let set = send(
        port,
        "POST",
        "/api/set",
        &auth,
        &format!(r#"{{"content":"{content}","scope":"global"}}"#),
    );
    assert_eq!(set.status, 200, "{}", set.body);
    let id = set.json()["id"].as_str().expect("id").to_owned();
    let (source_tool, author, audits): (String, String, i64) = owner
        .store
        .lock()
        .expect("lock")
        .query_row(
            "SELECT json_extract(metadata,'$.source_tool'),created_by,(SELECT COUNT(*) FROM audit_log WHERE memory_id=memories.id) FROM memories WHERE id=?",
            [&id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("row");
    assert_eq!(
        (source_tool.as_str(), author.as_str(), audits),
        ("http", "alice", 1)
    );

    let get = send(port, "GET", &format!("/api/get?id={id}"), &auth, "");
    assert_eq!(get.json()["content"], content);
    let list = send(port, "GET", "/api/list?scope=global", &auth, "");
    assert_eq!(list.json()[0]["id"], id.as_str());
    let search = send(
        port,
        "POST",
        "/api/search",
        &auth,
        &format!(r#"{{"query":"{content}"}}"#),
    );
    assert_eq!(search.json()[0]["memory"]["id"], id.as_str());
    assert_eq!(
        send(
            port,
            "POST",
            "/api/search",
            &auth,
            r#"{"query":"x","filter":"y"}"#
        )
        .status,
        501
    );
    assert_eq!(
        send(
            port,
            "POST",
            "/api/set",
            &auth,
            r#"{"content":"a","working":true}"#
        )
        .status,
        501
    );

    let delete = send(port, "DELETE", &format!("/api/delete?id={id}"), &auth, "");
    assert_eq!(delete.json(), serde_json::json!({"deleted":true}));
    assert_eq!(
        send(port, "GET", &format!("/api/get?id={id}"), &auth, "").status,
        404
    );
    assert_eq!(
        send(port, "DELETE", "/api/delete?id=missing", &auth, "").status,
        404
    );
}

#[test]
fn writes_respect_options_profiles_and_body_limits() {
    let disabled = owner(Options::default());
    let valid = bearer(&token("alice", "p1", 60));
    let auth = [("Authorization", valid.as_str())];
    assert_eq!(
        send(
            disabled.port,
            "POST",
            "/api/set",
            &auth,
            r#"{"content":"plain words"}"#
        )
        .status,
        501
    );
    assert_eq!(
        send(disabled.port, "DELETE", "/api/delete?id=x", &auth, "").status,
        501
    );
    drop(disabled);

    let strict = owner(Options {
        require_profile: true,
        ..writer()
    });
    let denied = send(strict.port, "POST", "/api/set", &auth, "{}");
    assert_eq!(denied.status, 403);
    assert!(denied.body.contains("no profile registered"));
    strict
        .store
        .lock()
        .expect("lock")
        .execute(
            "INSERT INTO profiles(id,name,role,created_at,updated_at) VALUES('p','alice','readonly',datetime(),datetime())",
            [],
        )
        .expect("profile");
    let readonly = send(strict.port, "POST", "/api/set", &auth, "{}");
    assert!(readonly.body.contains("read-only profile"));

    let open = owner(writer());
    let oversized = "x".repeat((1 << 20) + 1);
    assert_eq!(
        send(open.port, "POST", "/api/set", &auth, &oversized).status,
        413
    );
    assert_eq!(send(open.port, "POST", "/api/set", &auth, "{").status, 400);
}

#[test]
fn listener_must_be_loopback_and_secret_nonempty() {
    let store = Arc::new(Store::open_in_memory().expect("store"));
    let generator = EmbeddingGenerator::new("http://127.0.0.1:1", "test-model");
    assert!(
        Server::new(
            Arc::clone(&store),
            Vec::new(),
            EmbeddingGenerator::new("http://127.0.0.1:1", "test-model"),
            Options::default()
        )
        .is_err()
    );
    let server = Arc::new(
        Server::new(store, SECRET.to_vec(), generator, Options::default()).expect("server"),
    );
    let listener = TcpListener::bind("0.0.0.0:0").expect("bind");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let error = runtime
        .block_on(server.serve_until(listener, std::future::pending()))
        .expect_err("non-loopback");
    assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
}
