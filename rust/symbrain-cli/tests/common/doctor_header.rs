//! The one normalization for `guard doctor`'s frozen header block, shared by
//! every consumer of a frozen `doctor` fixture.
//!
//! `doctor` prints `  Version:   <v>` / `  Go:        <go>` / `  OS/Arch:
//! <os>/<arch>` — capitalized label, colon, run of spaces. Two of those three
//! rows can never be matched byte-for-byte by a Rust build:
//!
//! * the toolchain row is `runtime.Version()` in Go; the native binary
//!   relabels it `Rust:` and prints its real `rustc` version rather than
//!   fabricating a Go one (a fabrication was rejected in an earlier wave), so
//!   both labels reduce to the same token — the row's presence and position
//!   stay compared, the unpinnable label and value do not pretend to match;
//! * the `OS/Arch:` row is the real host platform, which legitimately differs
//!   from whatever machine recorded the fixture.
//!
//! Both patterns match only this exact "  Label:   value" shape, which was
//! verified by grep to appear nowhere else across the frozen corpora, so they
//! cannot swallow a real difference elsewhere.

/// Reduces `guard doctor`'s two unpinnable header rows to fixed tokens.
pub fn normalize(s: &str) -> String {
    let toolchain_row = regex::Regex::new(r"(?m)^[ \t]*(?:Go|Rust):[ \t]+.*$").unwrap();
    let out = toolchain_row.replace_all(s, "  <toolchain>:").to_string();
    let os_arch_row = regex::Regex::new(r"(?m)^[ \t]*OS/Arch:[ \t]+.*$").unwrap();
    os_arch_row.replace_all(&out, "  <os-arch>:").to_string()
}
