//! FETCH-008 batch contract against a real loopback HTTP server: cardinality
//! bounds, input order, bounded concurrency, partial failures and the
//! JSON-semantic status schema. No Go oracle exists, so Rust behavior is the
//! reference and comparison is JSON-semantic, as declared in the contract.
use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use serde_json::json;
use symbrowse_fetch::{
    Client, FetchClient,
    batch::{self, MAX_URLS},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

#[derive(Default)]
struct Counters {
    hits: AtomicUsize,
    active: AtomicUsize,
    peak: AtomicUsize,
}

struct Server {
    address: SocketAddr,
    counters: Arc<Counters>,
}

impl Server {
    async fn start() -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        let counters = Arc::new(Counters::default());
        let shared = Arc::clone(&counters);
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let counters = Arc::clone(&shared);
                tokio::spawn(async move {
                    let mut input = vec![0_u8; 16 * 1024];
                    let length = stream.read(&mut input).await.unwrap_or(0);
                    let request = String::from_utf8_lossy(&input[..length]).into_owned();
                    let path = request.split_whitespace().nth(1).unwrap_or("/").to_string();
                    counters.hits.fetch_add(1, Ordering::SeqCst);
                    let now = counters.active.fetch_add(1, Ordering::SeqCst) + 1;
                    counters.peak.fetch_max(now, Ordering::SeqCst);
                    // Earlier inputs answer later so completion order differs
                    // from input order and the sort is observable.
                    let index: u64 = path
                        .rsplit('/')
                        .next()
                        .and_then(|tail| tail.parse().ok())
                        .unwrap_or(0);
                    tokio::time::sleep(Duration::from_millis(5 * (25 - index.min(24)))).await;
                    let (status, body) = if path.starts_with("/gone/") {
                        ("404 Not Found", format!("gone {path}"))
                    } else {
                        ("200 OK", format!("body {path}"))
                    };
                    let head = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = stream.write_all(head.as_bytes()).await;
                    let _ = stream.write_all(body.as_bytes()).await;
                    counters.active.fetch_sub(1, Ordering::SeqCst);
                });
            }
        });
        Self { address, counters }
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.address)
    }
}

fn client() -> Arc<dyn Client> {
    Arc::new(FetchClient::honest().unwrap())
}

/// A loopback port that refuses connections: bind, learn the port, drop.
async fn refused_url() -> String {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let url = format!("http://{}/refused/0", listener.local_addr().unwrap());
    drop(listener);
    url
}

#[tokio::test]
async fn twenty_one_urls_are_rejected_before_any_request() {
    let server = Server::start().await;
    let urls: Vec<String> = (0..=MAX_URLS)
        .map(|index| server.url(&format!("/ok/{index}")))
        .collect();
    assert_eq!(urls.len(), 21);
    let error = batch::fetch_urls(client(), urls, 4, true)
        .await
        .err()
        .expect("21 URLs must be a contract error");
    assert_eq!(error, "too many URLs: maximum is 20");
    assert_eq!(server.counters.hits.load(Ordering::SeqCst), 0);

    let empty = batch::fetch_urls(client(), Vec::new(), 4, true)
        .await
        .err()
        .expect("empty batch must be a contract error");
    assert_eq!(empty, "at least one URL is required");
}

#[tokio::test]
async fn one_url_returns_a_single_ordered_item() {
    let server = Server::start().await;
    let results = batch::fetch_urls(client(), vec![server.url("/ok/0")], 4, true)
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].index, 0);
    let response = results[0].result.as_ref().unwrap();
    assert_eq!(response.status_code, 200);
    assert_eq!(response.body, b"body /ok/0");
}

#[tokio::test]
async fn twenty_urls_keep_input_order_bound_concurrency_and_partial_failures() {
    let server = Server::start().await;
    let refused = refused_url().await;
    let failing: [usize; 2] = [6, 13];
    let not_found: [usize; 2] = [3, 19];
    let urls: Vec<String> = (0..MAX_URLS)
        .map(|index| {
            if failing.contains(&index) {
                refused.clone()
            } else if not_found.contains(&index) {
                server.url(&format!("/gone/{index}"))
            } else {
                server.url(&format!("/ok/{index}"))
            }
        })
        .collect();
    assert_eq!(urls.len(), 20);

    let results = batch::fetch_urls(client(), urls.clone(), 4, true)
        .await
        .unwrap();
    assert_eq!(results.len(), 20);
    assert_eq!(
        results.iter().map(|item| item.index).collect::<Vec<_>>(),
        (0..20).collect::<Vec<_>>(),
        "results must follow input order, not completion order"
    );
    let peak = server.counters.peak.load(Ordering::SeqCst);
    assert!(
        (2..=4).contains(&peak),
        "concurrency must be bounded by the limit and actually overlap, peak={peak}"
    );

    for (index, item) in results.iter().enumerate() {
        match (&item.result, index) {
            (Err(_), i) => assert!(failing.contains(&i), "unexpected failure at {i}"),
            (Ok(response), i) if not_found.contains(&i) => {
                assert_eq!(response.status_code, 404);
                assert_eq!(response.body, format!("gone /gone/{i}").into_bytes());
            }
            (Ok(response), i) => {
                assert_eq!(response.status_code, 200);
                assert_eq!(response.body, format!("body /ok/{i}").into_bytes());
            }
        }
    }

    let snapshot = batch::status_snapshot(&results, &urls);
    assert_eq!(snapshot.len(), 20);
    for (index, entry) in snapshot.iter().enumerate() {
        let object = entry.as_object().unwrap();
        assert_eq!(object["index"], json!(index));
        assert_eq!(object["url"], json!(urls[index]));
        if failing.contains(&index) {
            assert_eq!(object["ok"], json!(false));
            assert!(
                object["error"]
                    .as_str()
                    .is_some_and(|text| !text.is_empty())
            );
        } else {
            assert_eq!(object["ok"], json!(true));
            assert!(!object.contains_key("error"));
        }
    }

    let (successes, failures) = batch::partition(results);
    assert_eq!(successes.len(), 18);
    assert_eq!(
        failures.iter().map(|(index, _)| *index).collect::<Vec<_>>(),
        failing.to_vec()
    );
}

#[tokio::test]
async fn zero_concurrency_degrades_to_serial_execution() {
    let server = Server::start().await;
    let urls: Vec<String> = (0..4).map(|i| server.url(&format!("/ok/{i}"))).collect();
    let results = batch::fetch_urls(client(), urls, 0, true).await.unwrap();
    assert_eq!(results.len(), 4);
    assert!(results.iter().all(|item| item.result.is_ok()));
    assert_eq!(server.counters.peak.load(Ordering::SeqCst), 1);
}
