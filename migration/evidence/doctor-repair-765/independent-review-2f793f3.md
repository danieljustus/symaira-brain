# Independent full-layer review: native Doctor repair #765

Disposition: **APPROVE the scoped native managed-release Doctor repair and shared managed decoder change**, subject to fresh required native Linux/macOS/Windows CI on the published/integrated head. No actionable findings identified. This is not approval to close #765 or to bypass merge protection; full typed configuration-failure diagnostics and setup source/module build lifecycle remain Go-owned and explicitly open.

## Immutable artifacts and independence

- Base: `e8c7f7990ab61967db0a3cbadaf6e485b4a33d99`.
- Reviewed/tested clean source: `2f793f3cb78c4a3731dfe0d06a4fb222c6ccc2eb`.
- Final clean evidence checkpoint inspected: `922d616143d3815dd61de329eaac3e5bf03c1f06`. Verified its only delta from the source checkpoint is seven new `migration/evidence/doctor-repair-765/linux-*` proof files; production source is identical.
- Frozen production Go oracle: `dcddcef0df5789123c7c9a7ebe6e01f10e941f2c`, freshly built from a detached immutable worktree with Go 1.26.7/CGO disabled, never rewritten.
- Reviewer did not author the candidate, edit its files, publish it, or delegate the review. All new review files are outside the candidate under `/tmp`. Existing worktree-owned target was reused only for this immutable code variant; lifecycle execution used the subreaper wrapper and umask 022.

## Inspected layers and conclusions

The Doctor dispatch now takes normal enabled repair flags through native Rust only when the existing full-config eligibility check and shared optional-core loader succeed. The gate performs no version probe or managed-install write. Invalid configuration still reaches the existing Go diagnostic boundary; the native test confirms original argument forwarding and absence of managed directory creation. `--force-release` is parsed as an explicit Go-compatible boolean and threaded to repair; default/false forms preserve origin protection. Stop rules, legacy single-dash forms, ignored JSON on the repair presentation and flag spellings are included in actual process comparisons. Existing normal Doctor presentation and MCP transport routing are retained.

Repair selects the same mandatory/configured optional cores, honors platform support, probes once per attempted core, skips already-correct versions before checking provenance, preserves source builds and corrupt/unreadable provenance unless force-release was explicit, and continues subsequent cores after a failure. Completion counts and nonzero aggregate failure are reproduced. Receiver commit values retain Go log quoting. The installer remains `allow_unsigned=false`: checksum, pinned identity/issuer, signature/certificate association, verified extraction, atomic installation and release provenance remain shared mandatory stages. The local publisher fixture is explicitly a synthetic invocation verifier, not a production signing authority.

Version and provenance decoding now share `symbrain-managed` ownership. The reviewed iterative syntax scanner retains byte-level Go errors and the 10,000-container boundary. Raw ranges preserve field order, duplicates, huge ignored numbers and literal time tokens. Known string fields reproduce null retention, case/Unicode folding, invalid UTF-8 replacement per byte, and unpaired surrogate handling. The first ordinary type error remains saved, while `time.Time` custom decode errors retain their actual precedence. The timestamp parser validates Go's accepted RFC3339 fallback forms and preserves byte quotes for errors. Supplemental independent process probes reached either side of the real depth boundary and combined duplicate/type/time errors; every observable matched Go. Publishing the pure Core byte-quote helper introduces no Guard, gateway, authorization or credential coupling, and no registry/Git dependency pin changed.

The Unix probe deadline now uses immediate owned-group KILL and reaps the child; the managed lifecycle test independently passed its descendant cleanup check. Actual timeout, signal and nonexecutable process cases matched the frozen Go diagnostics. Windows preserves original-path existence before PATHEXT resolution, and conditional status code formatting handles large unsigned exits. Windows/macOS runtime behavior still requires their real CI jobs; Linux checks do not establish those platform results.

The harness uses isolated HOME/XDG/project roots, absent Go fallback, fixture-only PATH and a local release server. It compares complete stdout/exit/files/modes, exact event order/attributes within each core, and the full completion tail. Only validated in-window clock fields, verified newly created UTC release sidecar time and owned root paths are normalized. Between-core log order is explicitly ignored because actual Go iterates a map; probe-call files remain exact. Controls execute the actual Rust CLI and mutate one observable, and assert complete replay plus exactly the intended failure. The three CI jobs add a named native process gate and retain all four receipts for 14 days even on failure. Existing gates and action pins are retained. ADR and DOC-002 accurately limit acceptance; earlier failing receipts remain intact.

Inherited broader file/network/output resource limits outside this patch are not established by this review. This review checks changed behavior and its direct consumers; it does not assert repository-wide hardening or full migration completion.

## Independent executions and evidence

