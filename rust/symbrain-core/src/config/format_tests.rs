use super::*;
use crate::config::run_config_get_with_path;
use std::ffi::OsString;
use std::fs;
use tempfile::tempdir;
use toml_edit::DocumentMut;

#[test]
fn get_key_formats_floats_including_neg_zero_infinities_nan_and_scientific_boundaries() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let content = r"
pos_zero = 0.0
neg_zero = -0.0
pos_inf = inf
pos_inf_plus = +inf
neg_inf = -inf
nan = nan
pos_nan = +nan
neg_nan = -nan
large = 1e6
large2 = 1e5
small = 1e-4
small2 = 1e-5
large_neg = -1e6
large2_neg = -1e5
small_neg = -1e-4
small2_neg = -1e-5
sci1 = 6.022e23
sci2 = 1.23e-10
pi = 3.14
boundary_below_large = 999999.9
boundary_above_small = 0.000100001
boundary_below_small = 0.000099999
";
    fs::write(&path, content).unwrap();

    let test_cases = [
        ("pos_zero", "0\n"),
        ("neg_zero", "-0\n"),
        ("pos_inf", "+Inf\n"),
        ("pos_inf_plus", "+Inf\n"),
        ("neg_inf", "-Inf\n"),
        ("nan", "NaN\n"),
        ("pos_nan", "NaN\n"),
        ("neg_nan", "NaN\n"),
        ("large", "1e+06\n"),
        ("large2", "100000\n"),
        ("small", "0.0001\n"),
        ("small2", "1e-05\n"),
        ("large_neg", "-1e+06\n"),
        ("large2_neg", "-100000\n"),
        ("small_neg", "-0.0001\n"),
        ("small2_neg", "-1e-05\n"),
        ("sci1", "6.022e+23\n"),
        ("sci2", "1.23e-10\n"),
        ("pi", "3.14\n"),
        ("boundary_below_large", "999999.9\n"),
        ("boundary_above_small", "0.000100001\n"),
        ("boundary_below_small", "9.9999e-05\n"),
    ];

    for (key, want) in test_cases {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let code =
            run_config_get_with_path(&path, &[OsString::from(key)], &mut stdout, &mut stderr);
        assert_eq!(code, crate::exit::OK, "failed for key {key}");
        assert_eq!(
            String::from_utf8(stdout).unwrap(),
            want,
            "mismatch for key {key}"
        );
        assert!(stderr.is_empty(), "stderr not empty for key {key}");
    }
}

