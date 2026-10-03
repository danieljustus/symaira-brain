//! Compare real native CLI formatters against source-bound Go output bytes.
use super::{Report, render_report_table};
use symbrain_core::output::{self, OutputFormat};

#[test]
fn usage_fetch_620_cli_json_and_table_match_go_bytes() {
    let fresh = std::env::var_os("USAGE_FETCH_ORACLE_620");
    let data = fresh.map_or_else(
        || include_bytes!("../../symbrain-usage/tests/fixtures/usage_fetch_620.json").to_vec(),
        |path| std::fs::read(path).expect("fresh Go output oracle"),
    );
    let cases: Vec<serde_json::Value> = serde_json::from_slice(&data).expect("Go cases");
    assert_eq!(cases.len(), 133);
    let native: Option<Vec<serde_json::Value>> =
        std::env::var_os("USAGE_FETCH_NATIVE_620").map(|path| {
            serde_json::from_slice(&std::fs::read(path).expect("native fetch reports"))
                .expect("parse actual native reports")
        });
    if let Some(native) = &native {
        assert_eq!(native.len(), cases.len());
    }
    for (index, case) in cases.iter().enumerate() {
        let report_value = native.as_ref().map_or(&case["report"], |reports| {
            assert_eq!(reports[index]["id"], case["id"]);
            &reports[index]["report"]
        });
        let report: Report =
            serde_json::from_value(report_value.clone()).expect("typed native report");
        for (format, key) in [(OutputFormat::Json, "json"), (OutputFormat::Table, "table")] {
            let mut bytes = Vec::new();
            output::render(&mut bytes, format, &report, |writer| {
                render_report_table(writer, &report)
            })
            .expect("native formatter");
            assert_eq!(
                bytes,
                case[key].as_str().expect("Go output bytes").as_bytes(),
                "{}: {key}",
                case["id"]
            );
        }
    }
}
