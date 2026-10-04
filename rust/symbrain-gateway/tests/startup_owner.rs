//! Startup ownership never grants capabilities or retries a failed store owner.
use std::{collections::BTreeMap, fs, path::PathBuf, sync::Arc};

use symbrain_gateway::Gateway;
use symbrain_mcp::Request;
use symbrain_memory::{MemoryRuntime, SecretOptions};

struct Owned(PathBuf);
impl Owned {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("gateway-startup-owner-{}", std::process::id()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Owned {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn hidden_call(gateway: &Gateway) {
    let request: Request = serde_json::from_str(
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"memory_set","arguments":{"id":"owned-denied","content":"must never write"}}}"#,
    ).unwrap();
    let response = gateway.handle(&request).unwrap().unwrap();
    let error = response.error.expect("unexposed tool must fail");
    assert_eq!(error.code, symbrain_mcp::CODE_METHOD_NOT_FOUND);
    assert_eq!(error.message, "Unknown tool: memory_set");
}

#[test]
fn eager_owner_does_not_expose_or_mutate_disabled_memory() {
    let root = Owned::new();
    let db = root.0.join("database/default.db");
    let owner = MemoryRuntime::open(
        &db,
        || {
            Ok(SecretOptions {
                primary: b"owned-synthetic-key".to_vec(),
                path: Ok(root.0.join("config/jwt.secret")),
            })
        },
        &mut Vec::new(),
    )
    .unwrap();
    let before = fs::read(db.with_file_name("default.db-wal")).unwrap();
    let profile = symbrain_policy::profile::parse::parse(
        "[profile]\nname='owned'\n[servers.memory]\nenabled=false\n",
    )
    .unwrap();
    let gateway =
        Gateway::new_with_memory_runtime(profile, BTreeMap::new(), "dev", Some(Arc::new(owner)))
            .unwrap();
    assert!(
        !gateway
            .tools()
            .iter()
            .any(|tool| tool.name.starts_with("memory_") || tool.name.starts_with("activity_"))
    );
    hidden_call(&gateway);
    assert_eq!(
        fs::read(db.with_file_name("default.db-wal")).unwrap(),
        before
    );
    assert!(
        !root.0.join("config/jwt.secret").exists(),
        "literal primary must not provision a file"
    );
}

#[test]
fn failed_supplied_owner_hides_memory_without_constructor_retry() {
    let profile = symbrain_policy::profile::parse::parse(
        "[profile]\nname='owned'\n[servers.memory]\nenabled=true\nmode='read_write'\n",
    )
    .unwrap();
    let gateway = Gateway::new_with_memory_runtime(profile, BTreeMap::new(), "dev", None).unwrap();
    assert!(
        !gateway
            .tools()
            .iter()
            .any(|tool| tool.name.starts_with("memory_") || tool.name.starts_with("activity_"))
    );
    hidden_call(&gateway);
}