#[test]
#[allow(clippy::too_many_lines)]
fn format_go_datetime_matches_burnt_sushi_utc_variants() {
    // Go-oracle generated expectations in UTC (offset 0)
    let oracle_cases_utc = [
        ("1979-05-27T07:32:00Z", "1979-05-27 07:32:00 +0000 UTC"),
        ("1979-05-27t07:32:00z", "1979-05-27 07:32:00 +0000 UTC"),
        ("1979-05-27 07:32:00Z", "1979-05-27 07:32:00 +0000 UTC"),
        (
            "1979-05-27T07:32:00.999999Z",
            "1979-05-27 07:32:00.999999 +0000 UTC",
        ),
        (
            "1979-05-27T07:32:00.123Z",
            "1979-05-27 07:32:00.123 +0000 UTC",
        ),
        (
            "1979-05-27T07:32:00.123456789Z",
            "1979-05-27 07:32:00.123456789 +0000 UTC",
        ),
        (
            "1979-05-27T07:32:00.120000Z",
            "1979-05-27 07:32:00.12 +0000 UTC",
        ),
        ("1979-05-27T07:32:00.000Z", "1979-05-27 07:32:00 +0000 UTC"),
        ("1979-05-27T07:32Z", "1979-05-27 07:32:00 +0000 UTC"),
        (
            "1979-05-27T07:32:00+02:00",
            "1979-05-27 07:32:00 +0200 +0200",
        ),
        (
            "1979-05-27T00:32:00-07:00",
            "1979-05-27 00:32:00 -0700 -0700",
        ),
        ("1979-05-27T00:32:00+00:00", "1979-05-27 00:32:00 +0000 UTC"),
        ("1979-05-27T00:32:00-00:00", "1979-05-27 00:32:00 +0000 UTC"),
        (
            "1979-05-27 00:32:00-07:00",
            "1979-05-27 00:32:00 -0700 -0700",
        ),
        (
            "1979-05-27T00:32:00.999999-07:00",
            "1979-05-27 00:32:00.999999 -0700 -0700",
        ),
        (
            "1979-05-27T00:32:00.123+05:30",
            "1979-05-27 00:32:00.123 +0530 +0530",
        ),
        (
            "1979-05-27T00:32:00.120000-07:00",
            "1979-05-27 00:32:00.12 -0700 -0700",
        ),
        ("1979-05-27T07:32-05:00", "1979-05-27 07:32:00 -0500 -0500"),
        (
            "1979-05-27T07:32:00",
            "1979-05-27 07:32:00 +0000 datetime-local",
        ),
        (
            "1979-05-27 07:32:00",
            "1979-05-27 07:32:00 +0000 datetime-local",
        ),
        (
            "1979-05-27t07:32:00",
            "1979-05-27 07:32:00 +0000 datetime-local",
        ),
        (
            "1979-05-27T07:32:00.999999",
            "1979-05-27 07:32:00.999999 +0000 datetime-local",
        ),
        (
            "1979-05-27T07:32:00.123",
            "1979-05-27 07:32:00.123 +0000 datetime-local",
        ),
        (
            "1979-05-27T07:32:00.123456789",
            "1979-05-27 07:32:00.123456789 +0000 datetime-local",
        ),
        (
            "1979-05-27T07:32:00.120000",
            "1979-05-27 07:32:00.12 +0000 datetime-local",
        ),
        (
            "1979-05-27T07:32:00.000",
            "1979-05-27 07:32:00 +0000 datetime-local",
        ),
        (
            "1979-05-27T07:32",
            "1979-05-27 07:32:00 +0000 datetime-local",
        ),
        ("1979-05-27", "1979-05-27 00:00:00 +0000 date-local"),
        ("2026-09-05", "2026-09-05 00:00:00 +0000 date-local"),
        ("0001-01-01", "0001-01-01 00:00:00 +0000 date-local"),
        ("07:32:00", "0000-01-01 07:32:00 +0000 time-local"),
        (
            "07:32:00.999999",
            "0000-01-01 07:32:00.999999 +0000 time-local",
        ),
        ("07:32:00.123", "0000-01-01 07:32:00.123 +0000 time-local"),
        (
            "07:32:00.123456789",
            "0000-01-01 07:32:00.123456789 +0000 time-local",
        ),
        ("07:32:00.120000", "0000-01-01 07:32:00.12 +0000 time-local"),
        ("07:32:00.000", "0000-01-01 07:32:00 +0000 time-local"),
        ("07:32", "0000-01-01 07:32:00 +0000 time-local"),
    ];

    for (input, expected) in oracle_cases_utc {
        let doc: DocumentMut = format!("v = {input}\n").parse().unwrap();
        let dt = doc
            .get("v")
            .unwrap()
            .as_value()
            .unwrap()
            .as_datetime()
            .unwrap();
        let mut out = String::new();
        format_go_datetime_with_offset(dt, 0, &mut out);
        assert_eq!(&out, expected, "Mismatch for input {input}");
    }
}

#[test]
fn format_go_datetime_matches_burnt_sushi_berlin_variants() {
    // Go-oracle generated expectations in Berlin (offset +7200)
    let oracle_cases_berlin = [
        ("1979-05-27T07:32:00Z", "1979-05-27 07:32:00 +0000 UTC"),
        (
            "1979-05-27T07:32:00+02:00",
            "1979-05-27 07:32:00 +0200 +0200",
        ),
        (
            "1979-05-27T00:32:00-07:00",
            "1979-05-27 00:32:00 -0700 -0700",
        ),
        (
            "1979-05-27T00:32:00+00:00",
            "1979-05-27 00:32:00 +0000 +0000",
        ),
        (
            "1979-05-27T07:32:00",
            "1979-05-27 07:32:00 +0200 datetime-local",
        ),
        ("1979-05-27", "1979-05-27 00:00:00 +0200 date-local"),
        ("07:32:00", "0000-01-01 07:32:00 +0200 time-local"),
    ];

    for (input, expected) in oracle_cases_berlin {
        let doc: DocumentMut = format!("v = {input}\n").parse().unwrap();
        let dt = doc
            .get("v")
            .unwrap()
            .as_value()
            .unwrap()
            .as_datetime()
            .unwrap();
        let mut out = String::new();
        format_go_datetime_with_offset(dt, 7200, &mut out);
        assert_eq!(&out, expected, "Mismatch for Berlin input {input}");
    }
}
