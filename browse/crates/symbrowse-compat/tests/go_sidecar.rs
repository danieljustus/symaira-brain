use std::{
    io::{Read, Write},
    net::TcpListener,
    time::Duration,
};

use symbrowse_compat::{CompatClient, Request};

#[tokio::test]
#[ignore = "requires the native Go compat executable"]
async fn production_go_sidecar_exchanges_versioned_requests() {
    let binary = std::env::var("SYMBROWSE_COMPAT_BINARY")
        .expect("SYMBROWSE_COMPAT_BINARY must point to the native Go executable");
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fixture");
    let address = listener.local_addr().expect("fixture address");
    let server = std::thread::spawn(move || {
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().expect("accept Go request");
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .expect("read timeout");
            let mut request = [0; 4096];
            let count = stream.read(&mut request).expect("read Go request");
            assert!(request[..count].starts_with(b"GET /compat HTTP/1.1"));
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\ncompat",
                )
                .expect("write fixture response");
        }
    });
    let mut client = CompatClient::new(
        binary,
        ["compat-sidecar".to_owned()],
        Duration::from_secs(15),
    );
    for _ in 0..2 {
        let response = client
            .request_with_timeout(Request {
                id: 0,
                method: "GET".into(),
                url: format!("http://{address}/compat"),
                profile: "chrome".into(),
                headers: vec![],
                body: String::new(),
                timeout_ms: 10_000,
                max_body_bytes: 1024,
            })
            .await
            .expect("Go sidecar response");
        assert!(response.ok, "{response:?}");
        assert_eq!(response.status, 200);
        assert_eq!(response.body, "compat");
    }
    server.join().expect("fixture server");
}
