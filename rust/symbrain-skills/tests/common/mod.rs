// Each integration-test binary includes this module but only uses one fixture.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

fn fixture(env_name: &str, checked_in_name: &str) -> Vec<u8> {
    let path = std::env::var_os(env_name).map_or_else(
        || {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures")
                .join(checked_in_name)
        },
        PathBuf::from,
    );
    fs::read(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

pub fn skills_oracle() -> Vec<u8> {
    fixture("SYMBRAIN_SKILLS_ORACLE_FIXTURE", "oracle_expectations.json")
}

pub fn skills_install_oracle() -> Vec<u8> {
    fixture(
        "SYMBRAIN_SKILLS_INSTALL_ORACLE_FIXTURE",
        "install_status_oracle.json",
    )
}
