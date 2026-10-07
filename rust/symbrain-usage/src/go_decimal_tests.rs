//! Unexecuted native regressions bound to actual SDK-only Linux/AMD64 floats.
//! Integer/provider/native-platform results require the full fresh process gate.
#[test]
fn long_decimal_public_values_match_original_actual_sdk_bits() {
    let oracle: serde_json::Value = serde_json::from_str(include_str!(
        "../tests/fixtures/retry_decimal_sdk_linux_amd64.json"
    ))
    .unwrap();
    assert_eq!(oracle["sdk"], "go1.26.7");
    assert_eq!(oracle["goos"], "linux");
    assert_eq!(oracle["goarch"], "amd64");
    let rows = oracle["records"].as_array().unwrap();
    assert_eq!(rows.len(), 81);
    for row in rows {
        let raw = row["value_hex"]
            .as_str()
            .unwrap()
            .as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect::<Vec<_>>();
        let text = std::str::from_utf8(&raw).unwrap();
        let actual =
            crate::parse_retry_after(text).map(|value| format!("{:016x}", value.to_bits()));
        assert_eq!(
            serde_json::json!(actual),
            row["retry_bits"],
            "{} actual SDK reference",
            row["id"]
        );
    }
}
