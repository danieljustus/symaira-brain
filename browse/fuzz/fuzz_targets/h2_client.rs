#![no_main]

//! Attacker-controlled HTTP/2 bytes as the pinned `h2` client sees them:
//! reqwest negotiates HTTP/2 with remote servers, so the frame decoder, HPACK
//! and flow control read server-chosen bytes. The input is the whole server
//! side of an in-memory connection (it is closed after the last byte), one
//! request is issued, and the exchange must finish on its own within a
//! deadline.
//! Keep the body in sync with `crates/symbrowse-fetch/tests/parser_dependency_replay.rs`.

use std::time::Duration;

use libfuzzer_sys::fuzz_target;
use tokio::io::AsyncWriteExt;

const MAX_INPUT: usize = 1 << 19;

thread_local! {
    static RUNTIME: tokio::runtime::Runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .expect("runtime");
}

async fn exchange(data: &[u8]) {
    let (client_io, mut server_io) = tokio::io::duplex(1 << 20);
    let _ = server_io.write_all(&data[..data.len().min(MAX_INPUT)]).await;
    drop(server_io);
    let Ok((send, connection)) = h2::client::handshake(client_io).await else {
        return;
    };
    let request = http::Request::get("https://example.com/")
        .body(())
        .expect("static request");
    let stream = async move {
        let mut send = send.ready().await.ok()?;
        let (response, _) = send.send_request(request, true).ok()?;
        let mut body = response.await.ok()?.into_body();
        while let Some(chunk) = body.data().await {
            let chunk = chunk.ok()?;
            let _ = body.flow_control().release_capacity(chunk.len());
        }
        let _ = body.trailers().await;
        Some(())
    };
    let _ = tokio::join!(connection, stream);
}

fuzz_target!(|data: &[u8]| {
    RUNTIME.with(|runtime| {
        runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(10), exchange(data))
                .await
                .expect("h2 client exchange must finish on its own");
        });
    });
});
