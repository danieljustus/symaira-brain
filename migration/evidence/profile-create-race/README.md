# Atomic profile creator race evidence

`original-windows-ci.log` is the unchanged failing native job111291463063 from
run37153314306. It contains the original PermissionDenied/AlreadyExists mismatch,
actual merge checkout provenance, and a later passing ordinary rerun. It is not
rewritten or replaced by corrected results.

`linux-verification.json` binds corrected clean source0e71ef14 to source hashes,
the original log,104 policy tests,23 CLI init/profile integration tests, five CLI
init unit tests, strict checks, and all264 observed creator outcomes. Raw Linux
logs are retained alongside it. The source manifest includes production, tests,
workspace lockfile, affected CLI callers and native CI wiring.

Reproduce with Rust1.98, an exclusively owned target directory, dev/test debug0
and incremental0, running under a Linux subprocess subreaper:

- `cargo test --locked -p symbrain-policy --all-features -- --nocapture`
- `cargo test --locked -p symbrain-cli --all-features --test init_cli_tests --test profile_cli_tests`
- `cargo test --locked -p symbrain-cli --all-features --lib init_cli::tests`
- `cargo clippy --locked -p symbrain-policy -p symbrain-cli --all-targets --all-features -- -D warnings`
- `cargo fmt --all --check` and `actionlint .github/workflows/ci.yml`

No Windows/macOS corrected-candidate proof or independent approval is claimed.
Native focused CI retains thread/process logs even when a preceding check fails.
See `docs/adr/profile-create-atomic-publication.md` for the algorithm and limits.
