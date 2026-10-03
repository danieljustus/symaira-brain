//! Retain only LLVM's profiling destination across hermetic Rust subprocesses.
//! Without it instrumented children write .profraw files into fixture trees,
//! corrupt filesystem comparisons, and lose coverage when temp roots disappear.
use std::ffi::OsString;

pub fn profile_environment() -> Option<(&'static str, OsString)> {
    std::env::var_os("LLVM_PROFILE_FILE").map(|path| ("LLVM_PROFILE_FILE", path))
}
