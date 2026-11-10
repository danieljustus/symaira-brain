//! FETCH-008 Wayback and 404/410 recovery failure branches over loopback HTTP.
//! The comparison is JSON-semantic (status, final URL, body); the Go oracle is
//! gone, so Rust behavior is the reference.
use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
};

use symbrowse_fetch::{
    FetchClient, Request,
    archive::{self, CdxClient, CdxQuery, WaybackClient},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

struct Server {
    address: SocketAddr,
    paths: Arc<Mutex<Vec<String>>>,
}

impl Server {
    async fn start() -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        let paths = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&paths);
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let seen = Arc::clone(&seen);
                tokio::spawn(async move {
                    let mut input = vec![0_u8; 16 * 1024];
                    let length = stream.read(&mut input).await.unwrap_or(0);
                    let request = String::from_utf8_lossy(&input[..length]).into_owned();
                    let path = request.split_whitespace().nth(1).unwrap_or("/").to_string();
                    seen.lock().unwrap().push(path.clone());
                    let (status, body): (&str, &[u8]) = if path.starts_with("/cdx/bad") {
                        ("200 OK", b"not json")
                    } else if path.starts_with("/cdx/empty") {
                        ("200 OK", br#"[["timestamp","original"]]"#)
                    } else if path.starts_with("/cdx/short") {
                        (
                            "200 OK",
                            br#"[["timestamp","original","mimetype","statuscode","digest","length"],["2026","only-two-columns"],["20260101120000","https://e.test/a","text/html","200","d","7"]]"#,
                        )
                    } else if path.starts_with("/cdx/down") {
                        ("503 Service Unavailable", b"down")
                    } else if path.contains("/web/") && path.contains("/dead") {
                        ("404 Not Found", b"no snapshot")
                    } else if path.starts_with("/web/") {
                        ("200 OK", b"archived body")
                    } else if path.starts_with("/site/gone404") {
                        ("404 Not Found", b"missing")
                    } else if path.starts_with("/site/gone410") {
                        ("410 Gone", b"gone")
                    } else if path.starts_with("/site/error") {
                        ("500 Internal Server Error", b"boom")
                    } else {
                        ("200 OK", b"live body")
                    };
                    let head = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = stream.write_all(head.as_bytes()).await;
                    let _ = stream.write_all(body).await;
                });
            }
        });
        Self { address, paths }
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.address)
    }

    fn requests(&self) -> Vec<String> {
        self.paths.lock().unwrap().clone()
    }
}

fn request(url: String) -> Request {
    let mut request = Request::get(url);
    request.allow_private = true;
    request
}

#[tokio::test]
async fn live_and_non_recovery_statuses_never_touch_the_archive() {
    let site = Server::start().await;
    let archive_server = Server::start().await;
    let archive_client = WaybackClient::new(archive_server.url("/web")).unwrap();
    let client = FetchClient::honest().unwrap();
    for (path, status, body) in [
        ("/site/live", 200, "live body"),
        ("/site/error", 500, "boom"),
    ] {
        let response = archive::fetch_with_wayback(
            &client,
            request(site.url(path)),
            &archive_client,
            Some("20260101120000"),
        )
        .await
        .unwrap();
        assert_eq!(response.status_code, status, "{path}");
        assert_eq!(response.body, body.as_bytes());
        assert_eq!(response.final_url, site.url(path));
    }
    assert!(archive_server.requests().is_empty());
}

#[tokio::test]
async fn both_not_found_statuses_are_replaced_by_the_snapshot() {
    let site = Server::start().await;
    let archive_server = Server::start().await;
    let archive_client = WaybackClient::new(archive_server.url("/web")).unwrap();
    let client = FetchClient::honest().unwrap();
    for path in ["/site/gone404", "/site/gone410"] {
        let original = site.url(path);
        let response = archive::fetch_with_wayback(
            &client,
            request(original.clone()),
            &archive_client,
            Some("20260101120000"),
        )
        .await
        .unwrap();
        assert_eq!(response.status_code, 200, "{path}");
        assert_eq!(response.body, b"archived body");
        assert_eq!(
            response.final_url,
            archive_client.archive_url(&original, Some("20260101120000"))
        );
    }
    assert_eq!(archive_server.requests().len(), 2);
}

#[tokio::test]
async fn failed_or_unreachable_archive_keeps_the_original_not_found_response() {
    let site = Server::start().await;
    let archive_server = Server::start().await;
    let client = FetchClient::honest().unwrap();

    // The archive answers 404 for this original: recovery is abandoned.
    let dead = WaybackClient::new(archive_server.url("/web")).unwrap();
    let mut dead_request = request(site.url("/site/gone404"));
    dead_request.url.push_str("/dead");
    let response = archive::fetch_with_wayback(&client, dead_request, &dead, None)
        .await
        .unwrap();
    assert_eq!(response.status_code, 404);
    assert_eq!(response.body, b"missing");

    // The archive host refuses connections: the original is still returned.
    let refused = {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let url = format!("http://{}/web", listener.local_addr().unwrap());
        drop(listener);
        url
    };
    let unreachable = WaybackClient::new(refused).unwrap();
    let response = archive::fetch_with_wayback(
        &client,
        request(site.url("/site/gone410")),
        &unreachable,
        None,
    )
    .await
    .unwrap();
    assert_eq!(response.status_code, 410);
    assert_eq!(response.body, b"gone");
}

#[tokio::test]
async fn wildcard_timestamp_is_used_when_none_is_requested() {
    let site = Server::start().await;
    let archive_server = Server::start().await;
    let archive_client = WaybackClient::new(archive_server.url("/web")).unwrap();
    let client = FetchClient::honest().unwrap();
    let original = site.url("/site/gone404");
    archive::fetch_with_wayback(&client, request(original.clone()), &archive_client, None)
        .await
        .unwrap();
    assert_eq!(
        archive_server.requests(),
        vec![format!("/web/*/{original}")]
    );
}

#[tokio::test]
async fn cdx_lookup_handles_empty_short_malformed_and_failing_responses() {
    let server = Server::start().await;
    let query = CdxQuery::new("https://e.test/a");

    let empty = CdxClient::new(server.url("/cdx/empty")).unwrap();
    assert!(
        empty
            .lookup_with_policy(&query, true, None)
            .await
            .unwrap()
            .is_empty()
    );

    let short = CdxClient::new(server.url("/cdx/short")).unwrap();
    let rows = short.lookup_with_policy(&query, true, None).await.unwrap();
    assert_eq!(
        rows.len(),
        1,
        "rows with fewer than six columns are dropped"
    );
    assert_eq!(rows[0].original, "https://e.test/a");
    assert_eq!(rows[0].length, "7");

    let bad = CdxClient::new(server.url("/cdx/bad")).unwrap();
    assert!(bad.lookup_with_policy(&query, true, None).await.is_err());

    let down = CdxClient::new(server.url("/cdx/down")).unwrap();
    assert!(down.lookup_with_policy(&query, true, None).await.is_err());
}

#[test]
fn cdx_parser_returns_no_rows_for_header_only_and_rejects_non_json() {
    assert_eq!(
        archive::parse_cdx_response(r#"[["timestamp","original"]]"#),
        Ok(Vec::new())
    );
    assert_eq!(archive::parse_cdx_response("[]"), Ok(Vec::new()));
    let error = archive::parse_cdx_response("not json").unwrap_err();
    assert!(error.starts_with("decode cdx response: "), "{error}");
}
