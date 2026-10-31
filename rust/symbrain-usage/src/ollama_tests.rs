use super::*;
use ring::signature::{ED25519, UnparsedPublicKey};

fn string(out: &mut Vec<u8>, value: &[u8]) {
    out.extend(u32::try_from(value.len()).unwrap().to_be_bytes());
    out.extend(value);
}
fn fixture() -> (Vec<u8>, Vec<u8>) {
    let seed = [42; 32]; // generated test-only material, never user credentials
    let key = Ed25519KeyPair::from_seed_unchecked(&seed).unwrap();
    let mut public = Vec::new();
    string(&mut public, b"ssh-ed25519");
    string(&mut public, key.public_key().as_ref());
    let mut private = Vec::new();
    private.extend(123_u32.to_be_bytes());
    private.extend(123_u32.to_be_bytes());
    string(&mut private, b"ssh-ed25519");
    string(&mut private, key.public_key().as_ref());
    let mut secret = seed.to_vec();
    secret.extend(key.public_key().as_ref());
    string(&mut private, &secret);
    string(&mut private, b"offline Ollama fixture");
    let padding = 8 - private.len() % 8;
    for i in 1..=padding {
        private.push(u8::try_from(i).unwrap());
    }
    let mut blob = b"openssh-key-v1\0".to_vec();
    string(&mut blob, b"none");
    string(&mut blob, b"none");
    string(&mut blob, b"");
    blob.extend(1_u32.to_be_bytes());
    string(&mut blob, &public);
    string(&mut blob, &private);
    (blob, public)
}
fn pem(blob: &[u8]) -> Vec<u8> {
    format!(
        concat!(
            "-----BEGIN OPENSSH ",
            "PRIVATE KEY-----\n{}\n-----END OPENSSH PRIVATE KEY-----\n"
        ),
        STANDARD.encode(blob)
    )
    .into_bytes()
}

#[test]
fn signed_request_and_tampering() {
    let (blob, public) = fixture();
    let (key, parsed) = parse_key(&pem(&blob)).unwrap();
    assert_eq!(parsed, public);
    let request = signed_request(&key, &public, 1_234_567_890);
    assert_eq!(request.url, "https://ollama.com/api/balance?ts=1234567890");
    assert_eq!(request.method, "GET");
    let parts: Vec<_> = request.headers["Authorization"].split(':').collect();
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[0], STANDARD.encode(&public));
    let signature = STANDARD.decode(parts[1]).unwrap();
    assert_eq!(signature.len(), 64);
    let verify = UnparsedPublicKey::new(&ED25519, key.public_key().as_ref());
    verify
        .verify(b"GET,/api/balance?ts=1234567890", &signature)
        .unwrap();
    for bad in [
        "POST,/api/balance?ts=1234567890",
        "GET,/api/balance?ts=1234567891",
        "GET,https://ollama.com/api/balance?ts=1234567890",
    ] {
        assert!(verify.verify(bad.as_bytes(), &signature).is_err());
    }
    assert!(!format!("{request:?}").contains(parts[1]));
}
#[test]
fn container_is_exact_bounded_and_checks_every_public_copy() {
    let (blob, _) = fixture();
    let key = Ed25519KeyPair::from_seed_unchecked(&[42; 32]).unwrap();
    let positions: Vec<_> = blob
        .windows(32)
        .enumerate()
        .filter_map(|(i, b)| (b == key.public_key().as_ref()).then_some(i))
        .collect();
    assert_eq!(positions.len(), 3);
    for position in positions {
        let mut corrupt = blob.clone();
        corrupt[position] ^= 1;
        assert!(parse_key(&pem(&corrupt)).is_none());
    }
    for i in 0..blob.len() {
        assert!(parse_key(&pem(&blob[..i])).is_none(), "truncated at {i}");
    }
    let mut corrupt = blob.clone();
    corrupt.push(0);
    assert!(parse_key(&pem(&corrupt)).is_none());
    let mut corrupt = blob.clone();
    let end = corrupt.len() - 1;
    corrupt[end] = 0;
    assert!(parse_key(&pem(&corrupt)).is_none());
    let mut corrupt = blob.clone();
    corrupt[19] = b'x';
    assert!(parse_key(&pem(&corrupt)).is_none()); // cipher
    let mut corrupt = blob.clone();
    let mut reader = Wire(&blob[15..]);
    reader.string().unwrap();
    reader.string().unwrap();
    reader.string().unwrap();
    reader.u32().unwrap();
    reader.string().unwrap();
    let offset = blob.len() - reader.0.len() + 4;
    corrupt[offset] ^= 1;
    assert!(parse_key(&pem(&corrupt)).is_none());
    let mut trailing = pem(&blob);
    trailing.extend(b"garbage");
    assert!(parse_key(&trailing).is_none());
    assert!(parse_key(&vec![b'x'; 65537]).is_none());
}
#[test]
fn quotas_and_no_data() {
    for (body, used, count, balance) in [
        (
            r#"{"included":{"balance_usd":72.5,"allowance_usd":100,"period":{"until":"2026-10-15T09:30:00Z"}},"purchased":{"balance_usd":25}}"#,
            "27.5",
            1,
            Some("25"),
        ),
        (
            r#"{"included":{"session":{"remaining_percent":75},"weekly":{"remaining_percent":"40"}}}"#,
            "25",
            2,
            None,
        ),
        (
            r#"{"included":{"balance_usd":100,"allowance_usd":100},"purchased":{"balance_usd":0}}"#,
            "0",
            1,
            Some("0"),
        ),
        (
            r#"{"included":{"session":{"remaining_percent":200}}}"#,
            "0",
            1,
            None,
        ),
    ] {
        let snapshot = parse_balance(body.as_bytes(), chrono::Utc::now()).unwrap();
        assert_eq!(snapshot.meters.len(), count);
        assert_eq!(snapshot.meters[0].used.as_deref(), Some(used));
        assert_eq!(snapshot.balance.as_deref(), balance);
        if used == "27.5" {
            assert!(snapshot.meters[0].resets_at.is_some());
            assert_eq!(snapshot.meters[0].unit, "USD");
        }
    }
    for body in [
        r#"{"included":{},"purchased":{"balance_usd":25}}"#,
        r#"{"included":{"session":{"remaining_percent":"NaN"}}}"#,
        "[]",
        "null",
        "not JSON",
    ] {
        assert!(parse_balance(body.as_bytes(), chrono::Utc::now()).is_err());
    }
}