1. Fresh `scripts/doctor-repair-oracle/run.sh /tmp/symaira-doctor765-independent-process.json`: **255/255 complete real Go/Rust case pairs match**, zero failures, clean source checkpoint. All three actual-process controls each produce 0/1 matches and exactly their expected failure (`exit`, `stdout_base64`, `logs`). Complete raw process observations and persistence remain in the receipts.
2. Fresh original `scripts/setup-repair-oracle/run.sh /tmp/symaira-doctor765-independent-setup-process.json`: **151/151 case pairs match**, 302 complete individual observations, exit 0. This independently covers the shared decoder's Setup consumer.
3. Supplemental independent actual process replay: **14/14 match**, receipt `/tmp/symaira-doctor765-independent-extra-process.json`. Includes known/unknown version and provenance fields at nested depths 9998/9999/10000/10001 (outer object included in the limit), mixed quote/control/Unicode fields, saved string-type errors before null/duplicate bad time fields, combined accepted one-digit hour/+24:60 timezone, calendar-versus-trailing-error precedence, and surrogate adjacency. Runner source retained at `/tmp/doctor765-independent-extra.py`; candidate untouched.
4. `cargo test --locked --all-features -p symbrain-managed`: **18 passed**, zero failed/ignored, five complete summaries including zero-test targets. `cargo test --locked --all-features -p symbrain-cli --test doctor_fix_native`: **1 passed**, zero failed/ignored, one complete summary. Logs `/tmp/symaira-doctor765-independent-managed-tests.log` and `/tmp/symaira-doctor765-independent-fix-tests.log`. These tests actually ran independently; author suite counts are not substituted for them.
5. Independent whole-workspace format check, actionlint, oracle shell syntax and whitespace checks passed. The author's source-bound strict Clippy receipt was inspected rather than unnecessarily repeating the same broad suite.

Verified all **31 candidate source/input hashes** (including every changed non-evidence file), **108 immutable Go source/module hashes**, and actual native binary hash against the fresh independent receipt. Compared those source hashes and binary to the retained clean-source author receipt: they agree. The original Setup replay uses the same immutable Go source. Independently verified the hashes of all retained validation log strings and counted the author's full suite as **396 passed, 0 failed/ignored, 37 complete parent summaries**. That is author evidence, separately identified from the 19 tests executed by this reviewer. The 14 supplemental cases ran at clean final evidence checkpoint, whose production byte identity with the reviewed source checkpoint was checked.

Actual native binary SHA-256: `1965814de299b61960140a2f781e938db869a14665cf51b857901cfbe9ff33f2`.
Fresh frozen Go binary SHA-256: `f0d05cc3a1070f3572ef825dfea9836d85e0569b302264ce1fd7243b7ccd610b`.

## Inspected changed files

- `.github/workflows/ci.yml`
- `Cargo.lock`
- `docs/adr/2026-10-03-native-doctor-repair.md`
- `migration/contract-matrix.csv`
- `rust/symbrain-cli/src/cli_tests.rs`
- `rust/symbrain-cli/src/doctor_cli.rs`
- `rust/symbrain-cli/src/doctor_fix.rs`
- `rust/symbrain-cli/src/doctor_fix_log.rs`
- `rust/symbrain-cli/src/setup_cli.rs`
- `rust/symbrain-cli/src/setup_config.rs`
- `rust/symbrain-cli/tests/doctor_fix_native.rs`
- `rust/symbrain-core/src/config/format.rs`
- `rust/symbrain-core/src/config/mod.rs`
- `rust/symbrain-managed/Cargo.toml`
- `rust/symbrain-managed/src/json_record.rs`
- `rust/symbrain-managed/src/json_syntax.rs`
- `rust/symbrain-managed/src/lib.rs`
- `rust/symbrain-managed/src/process_status.rs`
- `rust/symbrain-managed/src/provenance.rs`
- `rust/symbrain-managed/src/provenance_json.rs`
- `rust/symbrain-managed/src/provenance_time.rs`
- `rust/symbrain-managed/src/version_probe.rs`
- `rust/symbrain-managed/tests/install_tests.rs`
- `scripts/doctor-repair-oracle/README.md`
- `scripts/doctor-repair-oracle/cases.py`
- `scripts/doctor-repair-oracle/probe_fixture.go.txt`
- `scripts/doctor-repair-oracle/replay.py`
- `scripts/doctor-repair-oracle/run.sh`

All original added failure receipts were inspected as failure evidence (not acceptance); final seven evidence-only files, their source/binary links, log hashes and counts were inspected separately. Direct surrounding consumers reviewed include managed installer/manifest, full-config eligibility, CLI dispatch, frozen Go repair/version/provenance/Doctor handlers, original Setup runner, isolated release-fixture environment, raw-path normalization and retained lifecycle tests.
