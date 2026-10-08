//! Actual response/task owners, with body timing armed after wire admission.
use super::super::{redirect, response::read_response};
use super::ConnectionTask;
use crate::{FetchClient, Request};
use futures_util::poll;
use reqwest::Method;
use std::{future::pending, time::Duration};
use symbrowse_core::policy::SsrfGuard;
use tokio::{io::AsyncWriteExt, net::TcpStream};

#[path = "../../tests/support/proxy_peer.rs"]
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
async fn raw_response_body_deadline_drops_actual_owner_before_client_teardown() {
    let mut peer = Peer::spawn(Reply::PartialBody).await;
    let client = FetchClient::honest().unwrap();
    let request = request(peer.url.clone());
    let (response, _, _) = redirect::send(&client, &request, &Method::GET, &SsrfGuard::new(true))
        .await
        .unwrap();
    peer.ready(Phase::PartialBodySent).await;
    assert_eq!(response.content_length(), Some(8));
    // This is the production decoder holding the actual raw OwnedBody. The
    // original 30ms budget is armed only after transport/response admission.
    let mut timed = Box::pin(tokio::time::timeout(
        Duration::from_millis(30),
        read_response(response, &request.url, 1024, 1024, false, true),
    ));
    assert!(
        poll!(timed.as_mut()).is_pending(),
        "partial body reader must be pending"
    );
    tokio::time::pause();
    tokio::time::advance(Duration::from_millis(31)).await;
    if std::env::var("FETCH_PROXY_FIXTURE_CONTROL").as_deref() == Ok("retained-body") {
        // Actual negative control: the elapsed timeout still owns its pending
        // inner response. Retain it while applying the same real EOF assertion.
        assert!(timed.as_mut().await.is_err());
        tokio::time::resume();
        peer.assert_closed(Phase::PartialBodySent).await;
        drop(timed);
    } else {
        // Await by value, as honest::fetch does: elapsed completion drops the
        // timed inner body before the close observation, not at scope teardown.
        assert!(timed.await.is_err());
        tokio::time::resume();
        peer.assert_closed(Phase::PartialBodySent).await;
    }
    client.close();
}

#[tokio::test]
async fn raw_connection_task_guard_aborts_owned_socket_before_client_teardown() {
    let mut peer = Peer::spawn(Reply::WithheldHeaders).await;
    let client = FetchClient::honest().unwrap();
    let mut socket = TcpStream::connect(peer.url.strip_prefix("http://").unwrap())
        .await
        .unwrap();
    socket
        .write_all(b"GET http://93.184.216.34:080/body HTTP/1.1\r\nHost: 93.184.216.34:080\r\n\r\n")
        .await
        .unwrap();
    let guard = ConnectionTask(tokio::spawn(async move {
        pending::<()>().await;
        drop(socket);
    }));
    let observation = peer.ready(Phase::HeadersRead).await;
    assert!(observation.header.ends_with(b"\r\n\r\n"));
    if std::env::var("FETCH_PROXY_FIXTURE_CONTROL").as_deref() == Ok("leaked-task") {
        peer.assert_closed(Phase::HeadersRead).await;
        drop(guard);
    } else {
        drop(guard);
        peer.assert_closed(Phase::HeadersRead).await;
    }
    client.close();
}

#[tokio::test]
async fn complete_actual_raw_body_reader_has_no_timeout_error() {
    let mut peer = Peer::spawn(Reply::CompleteBody).await;
    let client = FetchClient::honest().unwrap();
    let request = request(peer.url.clone());
    let (response, _, _) = redirect::send(&client, &request, &Method::GET, &SsrfGuard::new(true))
        .await
        .unwrap();
    peer.ready(Phase::CompleteBodySent).await;
    let body = tokio::time::timeout(
        Duration::from_secs(2),
        read_response(response, &request.url, 1024, 1024, false, true),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(body.body, b"12345678");
    peer.assert_closed(Phase::CompleteBodySent).await;
    client.close();
}
