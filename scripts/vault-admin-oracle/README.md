# Native vault administration process oracle

Build the complete production Go CLI from an immutable repository archive and
compare it with the native Rust CLI, using disposable HOME/XDG roots and a
compiled synthetic `symvault` child:

```sh
cargo build -p symbrain-cli --locked
python3 scripts/vault-admin-oracle/replay.py \
  --go-ref dcddcef0df5789123c7c9a7ebe6e01f10e941f2c \
  --rust target/debug/symbrain \
  --out /tmp/vault-admin-parity.json
```

On Windows use `target/debug/symbrain.exe`. The pinned Go 1.26.7 and Rust 1.98.0
toolchains must be available. No Go source is patched. The report records source
and child hashes, actual exit codes, stdout/stderr, and the child's raw argv and
stdin for each of 197 Unix cases and 182 Windows cases. Only the disposable root in diagnostics is
normalized, including the Go-quoted Windows representation. A regression
checks that this keeps other paths and error differences significant. A
mismatch exits nonzero; setup failure and timeouts are failures.
All data and secret values in this corpus are synthetic.

The CLI process test also replays 126 portable Go-derived records from
`rust/symbrain-cli/tests/fixtures/vault_admin_go.json`. That fixture stores Go
outputs only and binds the child-source hash; Rust cannot generate its expected
results. A separate `vault_unicode_go.json` adds 18 Go-only records: three
valid U+FFFD controls on all platforms and 15 raw invalid UTF-8 cases on Unix.
The original portable fixture remains unchanged. Platform-specific diagnostics and config failures are compared live
on each OS. The three native CI jobs retain their JSON reports as artifacts.
The Linux checkpoint is in
`migration/evidence/vault-admin-766/linux-process-parity.json`.

Coverage includes create/set/delete validation, single-line raw secret input,
metadata-only confirmation, mismatched values, invalid/null/repeated/numeric
JSON, deletion verification, discovery precedence, global/project/TOML/env
configuration boundaries, per-byte malformed UTF-8 path escaping versus valid
U+FFFD characters, and opaque passthrough arguments/status. The shared
native JSON nesting limit and bounded child process cleanup remain in force;
this corpus does not claim exhaustive behavior or satisfy the broader Rust
release/cutover requirements.
