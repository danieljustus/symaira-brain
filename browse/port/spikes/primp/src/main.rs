#![forbid(unsafe_code)]

use std::error::Error;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use base64::Engine as _;
use primp::{Client, Impersonate, ImpersonateOS};
use rcgen::generate_simple_self_signed;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::{ServerConfig, ServerConnection, StreamOwned};
use serde::Serialize;

const H2_PREFACE: &[u8; 24] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";
const MAX_FRAME_LEN: usize = 1 << 20;
const LOCAL_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Copy)]
struct CandidateProfile {
    name: &'static str,
    browser: Option<Impersonate>,
    os: Option<ImpersonateOS>,
}

const PROFILES: [CandidateProfile; 6] = [
    CandidateProfile {
        name: "chrome",
        browser: Some(Impersonate::ChromeV153),
        os: Some(ImpersonateOS::Windows),
    },
    CandidateProfile {
        name: "edge",
        browser: Some(Impersonate::EdgeV153),
        os: Some(ImpersonateOS::Windows),
    },
    CandidateProfile {
        name: "firefox",
        browser: Some(Impersonate::FirefoxV151),
        os: Some(ImpersonateOS::Windows),
    },
    CandidateProfile {
        name: "ios",
        browser: Some(Impersonate::SafariV26_4),
        os: Some(ImpersonateOS::IOS),
    },
    CandidateProfile {
        name: "opera",
        browser: Some(Impersonate::OperaV135),
        os: Some(ImpersonateOS::Windows),
    },
    CandidateProfile {
        name: "safari",
        browser: Some(Impersonate::SafariV26_4),
        os: Some(ImpersonateOS::MacOS),
    },
];

#[derive(Serialize)]
struct Capture {
    profile: &'static str,
    protocol: &'static str,
    raw_client_hello_record_b64: String,
    raw_settings_frame_b64: String,
    raw_header_block_b64: String,
    h2_settings: Vec<String>,
    header_names: Vec<String>,
}

#[derive(Serialize)]
struct SpikeOutput {
    schema_version: u8,
    capture_kind: &'static str,
    parity_acceptance: bool,
    note: &'static str,
    worktree_sha: String,
    profiles: Vec<Capture>,
}

struct WireCapture {
    client_hello_record: Vec<u8>,
    settings_payload: Vec<u8>,
    header_block: Vec<u8>,
}

struct TapStream {
    stream: TcpStream,
    received: Arc<Mutex<Vec<u8>>>,
}

impl Read for TapStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.stream.read(buf)?;
        if n != 0 {
            self.received
                .lock()
                .map_err(|_| io::Error::other("capture lock poisoned"))?
                .extend_from_slice(&buf[..n]);
        }
        Ok(n)
    }
}

impl Write for TapStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.stream.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}

fn server_config() -> Result<Arc<ServerConfig>, Box<dyn Error>> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let generated = generate_simple_self_signed(vec!["localhost".to_owned()])?;
    let cert: CertificateDer<'static> = generated.cert.der().clone();
    let key = PrivateKeyDer::try_from(generated.signing_key.serialize_der())?;
    let mut config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert], key)?;
    config.alpn_protocols = vec![b"h2".to_vec()];
    Ok(Arc::new(config))
}

fn read_frame<R: Read>(stream: &mut R) -> io::Result<(u8, u8, u32, Vec<u8>)> {
    let mut header = [0u8; 9];
    stream.read_exact(&mut header)?;
    let len =
        (usize::from(header[0]) << 16) | (usize::from(header[1]) << 8) | usize::from(header[2]);
    if len > MAX_FRAME_LEN {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "HTTP/2 frame exceeds capture bound",
        ));
    }
    let mut payload = vec![0; len];
    stream.read_exact(&mut payload)?;
    let stream_id = u32::from_be_bytes([header[5] & 0x7f, header[6], header[7], header[8]]);
    Ok((header[3], header[4], stream_id, payload))
}

fn write_frame<W: Write>(
    stream: &mut W,
    frame_type: u8,
    flags: u8,
    stream_id: u32,
    payload: &[u8],
) -> io::Result<()> {
    if payload.len() > 0x00ff_ffff || payload.len() > MAX_FRAME_LEN {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "HTTP/2 response frame is too large",
        ));
    }
    let len = payload.len();
    let header = [
        (len >> 16) as u8,
        (len >> 8) as u8,
        len as u8,
        frame_type,
        flags,
        ((stream_id >> 24) as u8) & 0x7f,
        (stream_id >> 16) as u8,
        (stream_id >> 8) as u8,
        stream_id as u8,
    ];
    stream.write_all(&header)?;
    stream.write_all(payload)?;
    stream.flush()
}

