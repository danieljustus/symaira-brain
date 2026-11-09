use super::{
    fetch_antigravity_from_observations, parse_antigravity_candidates, parse_antigravity_ports,
};
use crate::{Cancellation, FixtureTransport, Transport};
use std::sync::Arc;

#[test]
fn parses_every_go_antigravity_candidate_and_listening_port() {
    let process_list = concat!(
        " 101 /Applications/Antigravity.app/Contents/Resources/language_server --app_data_dir antigravity --csrf_token token-a\n",
        " 102 /opt/homebrew/bin/agy\n",
        " 103 /other/language_server --app_data_dir cursor\n",
    );
    assert_eq!(
        parse_antigravity_candidates(process_list),
        [
            super::AntigravityCandidate {
                pid: 101,
                csrf_token: Some("token-a".into()),
            },
            super::AntigravityCandidate {
                pid: 102,
                csrf_token: None,
            },
        ]
    );
    assert_eq!(
        parse_antigravity_ports(concat!(
            "COMMAND PID NAME\n",
            "language 101 TCP 127.0.0.1:43121 (LISTEN)\n",
            "language 101 TCP 127.0.0.1:43122 (LISTEN)\n",
            "language 101 TCP 127.0.0.1:43121 (LISTEN)\n",
            "language 101 TCP 127.0.0.1:43123 (ESTABLISHED)\n",
        )),
        [43121, 43122]
    );
}

#[test]
fn pid_above_go_lsof_limit_does_not_invoke_probe_or_send_http() {
    let provider = crate::providers::provider_config::antigravity_from_process_list(Some(
        "2147483648 /opt/homebrew/bin/agy\n",
    ));
    let fixture = FixtureTransport::default();
    let transport: Arc<dyn Transport> = Arc::new(fixture.clone());
    let cancellation = Cancellation::new();
    let mut lsof_called = false;
    let result = fetch_antigravity_from_observations(
        &provider,
        &transport,
        &cancellation,
        "2147483648 /opt/homebrew/bin/agy\n",
        |_| {
            lsof_called = true;
            Some("agy 2147483648 TCP 127.0.0.1:43121 (LISTEN)".into())
        },
    );
    assert!(result.is_err());
    assert!(!lsof_called);
    assert!(fixture.requests().is_empty());
}

#[test]
fn closed_app_has_no_cached_quota_fallback_and_sends_no_http() {
    // #934: no read-only source exists, so a closed app stays "not running".
    let provider = crate::providers::provider_config::antigravity_from_process_list(Some(""));
    let fixture = FixtureTransport::default();
    let transport: Arc<dyn Transport> = Arc::new(fixture.clone());
    let error = fetch_antigravity_from_observations(
        &provider,
        &transport,
        &Cancellation::new(),
        " 7 /usr/bin/unrelated\n",
        |_| panic!("no candidate may be probed"),
    )
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "Antigravity is not running — no local quota server found."
    );
    assert!(fixture.requests().is_empty());
}
