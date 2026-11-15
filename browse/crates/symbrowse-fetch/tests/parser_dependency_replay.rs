//! Stable-Rust replay of the `httparse-response` and `h2-client` fuzz targets
//! (`browse/fuzz`): the third-party HTTP/1 and HTTP/2 parsers that read remote
//! server bytes through hyper and reqwest. Every seed, every prefix of it and a
//! deterministic byte-mutation sweep must be handled totally; the HTTP/2 client
//! exchange must also finish on its own.
#![deny(unsafe_code)]

use std::path::PathBuf;
use std::time::Duration;

use tokio::io::AsyncWriteExt;

fn seeds(target: &str) -> Vec<(String, Vec<u8>)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fuzz/corpus")
        .join(target);
    let mut seeds: Vec<_> = std::fs::read_dir(&dir)
        .unwrap_or_else(|error| panic!("{}: {error}", dir.display()))
        .map(|entry| {
            let path = entry.unwrap().path();
            (
                path.file_name().unwrap().to_string_lossy().into_owned(),
                std::fs::read(&path).unwrap(),
            )
        })
        .collect();
    seeds.sort();
    assert!(!seeds.is_empty(), "no seeds for {target}");
    seeds
}

/// Every seed, every prefix of it, and a deterministic single-byte mutation sweep.
fn sweep(target: &str, mut exercise: impl FnMut(&[u8])) {
    for (name, seed) in seeds(target) {
        exercise(&seed);
        for end in 0..seed.len() {
            exercise(&seed[..end]);
        }
        for index in 0..seed.len().min(128) {
            for replacement in [0x00, b'\n', b'\r', b':', 0x7f, 0xff] {
                let mut mutated = seed.clone();
                mutated[index] = replacement;
                exercise(&mutated);
            }
        }
        eprintln!("replayed {target}/{name}");
    }
}

#[test]
fn httparse_response_seeds_are_total_and_bounded() {
    sweep("httparse_response", |data| {
        let mut headers = [httparse::EMPTY_HEADER; 64];
        let mut response = httparse::Response::new(&mut headers);
        let first = response.parse(data);
        if let Ok(httparse::Status::Complete(used)) = first {
            assert!(used <= data.len());
            assert!(
                response
                    .headers
                    .iter()
                    .all(|header| !header.name.is_empty())
            );
        }
        let mut again_headers = [httparse::EMPTY_HEADER; 64];
        assert_eq!(
            first,
            httparse::Response::new(&mut again_headers).parse(data)
        );

        let mut header_block = [httparse::EMPTY_HEADER; 64];
        if let Ok(httparse::Status::Complete((used, _))) =
            httparse::parse_headers(data, &mut header_block)
        {
            assert!(used <= data.len());
        }
        if let Ok(httparse::Status::Complete((used, _))) = httparse::parse_chunk_size(data) {
            assert!(used <= data.len());
        }
        let mut request_headers = [httparse::EMPTY_HEADER; 64];
        let _ = httparse::Request::new(&mut request_headers).parse(data);
    });
}

async fn exchange(data: &[u8]) {
    let (client_io, mut server_io) = tokio::io::duplex(1 << 20);
    let _ = server_io.write_all(&data[..data.len().min(1 << 19)]).await;
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

#[test]
fn h2_client_seeds_terminate_without_panicking() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    sweep("h2_client", |data| {
        runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(10), exchange(data))
                .await
                .expect("h2 client exchange must finish on its own");
        });
    });
}