#[cfg(unix)]
#[test]
fn private_file_and_offline_http_boundaries() {
    use crate::{FixtureTransport, Response};
    use std::os::unix::fs::{PermissionsExt, symlink};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("key");
    let (blob, _) = fixture();
    std::fs::write(&path, pem(&blob)).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let provider = Provider::ollama();
    let fixture = FixtureTransport::new(BTreeMap::from([(
        "ollama".into(),
        Response {
            status: 200,
            body: br#"{"included":{"session":{"remaining_percent":75}}}"#.to_vec(),
            headers: BTreeMap::new(),
        },
    )]));
    let transport: Arc<dyn Transport> = Arc::new(fixture.clone());
    let snapshot = provider
        .fetch_ollama_at(&path, &transport, &Cancellation::new())
        .unwrap();
    assert_eq!(snapshot.meters[0].used.as_deref(), Some("25"));
    let requests = fixture.requests();
    assert_eq!(requests.len(), 1);
    assert!(
        requests[0]
            .url
            .starts_with("https://ollama.com/api/balance?ts=")
    );
    let (key, public) = parse_key(&pem(&blob)).unwrap();
    let header = &requests[0].headers["Authorization"];
    let parts: Vec<_> = header.split(':').collect();
    assert_eq!(parts[0], STANDARD.encode(public));
    let signature = STANDARD.decode(parts[1]).unwrap();
    let uri = requests[0].url.strip_prefix("https://ollama.com").unwrap();
    UnparsedPublicKey::new(&ED25519, key.public_key().as_ref())
        .verify(format!("GET,{uri}").as_bytes(), &signature)
        .unwrap();
    for status in [302, 401, 403, 429, 500] {
        let transport: Arc<dyn Transport> = Arc::new(FixtureTransport::new(BTreeMap::from([(
            "ollama".into(),
            Response {
                status,
                body: b"{}".to_vec(),
                headers: BTreeMap::new(),
            },
        )])));
        assert!(
            provider
                .fetch_ollama_at(&path, &transport, &Cancellation::new())
                .is_err()
        );
    }
    for mode in [0o644, 0o620, 0o4700] {
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        assert!(read_key(&path).is_err());
    }
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let link = dir.path().join("symlink");
    symlink(&path, &link).unwrap();
    assert!(read_key(&link).is_err());
    let link = dir.path().join("hardlink");
    std::fs::hard_link(&path, &link).unwrap();
    assert!(read_key(&link).is_err());
    assert!(read_key(&path).is_err());
    std::fs::remove_file(link).unwrap();
    assert!(read_key(dir.path()).is_err());
    assert!(read_key(&dir.path().join("missing")).is_err());
    let fifo = dir.path().join("fifo");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    assert!(read_key(&fifo).is_err());
    std::fs::write(&path, vec![0; 65537]).unwrap();
    assert!(read_key(&path).is_err());
}
