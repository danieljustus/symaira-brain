//! Actual pinned-Go public parser and full TLS constructor contract.
use super::*;
use crate::{Provider, Request, Response, Transport};

struct Mock(String);
impl Transport for Mock {
    fn request(&self, _request: Request) -> Result<Response, String> {
        Ok(Response {
            status: 429,
            body: b"{}".to_vec(),
            headers: BTreeMap::from([("retry-after".into(), self.0.clone())]),
        })
    }
}

fn decoded(value: &str) -> Vec<u8> {
    assert_eq!(value.len() % 2, 0, "complete byte-oriented hex input");
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

#[test]
#[ignore = "requires actual frozen Go scalar processes and private TLS peer"]
fn retry_after_oracle_matches_fresh_go() {
    compare_contract(1600, 1164);
}

#[test]
#[ignore = "requires actual frozen Go decimal/scalar processes and private TLS peer"]
fn retry_after_decimal_oracle_matches_fresh_go() {
    compare_contract(1681, 1356);
}

fn compare_contract(public_count: usize, tls_count: usize) {
    let corpus: Value = serde_json::from_slice(
        &std::fs::read(std::env::var("USAGE_RETRY_INPUT").unwrap()).unwrap(),
    )
    .unwrap();
    let go: Value =
        serde_json::from_slice(&std::fs::read(std::env::var("USAGE_RETRY_GO").unwrap()).unwrap())
            .unwrap();
    let inputs = corpus["public"].as_array().unwrap();
    let rows = go["records"].as_array().unwrap();
    assert_eq!(inputs.len(), public_count, "complete public scalar inputs");
    assert_eq!(rows.len(), inputs.len(), "complete public scalar Go corpus");
    assert_eq!(go["sdk"], "go1.26.7");
    let operating_system = match std::env::consts::OS {
        "macos" => "darwin",
        other => other,
    };
    assert_eq!(
        go["goos"], operating_system,
        "actual native SDK operating system"
    );
    assert_eq!(go["int_bits"], isize::BITS);
    let architecture = match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        "x86" => "386",
        other => other,
    };
    assert_eq!(go["goarch"], architecture, "actual native SDK architecture");
    let mut output = vec![];
    for (row, input) in rows.iter().zip(inputs) {
        assert_eq!(row["id"], input["id"]);
        assert_eq!(row["value_hex"], input["value_hex"]);
        let bytes = decoded(input["value_hex"].as_str().unwrap());
        let text = std::str::from_utf8(&bytes).ok();
        let bits = text
            .and_then(crate::parse_retry_after)
            .map(|v| format!("{:016x}", v.to_bits()));
        assert_eq!(
            json!(bits),
            row["bits"],
            "{} public RetryAfter binary64",
            row["id"]
        );
        let seconds = text
            .and_then(crate::retry_after::seconds)
            .map(|v| v.to_string());
        assert_eq!(
            json!(seconds),
            row["seconds"],
            "{} target SDK integer conversion",
            row["id"]
        );
        if let Some(value) = text {
            let report = Service::with_transport(
                vec![Provider::fixture("kimi", "Kimi")],
                Arc::new(Mock(value.into())),
            )
            .report();
            assert_eq!(
                report.providers[0].error.as_deref().unwrap(),
                format!(
                    "all AI usage fallbacks failed: {}",
                    row["diagnostic"].as_str().unwrap()
                ),
                "{} public parser consumer",
                row["id"]
            );
        }
        output.push(json!({"id":row["id"],"value_hex":input["value_hex"],"bits":bits,"seconds":seconds,"matched":true}));
    }
    let status = super::status::compare_with_count(tls_count);
    std::fs::write(std::env::var("USAGE_RETRY_NATIVE").unwrap(), serde_json::to_vec_pretty(&json!({"public_cases":public_count,"tls_cases":tls_count,"goos":go["goos"],"goarch":go["goarch"],"sdk":go["sdk"],"int_bits":go["int_bits"],"public":output,"statuses":status})).unwrap()).unwrap();
}
