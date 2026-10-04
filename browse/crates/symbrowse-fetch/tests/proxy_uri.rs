//! Raw-URI connection ownership is observable before client/process teardown.
use std::time::Duration;
use symbrowse_fetch::{FetchClient, FetchError, Request};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::oneshot,
};

async fn stalled_proxy() -> (String, oneshot::Receiver<()>, oneshot::Receiver<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (started, start) = oneshot::channel();
    let (closed, close) = oneshot::channel();
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            let mut byte = [0];
            socket.read_exact(&mut byte).await.unwrap();
            request.push(byte[0]);
            assert!(request.len() < 8192);
        }
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\n\r\n1")
            .await
            .unwrap();
        let _ = started.send(());
        let mut byte = [0];
        let read = tokio::time::timeout(Duration::from_secs(1), socket.read(&mut byte))
            .await
            .unwrap();
        assert!(
            matches!(read, Ok(0)) || read.is_err(),
            "owned proxy remains connected"
        );
        let _ = closed.send(());
    });
    (format!("http://{address}"), start, close)
}

fn request(proxy: String) -> Request {
    Request {
        proxy: Some(proxy),
        allow_private: true,
        ..Request::get("http://93.184.216.34:080/body")
    }
}

#[tokio::test]
async fn raw_proxy_deadline_closes_connection_while_client_remains_alive() {
    let (proxy, _, closed) = stalled_proxy().await;
    let client = FetchClient::honest().unwrap();
    let mut request = request(proxy);
    request.timeout = Some(Duration::from_millis(30));
    assert!(matches!(
        client.fetch(request).await,
        Err(FetchError::Timeout)
    ));
    tokio::time::timeout(Duration::from_secs(1), closed)
        .await
        .unwrap()
        .unwrap();
    client.close();
}

#[tokio::test]
async fn raw_proxy_body_limit_closes_connection_while_client_remains_alive() {
    let (proxy, _, closed) = stalled_proxy().await;
    let client = FetchClient::honest().unwrap();
    let mut request = request(proxy);
    request.max_compressed_body = Some(1);
    assert!(matches!(
        client.fetch(request).await,
        Err(FetchError::BodyTooLarge(_))
    ));
    tokio::time::timeout(Duration::from_secs(1), closed)
        .await
        .unwrap()
        .unwrap();
    client.close();
}

#[tokio::test]
async fn cancelling_raw_proxy_fetch_closes_its_connection() {
    let (proxy, started, closed) = stalled_proxy().await;
    let client = FetchClient::honest().unwrap();
    let worker = client.clone();
    let task = tokio::spawn(async move { worker.fetch(request(proxy)).await });
    started.await.unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    tokio::time::timeout(Duration::from_secs(1), closed)
        .await
        .unwrap()
        .unwrap();
    client.close();
}
