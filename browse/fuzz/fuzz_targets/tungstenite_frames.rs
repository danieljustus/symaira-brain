#![no_main]

//! Attacker-controlled WebSocket bytes as the pinned `tungstenite` sees them
//! (the version `async-tungstenite` links for the Chrome, Firefox and Safari
//! engines). The first byte selects the mode: an established client socket
//! reading server frames, or a client opening handshake reading a server
//! response. Message and frame sizes are capped so a hostile length prefix
//! cannot allocate unbounded memory, and reading must stop on its own.
//! Keep the body in sync with `crates/symbrowse-engine-chrome/tests/fuzz_seed_replay.rs`.

use std::io::{self, Cursor, Read, Write};

use libfuzzer_sys::fuzz_target;
use tungstenite::client::IntoClientRequest;
use tungstenite::protocol::{Role, WebSocket, WebSocketConfig};

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

fuzz_target!(|data: &[u8]| {
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
        let request = "ws://localhost/".into_client_request().expect("static request");
        let _ = tungstenite::client::client(request, stream);
    }
});
