//! Stable-Rust replay of the `tungstenite-frames` fuzz target (`browse/fuzz`):
//! the WebSocket frame reader and client handshake parser that the Chrome,
//! Firefox and Safari engines run on browser-supplied bytes (through
//! `async-tungstenite`'s re-export, so the replay uses the production version).
//! Every seed, every prefix and a deterministic byte-mutation sweep must be
//! handled without panicking, within the configured size caps.
#![deny(unsafe_code)]

use std::io::{self, Cursor, Read, Write};
use std::path::PathBuf;

use async_tungstenite::tungstenite::client::IntoClientRequest;
use async_tungstenite::tungstenite::protocol::{Role, WebSocket, WebSocketConfig};

const LIMIT: usize = 1 << 16;

/// Reads the scripted bytes, then EOF; every write is accepted and dropped.
struct Script<'a>(Cursor<&'a [u8]>);

impl Read for Script<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

impl Write for Script<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn seeds() -> Vec<(String, Vec<u8>)> {
    let dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus/tungstenite_frames");
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
    assert!(!seeds.is_empty(), "no tungstenite_frames seeds");
    seeds
}

fn exercise(data: &[u8]) {
    let Some((&mode, rest)) = data.split_first() else {
        return;
    };
    let stream = Script(Cursor::new(rest));
    if mode % 2 == 0 {
        let config = WebSocketConfig::default()
            .max_message_size(Some(LIMIT))
            .max_frame_size(Some(LIMIT));
        let mut socket = WebSocket::from_raw_socket(stream, Role::Client, Some(config));
        for _ in 0..64 {
            match socket.read() {
                Ok(message) => assert!(message.len() <= LIMIT),
                Err(_) => break,
            }
        }
    } else {
        let request = "ws://localhost/"
            .into_client_request()
            .expect("static request");
        let _ = async_tungstenite::tungstenite::client::client(request, stream);
    }
}

#[test]
fn tungstenite_frames_seeds_are_total_and_bounded() {
    for (name, seed) in seeds() {
        exercise(&seed);
        for end in 0..seed.len() {
            exercise(&seed[..end]);
        }
        for index in 0..seed.len().min(128) {
            for replacement in [0x00, 0x01, 0x80, 0x7e, 0x7f, 0xff] {
                let mut mutated = seed.clone();
                mutated[index] = replacement;
                exercise(&mutated);
            }
        }
        eprintln!("replayed tungstenite_frames/{name}");
    }
}
