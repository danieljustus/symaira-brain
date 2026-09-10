//! Go-oracle differential fixtures for CAT-001.

use serde::Deserialize;
use serde_json::Value;
use symbrain_catalog::{Catalog, ServerTools, Tool};
use symbrain_policy::Report;

#[derive(Debug, Deserialize)]
struct Suite {
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
struct Case {
    id: String,
    servers: Vec<ServerInput>,
    success: bool,
    #[serde(default)]
    entries: Vec<Value>,
    #[serde(default)]
    names: Vec<String>,
    #[serde(default)]
    error: String,
}

#[derive(Debug, Deserialize)]
struct ServerInput {
    server: String,
    tools: Vec<Tool>,
    report: Report,
}

#[test]
fn catalog_matches_go_oracle() {
    let suite: Suite = serde_json::from_slice(include_bytes!("fixtures/oracle_expectations.json"))
        .expect("parse catalog oracle fixture");

    for case in suite.cases {
        let servers = case
            .servers
            .into_iter()
            .map(|server| ServerTools {
                server: server.server,
                tools: server.tools,
                report: server.report,
            })
            .collect::<Vec<_>>();
        let result = Catalog::build(&servers);

        if case.success {
            let catalog = result
                .unwrap_or_else(|error| panic!("case {}: expected success, got {error}", case.id));
            let actual_entries = serde_json::to_value(catalog.all()).expect("serialize entries");
            assert_eq!(
                actual_entries,
                Value::Array(case.entries),
                "case {}: entries differ from Go oracle",
                case.id
            );
            let actual_names = catalog
                .names()
                .into_iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>();
            assert_eq!(actual_names, case.names, "case {}: names differ", case.id);
        } else {
            let error = result.expect_err(&format!(
                "case {}: expected collision, catalog built",
                case.id
            ));
            assert_eq!(
                error.to_string(),
                case.error,
                "case {}: error differs",
                case.id
            );
        }
    }
}
