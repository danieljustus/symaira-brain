#![cfg(target_os = "macos")]

use serde_json::Value;
use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use symbrowse_engine_safari::{AttachEngine, AttachError, ScriptRunner};

const ORACLE_COMMIT: &str = "2ddbda51ef059181b99171ce7f87b6f6d8044c92";

fn fixture() -> Value {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/port/engine/safari.json");
    serde_json::from_slice(&fs::read(path).expect("Safari fixture")).expect("valid Safari fixture")
}

#[test]
fn safari_fixture_freezes_attach_and_bidi_boundaries() {
    let value = fixture();
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["suite"], "safari");
    assert_eq!(value["attach"]["engine_kind"], "safari-attach");
    assert_eq!(value["attach"]["launch_mode"], "attach");
    let attach = AttachEngine::new(FixtureNoopRunner);
    assert_eq!(
        serde_json::to_value(attach.capabilities()).expect("attach capabilities"),
        value["attach"]["capabilities"]
    );
    assert_eq!(
        value["attach"]["policy"]["invalid_target_before_runner"],
        true
    );
    assert_eq!(value["attach"]["lifecycle"]["close_quits_safari"], false);
    assert_eq!(value["bidi"]["engine_kind"], "safari-bidi");
    assert_eq!(value["bidi"]["launch_mode"], "launch");
    let bidi = symbrowse_engine_safari::BidiEngine::from_transport(
        Box::new(FixtureNoopTransport),
        "fixture-context",
    );
    assert_eq!(
        serde_json::to_value(bidi.capabilities()).expect("BiDi capabilities"),
        value["bidi"]["capabilities"]
    );
    assert_eq!(
        value["bidi"]["protocol"]["boolean_websocket_url_rejected"],
        true
    );
    assert_eq!(value["bidi"]["protocol"]["foreign_socket_rejected"], true);
    assert_eq!(
        value["bidi"]["lifecycle"]["session_delete_before_driver_stop"],
        true
    );
}

struct FixtureNoopRunner;

impl ScriptRunner for FixtureNoopRunner {
    fn run(&self, _script: &str, _timeout: Duration) -> Result<String, AttachError> {
        Ok(String::new())
    }
}

struct FixtureNoopTransport;

impl symbrowse_engine_safari::BidiTransport for FixtureNoopTransport {
    fn command<'a>(
        &'a mut self,
        _method: &'a str,
        _params: Value,
    ) -> symbrowse_engine_safari::BoxFuture<'a, Value> {
        Box::pin(async { Ok(Value::Null) })
    }

    fn close<'a>(&'a mut self) -> symbrowse_engine_safari::BoxFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
}

#[test]
fn safari_manifest_pins_the_go_oracle() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/port/engine/safari-manifest.json");
    let value: Value = serde_json::from_slice(&fs::read(path).expect("Safari manifest"))
        .expect("valid Safari manifest");
    assert_eq!(value["oracle_commit"], ORACLE_COMMIT);
    assert_eq!(value["native_gate"]["status"], "manual-gate");
}

#[derive(Clone)]
struct NavigationRunner {
    urls: Vec<String>,
    reads: Arc<Mutex<usize>>,
}

impl ScriptRunner for NavigationRunner {
    fn run(&self, script: &str, _timeout: Duration) -> Result<String, AttachError> {
        if script.contains("set URL of its document") {
            return Ok(String::new());
        }
        let mut reads = self.reads.lock().unwrap();
        let value = self
            .urls
            .get(*reads)
            .expect("navigation fixture exhausted")
            .clone();
        *reads += 1;
        Ok(value)
    }
}

#[test]
fn attach_navigation_settles_only_at_the_go_oracle_target() {
    let value = fixture();
    let cases = value["attach"]["navigation"]
        .as_array()
        .expect("Go navigation cases");
    assert_eq!(cases.len(), 2);
    for case in cases {
        let runner = NavigationRunner {
            urls: serde_json::from_value(case["observed_urls"].clone()).unwrap(),
            reads: Arc::new(Mutex::new(0)),
        };
        let engine = AttachEngine::new(runner.clone())
            .with_navigation_timeout(Duration::from_secs(1))
            .with_poll_interval(Duration::from_millis(1));
        let page = engine.new_page(&engine.new_context().unwrap()).unwrap();
        let target = case["target"].as_str().unwrap();
        let result = engine
            .navigate(&page, target)
            .expect("Go navigation succeeded");
        assert_eq!(
            result.frame_id, case["result"]["FrameID"],
            "{}",
            case["name"]
        );
        assert_eq!(
            result.loader_id, case["result"]["LoaderID"],
            "{}",
            case["name"]
        );
        assert_eq!(
            result.error_text, case["result"]["ErrorText"],
            "{}",
            case["name"]
        );
        assert_eq!(result.url, target, "{}", case["name"]);
        assert_eq!(
            *runner.reads.lock().unwrap(),
            case["url_reads"].as_u64().unwrap() as usize,
            "{}",
            case["name"]
        );
    }
}
