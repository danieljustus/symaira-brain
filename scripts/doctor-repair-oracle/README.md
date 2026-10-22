`run.sh OUTPUT_JSON` builds the complete production Go CLI from
`0541e64a36d83bac8e5e4f8c4136033c03db83ae` (the #765 base; CoreKit v0.18.2), builds the
candidate Rust CLI, and compares real `doctor --fix`, `setup --fix` and
`setup --from-source` subprocesses with an absent Go fallback.
It replays 567 cases on Linux, 556 on macOS and 551 on Windows (Unix signals/permissions
and Windows large exitcodes have separate native fixtures). A supplemental native
Go fixture supplies version responses; it never changes the oracle, production Go or frozen cases.

Cases cover correct/missing/mismatched versions, nonzero probes and the actual
three-second timeout, typed/duplicate/case-folded/null fields, malformed JSON,
invalid UTF-8 and unpaired escapes, literal Go time.Time parsing, source and
corrupt-origin protection, explicit forced replacement, optional-core config
and nonzero config merging, boolean flags, Go flag stop rules and successful
verified installation. New cases clean managed owner paths lexically before touching symlinks, retain actual leaf/ancestor mkdir obstruction errors, preserve raw Unix managed paths through stdout and provenance logs,
fail before probes/writes when HOME or USERPROFILE is empty/missing, reject Windows
HOMEDRIVE/HOMEPATH fallback owners, and reject malformed excess-dash flags before
any protected fixture can be probed or replaced, preserve the valid triple-dash
normalization control and compare raw Unix flag/boolean diagnostics. Successful fixture installs require real checksum
verification and the real pinned cosign arguments; the local verifier checks
identity, issuer and signature/certificate association and records one exclusive
receipt per core. It is a fixture, not a production signature authority.

The comparison is strict for stdout, exit, full fixture files/modes, every
per-core log sequence and attribute, and the completion tail. Only test roots,
validated in-window log timestamps and newly written verified-UTC release
sidecar timestamps differ by construction. The new raw-HOME forced-directory
case also normalizes the generated provenance rename source basename, as proven
by two retained actual Go runs. It requires the exact owned bin directory,
core name, destination, syscall and error; every other byte remains exact.
Go's ActiveCores map order varies
between actual processes; only order between separate cores is disregarded.
All compared per-core and tail lines store their exact bytes as base64,
including non-UTF-8 usage errors. Probe-call files remain exact, so repeated or missing probes cannot disappear.

Three actual wrapper processes run the real Rust CLI and deliberately change
one result: wrong exit, missing stdout header, or missing core event. Each replay
must fail for exactly that intended observable. A crashed/incomplete replay is
not an accepted control. The native CI always preserves all four JSON reports
for 14 days. Linux evidence alone does not establish macOS/Windows acceptance.

Native Cargo tests independently run with no Go CLI and no release host,
checking that source/corrupt records and binaries stay unchanged, optional
cores are selected, failed downloads continue, and invalid full configuration is
diagnosed natively without creating the managed binary directory. The decision
and reasons are recorded in `docs/adr/2026-10-03-native-doctor-repair.md`.

## Invalid-configuration cases (#765)

233 cases (`config-error-*`, `config-order-*`, `config-control-*`) run the Go
`config.Load()` failure paths for eight entry points (`setup`, `setup --fix`
with and without `--json`, `setup --from-source .` with and without `--modules`,
`doctor --fix` with `--json` and `--force-release`) against 26 rejected
configurations: TOML syntax errors, wrong value types for every `fileConfig`
leaf and for each Go dynamic type (`[]interface {}`, `time.Time`,
`map[string]interface {}`, `[]map[string]interface {}`), string conversions
(`ParseBool`, `ParseInt`, out of range), field order, a project-file failure
after a valid global file, `SYMBRAIN_*` overrides (including present-but-empty),
non-UTF-8, UTF-16 and NUL input, a configuration path that is a directory, and
a missing managed directory. Configuration documents are written as explicit
bytes (never `write_text`, which would turn `\n` into CRLF on Windows), and
explicit `\r\n` rows pin that the Go lexer treats `\r` as the line break (its
diagnostics name `'\r'`; `errorPrevLine` sites report line 0 on the first line).
On Windows the directory row additionally pins that Go's read-only directory
handle opens and fails the read (`read <path>: Incorrect function.`). Lone `\r`
not followed by `\n` is Go's control-character error at the first such byte; the
native reader does not reproduce that wording yet (no oracle row reaches it).
Stdout, the exact stderr tail, the exit code (2)
and the full managed-directory snapshot (nothing written) are compared.
Ordering cases pin that the `--from-source` root/flag checks precede loading and
that loading precedes module selection; control cases pin valid configurations.

### Oracle revision

The pin moved from the frozen `dcddcef0` (CoreKit v0.17.0, which skipped
zero-valued TOML fields and empty environment values) to the #765 base, whose
`go.mod` requires CoreKit v0.18.2 (present values, including `false`, `0`,
`""`, are converted and applied; `os.LookupEnv` replaces the non-empty check).
The only Go difference in `internal/config`, `cmd/symbrain` setup/doctor,
`internal/managed` and `internal/xdg` between the two revisions is that
dependency bump, so the existing cases (`config-3`, `config-4`,
`nonzero-config-merge`, `env-browse-`) now compare against the shipped
behavior: a later `browse = false` clears an earlier `true`, and `0`/`""`/an
empty `SYMBRAIN_MODULES_BROWSE` are rejected. The setup-repair and setup-source
oracles moved to the same revision (their module-config cases exercise the same
loader); oracles unrelated to configuration loading still pin `dcddcef0`.

### Measured residuals (parser dialect)

`toml_edit` (TOML 1.1) decides syntax validity; Go's wording is reconstructed
for the statement shapes in `setup_config_toml_tests.rs` (90 recorded real-Go
outcomes from the 0541e64a binary, all executed, 84 reproduced byte for byte).
The other 6 are listed as `UNMATCHED`; the test fails if any of them starts
matching (stale allowlist) or is missing from the corpus. Syntax errors of other
shapes keep Go's `toml: line N: ` envelope with the `toml_edit` message text.
Known disagreements, each observed from the real Go binary:

- **Dotted-key / header conflicts** (Go accepts, Rust rejects as a duplicate or
  extension error): `a.b = 1` then `a = 2`; `a.b.c = 1\n[a.b]\nd = 2`;
  `a.b = 1\n[a]\nc = 2`; `a = {x=1}\na.y = 2`. In these cases native
  `setup --fix`/`doctor --fix` stop with a config error where Go proceeds.
- **Newline inside an inline-table value** (`a = {x=\n1}`): Go rejects, Rust
  accepts (TOML 1.1).
- **Leap second** `a = 1979-05-27T00:00:60Z`: Go rejects with
  `invalid datetime: "1979-05-27T00:00:60Z"`, Rust accepts it.
- **Recursion limit**: `toml_edit` rejects more than 80 nested inline tables
  (`a = {b={b=...1...}}`) or 80 nested arrays with
  `toml: line 1: cannot recurse further; max recursion depth met`; Go accepted
  every depth tried (up to 10000). The limit is pinned by
  `recursion_limit_is_a_documented_residual`.
