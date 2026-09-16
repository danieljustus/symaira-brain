//! Review anchor for the raw-byte Go capture.
//!
//! Keep this Rust source independent from the Python generator, validator, and
//! fixture. The native gate parses these literals, while Rust acceptance tests
//! compile and check them. Updating this file requires independent review and
//! deliberate fixture regeneration; it is not derived by any check command.

pub const TRUSTED_ORACLE_COMMIT: &str = "0b585d52915a824664e1377d0a995dff3f5405cd";
pub const TRUSTED_GO_TOOLCHAIN: &str = "go1.26.7";
pub const TRUSTED_FIXTURE_SHA256: &str =
    "8685934276bf32dbdc1631a6c997e5dd3ca48a4fb29747a7d79686472b9ec098";
pub const TRUSTED_GENERATOR_SHA256: &str =
    "f8593d20a7260a42199ae96df387bb58cc4c2e366de9c33d0a1302b8a4e5c141";
pub const TRUSTED_VALIDATOR_SHA256: &str =
    "fdded3c530277b7fa9fce7ec3b594a02ff7f42e6ce0124d805ee8141740dad35";
pub const TRUSTED_CASE_COUNT: usize = 152;
pub const TRUSTED_SOURCE_FILES: [(&str, &str); 3] = [
    (
        "cmd/symbrain/cmd_guard.go",
        "cb0b10ae03a69b3c5c1d6952b706710a488ac4219fc908218c254056f2432abc",
    ),
    (
        "cmd/symbrain/main.go",
        "46a3245236f0104f34c7e0d0787fcb35c27a0eba6c6bba8d5a5098e1664c6e5b",
    ),
    (
        "guard/cmd/symguard/decide/command.go",
        "22fed158de555e04de72cb66fb63751c8812142e858df65ac0985a7cb450620f",
    ),
];
