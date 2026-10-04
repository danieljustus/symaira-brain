`run.sh OUTPUT_JSON` builds the complete frozen production Go CLI from
`dcddcef0df5789123c7c9a7ebe6e01f10e941f2c`, builds the candidate Rust CLI,
and compares real `doctor --fix` subprocesses with an absent Go fallback.
It replays 282 cases on Linux/macOS and 269 on Windows (Unix signals/permissions
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
cores are selected, failed downloads continue, and invalid full configuration
continues through the existing Go loader boundary without creating the managed
binary directory. Full configuration-failure diagnostics and source/module
build lifecycle remain open in #765. The decision and reasons are recorded in
`docs/adr/2026-10-03-native-doctor-repair.md`.
