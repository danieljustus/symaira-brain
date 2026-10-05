//! Prepared general JSON conversion bound to the retained actual SDK81 corpus.
#[test]
fn all_historical_sdk_numbers_keep_json_domain_and_exact_binary64_bits() {
    let oracle: serde_json::Value = serde_json::from_str(include_str!(
        "../tests/fixtures/go_json_float_sdk_linux_amd64.json"
    ))
    .unwrap();
    assert_eq!(oracle["sdk"], "go1.26.7");
    assert_eq!(oracle["goos"], "linux");
    assert_eq!(oracle["goarch"], "amd64");
    let records = oracle["records"].as_array().unwrap();
    assert_eq!(records.len(), 81);
    for row in records {
        let raw = row["value_hex"]
            .as_str()
            .unwrap()
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect::<Vec<_>>();
        let text = std::str::from_utf8(&raw).unwrap();
        let expected = if super::json_number(&raw) && row["error"] == "<nil>" {
            row["float_bits"].clone()
        } else {
            serde_json::Value::Null
        };
        let native = super::parse_finite(text).map(|number| format!("{:016x}", number.to_bits()));
        assert_eq!(serde_json::json!(native), expected, "{}", row["id"]);
    }
}
