# Package-local Skills YAML emitter

This directory contains selective, package-local forks of the existing YAML libraries used only by `symbrain-skills` output encoding. The parser and all other workspace YAML callers continue to use the unchanged workspace `serde_yaml_ng` dependency.

## Upstream provenance and licenses

- `serde_yaml_ng-0.10.0`: upstream `https://github.com/acatton/serde-yaml-ng`, MIT, package archive SHA-256 `7b4db627b98b36d4203a7b458cf3573730f2bb591b28871d916dfa9efabfd41f` (the registry checksum in the base `Cargo.lock`). Retain `LICENSE`.
- `unsafe-libyaml-0.2.11`: upstream `https://github.com/dtolnay/unsafe-libyaml`, MIT, package archive SHA-256 `673aac59facbab8a9007c7f6108d11f63b603f7cabff99fabf650fea5c32b861` (the registry checksum in the base `Cargo.lock`). Retain `LICENSE-MIT`.

The checked-in sources were copied selectively from those checksum-verified crate archives: normalized/original manifests, README, license, and library source files only. Test suites, build outputs, diagnostic corpora, binaries, and diagnostic archives are not included. Both package names, versions, authors, and upstream repository identities remain intact. The local `serde_yaml_ng` dependency points to the local `unsafe-libyaml` source so future emitter changes remain scoped; `symbrain-skills` refers to this crate through the `serde_yaml_ng_skill_encoder` dependency alias. The two vendor packages are excluded from the root workspace member set while remaining local path dependencies. Upstream test-only dependencies (`anyhow`, `indoc`, `serde_derive`, and `pretty_assertions`) are omitted from the fork manifests because their test suites are not vendored.

## Compatibility delta

The package-local `serde_yaml_ng` emitter adds a double-quoted scalar style and uses libyaml's four-space indentation. Its string inference selects double quotes for values that the existing YAML scalar resolver recognizes as booleans or numbers, rather than single quotes; ordinary strings retain the library's normal style. The fork makes the upstream FFI field borrows explicit where Rust 1.98 rejects implicit raw-pointer autoref; it uses no lint suppression. `symbrain-skills::encode_skill_md` emits the library bytes directly inside the frontmatter delimiters and appends the body bytes unchanged. No YAML byte postpass or global Cargo patch is used.

The subsequent source-bound policy includes yaml.v3's legacy boolean, base60, timestamp and numeric-string inference, supplementary Unicode escaping, and block-sequence indentation. Leading-LF strings and literal `<<` metadata keys use the explicitly approved lossless targets described in `docs/adr/skills-yaml-value-preservation-835.md`; frozen oracle data is not rewritten. No byte postpass is introduced.

This remains an unaccepted scoped repair, not a parity waiver. The 740 scalar rows, ordinary render/materialize golden trees and local Skills tests have been exercised. The cumulative typed-document corpus, metadata ordering/float cases, strict vendor lint, independent review and required CLI/MCP/native gates remain outstanding. In particular, existing vendor Clippy errors are not suppressed or excluded.

## Upgrade path

When either upstream crate changes, first verify its registry archive against the new Cargo.lock checksum, then refresh only the selected upstream package files while retaining license and repository identity. Reapply and review the small source-level delta, update these provenance checksums, and rerun the Skills-specific numeric/control regressions, the 740 scalar corpus, all typed documents, all admitted controls, actual render/materialize/hash callers, crate formatting/check/test/clippy and license/dependency review. Do not move this fork to `[patch.crates-io]`; retire it only after the scoped encoder's full compatibility evidence is green and parser/global callers remain unchanged.
