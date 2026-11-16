# Skills YAML scalar quoting after the Go removal (#835)

**Status:** accepted (maintainer decision 2026-11-16).

## Context

The retired Go skills renderer (`gopkg.in/yaml.v3`) emitted string scalars that
look like numbers with double quotes, for example `version: "2"`. The Rust
renderer (`rust/symbrain-skills/src/encode.rs`, serialized through
`serde_yaml_ng`) emits the same value with single quotes: `version: '2'`.

Both spellings are valid YAML and parse to the same string. The bytes differ,
so `SKILL.md` render output, the render `output_digest` and the install
marker `source_hash` differ from what the Go implementation produced for the
same library skill. The Go implementation and its toolchain were removed in
#1003, so no live oracle exists to validate a new Go-style emitter.

## Decision

Keep the Rust single-quoted scalar style as a documented difference from the
retired Go encoder. No Go-style emitter is built and no fixture is re-sealed.

## Consequences

- Renders produced by the Rust releases already use this style. The
  difference is only visible when comparing against a render produced by the
  Go implementation (v0.12.x and earlier): same parsed metadata, different
  bytes and hashes.
- `rust/symbrain-skills/tests/yaml_dependency_compatibility.rs` pins the
  current single-quoted output and the typed round trip, so an accidental
  style change is caught.
- Sealed Go oracle fixtures stay unchanged as historical evidence.
