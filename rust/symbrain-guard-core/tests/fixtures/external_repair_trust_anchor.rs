//! Review anchor for the raw-byte Go capture.
//!
//! Keep this Rust source independent from the Python generator, validator, and
//! fixture. The native gate parses these literals, while Rust acceptance tests
//! compile and check them. Updating this file requires independent review and
//! deliberate fixture regeneration; it is not derived by any check command.

pub const TRUSTED_ORACLE_COMMIT: &str = "0b585d52915a824664e1377d0a995dff3f5405cd";
pub const TRUSTED_GO_TOOLCHAIN: &str = "go1.26.7";
pub const TRUSTED_FIXTURE_SHA256: &str =
    "f4039e115827baeb6678a5b5fa3b838e2d553e7188b44a88743c10b9e820949b";
pub const TRUSTED_GENERATOR_SHA256: &str =
    "7a36a7a447ff1ce8065b4076bde6ed58059dd8d12e12f1452f1a9544e3e59fcb";
pub const TRUSTED_VALIDATOR_SHA256: &str =
    "3e74a37c61e418b98b591d46551d311f257cfcf183b10d70e0089a2165716ed8";
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