fn capture_server(listener: TcpListener, config: Arc<ServerConfig>) -> io::Result<WireCapture> {
    let (tcp, _peer): (TcpStream, SocketAddr) = listener.accept()?;
    tcp.set_read_timeout(Some(LOCAL_TIMEOUT))?;
    tcp.set_write_timeout(Some(LOCAL_TIMEOUT))?;
    let received = Arc::new(Mutex::new(Vec::new()));
    let tapped = TapStream {
        stream: tcp,
        received: Arc::clone(&received),
    };
    let connection = ServerConnection::new(config)
        .map_err(|error| io::Error::other(format!("TLS server init: {error}")))?;
    let mut stream = StreamOwned::new(connection, tapped);

    // A test-only loopback TLS server. The client bypasses trust checks only
    // for this 127.0.0.1 request, mirroring the Go capture harness overlay.
    write_frame(&mut stream, 4, 0, 0, &[])?;
    let mut preface = [0u8; 24];
    stream.read_exact(&mut preface)?;
    if &preface != H2_PREFACE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid HTTP/2 client preface",
        ));
    }

    let mut settings_payload = None;
    let mut header_stream_id = None;
    let mut header_block = Vec::new();
    let mut headers_complete = false;
    while !headers_complete {
        let (kind, flags, stream_id, payload) = read_frame(&mut stream)?;
        match kind {
            4 if flags & 1 == 0 => {
                if settings_payload.is_none() {
                    settings_payload = Some(payload);
                }
                write_frame(&mut stream, 4, 1, 0, &[])?;
            }
            1 => {
                header_stream_id = Some(stream_id);
                let mut start = 0;
                let mut end = payload.len();
                if flags & 8 != 0 {
                    let padding = usize::from(*payload.first().ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidData, "missing HEADERS padding length")
                    })?);
                    start += 1;
                    if padding > end.saturating_sub(start) {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "HEADERS padding exceeds frame",
                        ));
                    }
                    end -= padding;
                }
                if flags & 0x20 != 0 {
                    start += 5;
                }
                if start > end {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "invalid HEADERS frame prefix",
                    ));
                }
                header_block.extend_from_slice(&payload[start..end]);
                headers_complete = flags & 4 != 0;
            }
            9 if header_stream_id == Some(stream_id) => {
                header_block.extend_from_slice(&payload);
                headers_complete = flags & 4 != 0;
            }
            _ => {}
        }
    }

    let settings_payload = settings_payload.ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "client sent no SETTINGS frame")
    })?;
    let stream_id = header_stream_id.ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "client sent no HEADERS frame")
    })?;
    // HPACK index 8 is the static-table entry `:status: 200`.
    write_frame(&mut stream, 1, 5, stream_id, &[0x88])?;

    let received = received
        .lock()
        .map_err(|_| io::Error::other("capture lock poisoned"))?;
    if received.len() < 5 || received[0] != 0x16 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "captured bytes do not start with a TLS handshake record",
        ));
    }
    let hello_len = 5 + usize::from(u16::from_be_bytes([received[3], received[4]]));
    if received.len() < hello_len {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "captured ClientHello record is truncated",
        ));
    }
    Ok(WireCapture {
        client_hello_record: received[..hello_len].to_vec(),
        settings_payload,
        header_block,
    })
}

fn settings_order(payload: &[u8]) -> io::Result<Vec<String>> {
    if payload.len() % 6 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "SETTINGS payload is not a sequence of 6-byte entries",
        ));
    }
    Ok(payload
        .chunks_exact(6)
        .map(|setting| {
            let id = u16::from_be_bytes([setting[0], setting[1]]);
            let value = u32::from_be_bytes([setting[2], setting[3], setting[4], setting[5]]);
            format!("{id}={value}")
        })
        .collect())
}

async fn run_capture(
    profile: CandidateProfile,
    config: Arc<ServerConfig>,
) -> Result<Capture, Box<dyn Error>> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let address = listener.local_addr()?;
    let server = thread::spawn(move || capture_server(listener, config));

    let mut builder = Client::builder()
        .tls_danger_accept_invalid_certs(true)
        .timeout(LOCAL_TIMEOUT);
    if let Some(browser) = profile.browser {
        builder = builder.impersonate(browser);
    }
    if let Some(os) = profile.os {
        builder = builder.impersonate_os(os);
    }
    let client = builder.build()?;
    let response = client
        .get(format!("https://127.0.0.1:{}/capture", address.port()))
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(format!(
            "{} loopback response status was {}",
            profile.name,
            response.status()
        )
        .into());
    }
    let wire = server
        .join()
        .map_err(|_| io::Error::other("loopback capture server panicked"))??;
    let mut decoder = hpack::Decoder::new();
    let decoded = decoder
        .decode(&wire.header_block)
        .map_err(|error| io::Error::other(format!("decode captured HTTP/2 headers: {error:?}")))?;
    let header_names = decoded
        .into_iter()
        .map(|(name, _value)| String::from_utf8(name))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Capture {
        profile: profile.name,
        protocol: "h2",
        raw_client_hello_record_b64: base64::engine::general_purpose::STANDARD
            .encode(wire.client_hello_record),
        raw_settings_frame_b64: base64::engine::general_purpose::STANDARD
            .encode(&wire.settings_payload),
        raw_header_block_b64: base64::engine::general_purpose::STANDARD.encode(&wire.header_block),
        h2_settings: settings_order(&wire.settings_payload)?,
        header_names,
    })
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 1 {
        return Err("usage: fetch002-primp-wire-spike <capture-output.json>".into());
    }
    let config = server_config()?;
    let mut profiles = Vec::with_capacity(7);
    for profile in PROFILES {
        profiles.push(run_capture(profile, Arc::clone(&config)).await?);
    }
    profiles.push(
        run_capture(
            CandidateProfile {
                name: "honest-default",
                browser: None,
                os: None,
            },
            config,
        )
        .await?,
    );
    let sha =
        std::env::var("SYMBROWSE_SPIKE_SHA").unwrap_or_else(|_| "local-uncommitted".to_owned());
    let output = SpikeOutput {
        schema_version: 1,
        capture_kind: "loopback_hermetic_primp_candidate",
        parity_acceptance: false,
        note: "Diagnostic spike only; raw observations are compared with same-SHA Go/AzureTLS captures. No FETCH-002 parity acceptance is asserted.",
        worktree_sha: sha,
        profiles,
    };
    std::fs::write(&args[0], serde_json::to_vec_pretty(&output)?)?;
    Ok(())
}
