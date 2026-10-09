pub const ORACLE_REVISION: &str = "a92385d2deecc08d1fd96869908b81b7abd355fe";
pub const GO_TOOLCHAIN: &str = "go1.26.7";
pub const CURRENT_FIXTURE_SHA256: &str =
    "0c5826ae84f597c0b5a047661b32b4e3dde236c7d3a6ba9d68cda972ba370293";
pub const HISTORICAL_FIXTURE_SHA256: &str =
    "9a2547e73b1c660048b66b6f98736b019483c8df4d321bbbc3c8a506b1d06fdf";

pub const EVIDENCE_HASHES: &[(&str, &str)] = &[
    (
        "scripts/usage-request-oracle/opencode/main.go",
        "9fc64ed9c17a3cca413110acded261778196042c5f5cfb205bd175a31d8a6de2",
    ),
    (
        "scripts/usage-request-oracle/opencode/cases.go",
        "125be7ed13f8bb868deccf63770bf923fcf5328f84d9da4198565ceb4be42544",
    ),
    (
        "scripts/usage-request-oracle/opencode/review_cases.go",
        "0c60f79cdf5a5c1ac610e4689575e9153d04bf8752baf69563454f3ba1f8a831",
    ),
    (
        "scripts/usage-request-oracle/opencode/provenance.go",
        "136cd9561d01758b3e5c6effbd6945265d1659a6878266dd79eba4bd248f4809",
    ),
    (
        "rust/symbrain-usage/src/opencode_discovery_tests.rs",
        "ae89fd8c5b0f453ee2b6bcf76152d6ff4a08d6df2eecf72c5b90f27eb4a017a9",
    ),
    (
        "rust/symbrain-usage/src/opencode_provenance_tests.rs",
        "cb36e9c3752d50cf76916505c1157ce2c9ffca8bce6736c4b2cb47fb617503cf",
    ),
];
