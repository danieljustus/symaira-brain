# #769 slice 1 evidence (local macOS arm64, Go 1.26.7 toolchain, no CI)

* `oracle-report-de6530b.json` / `oracle-pass-de6530b.log`: `scripts/config-harness-oracle/run.sh` at clean candidate `de6530bf`, Go built from immutable `dcddcef0…`; 290/290 executed, 116 new + 174 legacy, no skips on macOS, Rust fallback absent.
* `negative-control-oracle-fail.log`: same oracle after mutating `flag provided but not defined` to `flag provided but undefined` in `go_flags.rs` (reverted afterwards): exit 1, 21 new cases fail, 268 pass. `negative-control-cargo-fail.log`: `config_harness_native` fails with the same mutation.
* `cargo-test-symbrain-cli.log`, `clippy-symbrain-cli.log`: acceptance runs; fmt check exited 0 (no output).
* Before the change, the same shapes against the base Rust binary with an absent fallback were measured in-session (exit 1 `start Go fallback` for every config flag shape; wrong bytes for harness -h/--help/help, list `--`, `-`, `--=x`, `-h`, `--project=`); that probe output was not retained as a file.
* Not evidence of native three-OS CI.
