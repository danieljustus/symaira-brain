//! Actual wire admission precedes deterministic deadlines and OS close proofs.
use std::time::Duration;
use symbrowse_fetch::{FetchClient, FetchError, Request};
use tokio::{io::AsyncWriteExt, net::TcpStream};

#[path = "support/proxy_peer.rs"]
mod proxy_peer;
use proxy_peer::{Peer, Phase, Reply};

fn request(proxy: String) -> Request {
    Request {
        proxy: Some(proxy),
        allow_private: true,
        ..Request::get("http://93.184.216.34:080/body")
    }
}

#[tokio::test]
async fn raw_proxy_deadline_closes_connection_while_client_remains_alive() {
    let mut peer = Peer::spawn(Reply::WithheldHeaders).await;
    let client = FetchClient::honest().unwrap();
    let mut request = request(peer.url.clone());
    // Setup is outside virtual advancement; the transport timer is already
    // armed when the owned peer admits a full header and withholds its reply.
    request.timeout = Some(Duration::from_secs(30));
    let worker = client.clone();
    let task = tokio::spawn(async move { worker.fetch(request).await });
    peer.ready(Phase::HeadersRead).await;
    assert!(
        !task.is_finished(),
        "fetch ended before its controlled deadline"
    );
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(1)).await;
    for _ in 0..16 {
        tokio::task::yield_now().await;
    }
    assert!(
        !task.is_finished(),
        "fetch timed out before its controlled deadline"
    );
    let advance = if std::env::var("FETCH_PROXY_FIXTURE_CONTROL").as_deref() == Ok("timeout") {
        Duration::from_millis(1)
    } else {
        Duration::from_secs(30)
    };
    tokio::time::advance(advance).await;
    for _ in 0..16 {
        tokio::task::yield_now().await;
    }
    assert!(
        task.is_finished(),
        "fetch did not finish at the controlled deadline"
    );
    assert!(matches!(task.await.unwrap(), Err(FetchError::Timeout)));
    tokio::time::resume();
    peer.assert_closed(Phase::HeadersRead).await;
    client.close();
}

#[tokio::test]
async fn raw_proxy_body_limit_closes_connection_while_client_remains_alive() {
    let mut peer = Peer::spawn(Reply::PartialBody).await;
    let client = FetchClient::honest().unwrap();
    let mut request = request(peer.url.clone());
    request.max_compressed_body = Some(1);
    assert!(matches!(
        client.fetch(request).await,
        Err(FetchError::BodyTooLarge(_))
    ));
    peer.ready(Phase::PartialBodySent).await;
    peer.assert_closed(Phase::PartialBodySent).await;
    client.close();
}

#[tokio::test]
async fn cancelling_raw_proxy_fetch_closes_its_connection() {
    let mut peer = Peer::spawn(Reply::PartialBody).await;
    let client = FetchClient::honest().unwrap();
    let worker = client.clone();
    let proxy = peer.url.clone();
    let task = tokio::spawn(async move { worker.fetch(request(proxy)).await });
    peer.ready(Phase::PartialBodySent).await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    peer.assert_closed(Phase::PartialBodySent).await;
    client.close();
}

#[tokio::test]
async fn complete_raw_proxy_response_succeeds_before_owned_peer_closes() {
    let mut peer = Peer::spawn(Reply::CompleteBody).await;
    let client = FetchClient::honest().unwrap();
    let response = client.fetch(request(peer.url.clone())).await.unwrap();
    assert_eq!(response.body, b"12345678");
    assert_eq!(response.status_code, 200);
    peer.ready(Phase::CompleteBodySent).await;
    peer.assert_closed(Phase::CompleteBodySent).await;
    client.close();
}

#[tokio::test]
async fn fixture_records_original_early_header_eof_without_dropping_its_sender() {
    let mut peer = Peer::spawn(Reply::WithheldHeaders).await;
    let mut socket = TcpStream::connect(peer.url.strip_prefix("http://").unwrap())
        .await
        .unwrap();
    socket.write_all(b"GET incomplete").await.unwrap();
    drop(socket);
    let observation = peer.ready(Phase::ClosedDuringHeaders).await;
    assert_eq!(observation.header, b"GET incomplete");
    peer.assert_closed(Phase::ClosedDuringHeaders).await;
}
