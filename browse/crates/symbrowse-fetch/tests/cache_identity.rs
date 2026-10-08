//! Real transport routes and cookie jars remain independent for delimiter strings.
use symbrowse_fetch::{FetchClient, Request};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

async fn proxy(label: &'static str) -> String {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut input = [0u8; 8192];
            let n = socket.read(&mut input).await.unwrap();
            let request = String::from_utf8_lossy(&input[..n]);
            let cookie = request
                .to_ascii_lowercase()
                .contains("cookie: isolated=first");
            let body = format!("{label}:{}", if cookie { "cookie" } else { "empty" });
            let set_cookie = if request.contains("/set-cookie ") {
                "Set-Cookie: isolated=first; Path=/\r\n"
            } else {
                ""
            };
            let response = format!(
                "HTTP/1.1 200 OK\r\n{set_cookie}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(response.as_bytes()).await.unwrap();
        }
    });
    format!("http://{address}")
}

#[tokio::test]
async fn delimiter_collision_cannot_change_proxy_route_or_attach_another_sessions_cookie_jar() {
    let first = proxy("first").await;
    let second = proxy("second").await;
    let client = FetchClient::honest().unwrap();
    let mut request = Request::get("http://93.184.216.34/set-cookie");
    request.allow_private = true;
    request.session = Some("A".to_owned());
    request.proxy = Some(format!("{first}/x;proxy={second}/y"));
    assert_eq!(
        client.fetch(request.clone()).await.unwrap().body,
        b"first:empty"
    );

    request.url = "http://93.184.216.34/cookie".to_owned();
    request.session = Some(format!("A;proxy={first}/x"));
    request.proxy = Some(format!("{second}/y"));
    assert_eq!(
        client.fetch(request.clone()).await.unwrap().body,
        b"second:empty"
    );

    request.session = Some("A".to_owned());
    request.proxy = Some(format!("{first}/x"));
    assert_eq!(client.fetch(request).await.unwrap().body, b"first:cookie");
    client.close();
}
