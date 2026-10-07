# Independent full-layer review: native Source/Module Setup #765

Disposition: **REQUEST CHANGES** at the immutable source below. Full changed-layer review is complete. Three P2 findings and one P3 finding remain; baseline acceptance suites passing do not override the independently reproduced additional failures. No candidate edits, push, PR, merge or issue closure were performed.

## Immutable inputs and verification binding

- Worktree: `/workspace/symaira-setup765-source`.
- Base: `922d616143d3815dd61de329eaac3e5bf03c1f06`.
- Clean production/harness/CI/docs source: `2454c461703fd2f79d727da76242d0f8be7ed7ef`.
- Final evidence-only clean HEAD: `5a57f36e55d4237da50163243667b4850c6a347f`.
- Actual frozen Go: `dcddcef0df5789123c7c9a7ebe6e01f10e941f2c`, SDK Go 1.26.7 Linux/amd64.
- Actual candidate CLI SHA256: `3a2a0bc07633848c901e9710befb857147703aff50aa9a30fb8ef28bd0e76f2a`.

Verified clean HEAD and that source-to-final changes contain evidence only. Independently checked all source hashes in fresh Source (116 candidate / 4 Go), Doctor (45 candidate / 108 Go) and Setup (10 candidate / 108 Go) receipts, their equality with retained source-commit manifests and actual candidate binary. All ten author artifact hashes were independently verified. The separate review receipt hashes all 39 files changed against the base, pinned SDK FlagSet/Windows lookup sources, inspected JobObject dependency sources, and reviewer proof inputs/logs: `/tmp/symaira-setup765-source-independent-review-receipt.json`.

## P2: reject malformed flag prefixes before installing anything

Location: `rust/symbrain-cli/src/setup_args.rs:29`, `trim_start_matches('-')`.

The source cutover exposes the permissive native parser to invocations previously handled by Go. After the existing one-time normalization, it removes arbitrary leading dashes. Frozen Go FlagSet permits its precise one/two-prefix form and rejects a remaining leading dash or `=` in the name.

Actual reproduced invocation:

```text
symbrain setup ----from-source ROOT --modules browse --json
```

Frozen Go returns exit 2, `bad flag syntax: ---from-source`, no tool invocation or managed installation. Candidate returns exit 0, invokes Git/compiler, installs the module binary and provenance sidecar, and reports success. Equivalent four-prefix `----modules` and `----json` triggers also install despite Go rejection. `--=bad` has the wrong diagnostic; permitted `---json` after normalization remains correctly accepted and was independently included as a passing control.

Full actual-process stdout/stderr/exit and before/after filesystem evidence: `/tmp/symaira-setup765-source-independent-flag-syntax-extra.json`; exact reviewer runner/log alongside it. Three cases mismatch exit, both output streams and filesystem; one diagnostic-only case mismatches stderr; the valid-prefix control matches.

Required correction: use the shared byte-preserving Go FlagSet contract (or equivalent exact grammar) rather than trimming arbitrary prefixes. Require malformed source invocations to fail before compiler or install side effects. Retain actual negative-prefix cases and the allowed normalization control in the process gate.

## P2: retain raw Unix option/path bytes through diagnostics and JSON

Primary locations: `rust/symbrain-cli/src/setup_args.rs:65`, `raw_value.to_string_lossy()`; `rust/symbrain-cli/src/setup_source.rs:168`, `root.display().to_string()`. Related unknown/bool flag formatting and source-root error rendering also discard bytes.

Actual Go/native observations at this immutable source confirm six distinct regressions:

- Separate and inline `--modules` values containing bytes `bad ff e2 82`: Go diagnoses `unknown module "bad\xff\xe2\x82"`; Rust diagnoses a lossy string with two U+FFFD characters.
- A nonexistent source path ending in `missing ff e2 82`: Go stderr retains the three raw bytes; Rust substitutes two U+FFFD characters.
- Unknown and invalid-boolean raw flags after `--from-source ROOT`: diagnostics no longer preserve the raw Go FlagSet bytes/quoting.
- An existing valid Unix source directory ending in `owned ff e2 82` builds and installs in both implementations with identical owned filesystem effects. Go JSON root encodes each invalid byte separately as `\ufffd\ufffd\ufffd`; Rust emits two literal U+FFFD characters. This changes the reported identity of a real supported Unix directory, not just a malformed request.

Evidence: `/tmp/symaira-setup765-source-independent-extra.json`, with complete raw and normalized observations; `/tmp/symaira-setup765-source-independent-extra.py` contains exact byte inputs and source-root construction. Five cases mismatch stderr; the existing-root case mismatches stdout. The production compiler argv itself retains the raw source path; the defect is early option conversion and output representation.

Required correction: carry module values and path identity as raw OS bytes until parsing/comparison, use existing Go raw-byte quoting for errors, and use Go-compatible per-invalid-byte JSON string encoding where required. Preserve existing valid U+FFFD distinctly from malformed bytes. Add separate/inline values, raw unknown/bool flags, missing paths and successful raw-path JSON/human probes to the actual-process corpus rather than weakening byte comparison.

## P2: honor Windows implicit CWD lookup when explicitly opted in

Location: `rust/symbrain-cli/src/setup_source_process.rs:57`, implicit candidate guarded by `!allow_relative`.

This is a **static finding**, not a claim of native Windows execution. The pinned SDK `/workspace/toolchains/go1.26.7/src/os/exec/lp_windows.go:155`–165 explicitly checks the current directory whenever `NoDefaultCurrentDirectoryInExePath` is absent. With `GODEBUG=execerrdot=0`, it immediately returns the implicit CWD executable before searching PATH. Candidate instead skips the implicit check when `allow_relative` is true, then searches PATH.

Concrete trigger: Windows CWD contains `git.exe`, a distinct PATH directory contains another `git.exe`, `NoDefaultCurrentDirectoryInExePath` is absent and `GODEBUG=execerrdot=0`. Go uses the opted-in CWD Git; native uses PATH Git. With only the CWD tool present, native reports missing. The same resolver controls compiler selection, so this can change receiver commit or builder identity. Existing static typechecks do not validate this behavior; committed CWD cases omit this opt-in combination.

Required correction: retain immediate implicit-CWD selection for explicit opt-in, preserve the existing ErrDot/same-file/default-directory-disabled cases for ordinary lookup, and add native Windows process cases for distinct CWD/PATH executables plus CWD-only and explicitly disabled CWD variants. Exact native proof remains required on Windows; do not relabel Linux or signature stubs as that proof.

## P3: preserve structured per-module failure for unusable temporary roots

Location: `rust/symbrain-cli/src/setup_source_process.rs:126`, new metadata capture tempfile allocation.

Actual owned cases with TMPDIR/TMP/TEMP set to a nonexistent path or a regular file both return exit 1. Go still resolves Git, then returns the JSON source report containing a per-module stage-directory failure. Rust fails capture allocation before invoking Git, returns empty stdout and a top-level plain stderr error with a random `.tmp` path. Thus the new capture mechanism changes failure ordering and removes structured output from a request handled as a report by the previous implementation.

Evidence: `/tmp/symaira-setup765-source-independent-tmp-extra.json`, with exact output, tool trace and filesystem observations for both cases; runner/log alongside it. Neither implementation installs a payload. This is lower priority than the unintended installations and identity/selection findings above.

Required correction: avoid making metadata capture introduce a new earlier fatal dependency on the requested worker temp root, or explicitly preserve Go's stage/error-report behavior at this boundary. Keep the owned missing/file temp-root observations in the process proof; do not normalize away the added random capture error or dropped JSON report.

## Independent execution observations

All lifecycle runs used `/tmp/symaira-subreaper.py`, umask 022, explicit Go SDK PATH, Rust toolchain environment, disposable HOME/XDG roots, existing candidate-owned target, incremental disabled and dev/test debug disabled. No operator credentials/state, production services, real Swift worker builds, hardware permission calls or signing were used.

- Fresh Source gate: **69/69** complete actual Go/native CLI observations and both genuine native-process controls. Actual owned successful-builder descendant cleanup passed. Receipt `/tmp/symaira-setup765-source-independent-process.json`; controls append `.wrong-exit.json` and `.wrong-source.json`; full launcher log beside it.
- Fresh Doctor regression: **255/255**, with all three genuine controls. Receipt `/tmp/symaira-setup765-source-independent-doctor.json` and corresponding control JSON/logs.
- Fresh Setup regression: **151/151** stdout/stderr/exit/full filesystem comparisons. Receipt `/tmp/symaira-setup765-source-independent-setup.json` and launcher log.
- Independent additional probes: six raw-byte cases expose findings above; five flag-grammar cases include four mismatches and one matching valid-prefix control; two unusable-temp-root cases expose the P3; seven case-sensitive/string/zero/config merge probes all match. Config evidence `/tmp/symaira-setup765-source-independent-config-extra.json`.
- Two actual native cancellation probes hold a source builder with a live owned descendant, send SIGINT or SIGTERM, and require exit 1, `source build cancelled`, prompt return and no live descendant. Both passed; recorded inside `...-independent-extra.json`. This is Unix proof, not Windows signal or hard-kill proof.
- Independently executed CLI library **120** parents, Managed library **6** parents and source-local publication **1** parent: **127 passed, 0 failed, 0 ignored**, three complete summaries. Includes new capture-order/deadline tests and atomic publication/sidecar failure behavior.
- Strict all-target CLI/Managed Clippy, workspace format, workflow actionlint and Bash syntax passed independently. Logs: `/tmp/symaira-setup765-source-independent-tests.log`, `...-clippy.log`, `...-validation.log`.

Author's separately retained validation records 398 passed / zero failed or ignored / 38 summaries. These were inspected as author evidence, not claimed as an independent duplicate run. Fresh independent process failures above remain acceptance blockers despite that ordinary suite passing.

## Full changed-layer inspection

Inspected all 28 source-commit changed files and final evidence additions against the base, including:

- CLI: `Cargo.toml`, `lib.rs`, removed obsolete fallback-only test, `setup_args.rs`, `setup_cli.rs`, `setup_config.rs`, and all new `setup_source.rs`, `setup_source_build.rs`, `setup_source_layout.rs`, `setup_source_process.rs` production behavior. Checked original frozen CLI/source implementation, default/explicit module ordering, home resolution, source/root/conflict/config fallback ordering, human/JSON/error continuation and actual worker argv/cwd/environment.
- Managed: archive atomic publication, exports, new `local_install.rs`, shared process status, provenance writer/reader/hash extraction, version probe adapter and source-local integration test. Binary publication before sidecar preserves Go's partial effect on sidecar failure. Installed version is read from the managed payload, while historical empty sidecar version remains unchanged. Existing release repair/source protection is exercised by the fresh regressions.
- Process ownership: Unix process groups, metadata/build deadlines, cancellation flags and unregister paths, RAII drop on success/error/budget/cancel, merged capture offset/order, captured output and staging cleanup. The extra SIGINT/SIGTERM probes and existing successful-return descendant probe validate actual Linux behavior. Installed-version probes remain the existing Managed implementation; this review does not expand its Windows/hard-kill scope.
- Pinned `process-wrap=10.0.1` source and enabled features: std + JobObject only, target Windows. The implementation adds CREATE_SUSPENDED before spawn, assigns the Job before resuming, kills the child on setup/resume failure, exposes Job-wide termination and waits/reaps through the owned wrapper. CLI drop explicitly kills the Job; dependency JobObject does not enable kill-on-close by itself, so hard termination of the CLI is correctly not asserted as proven cleanup. Inspected `same-file=1.0.6` handle usage and Windows explicit Lstat-style reparse identity. New lockfile dependencies and Windows resolution changes require fresh native compilation, not only narrow signature stubs.
- macOS external layout: required mounted volume, absolute/contained base/runtime paths, nearest-existing-ancestor resolution, pre/post creation checks, restrictive cache creation, worker-only environment, Swift scratch/cache argv and stable module ownership. CI bypass and non-macOS empty layout preserve intended contracts. Linux did not execute this cfg branch or native Swift/hardware behavior.
- Harness: every Source runner/README/replay/native fixture file. Real full frozen Go CLI, actual native instrumentation executables and a separately actual-Go-compiled dependency-free worker; complete output/exit and before/after FS types/modes/hashes; payload-bound source sidecar and in-window UTC timestamps; only approved owned-root/staging/time normalization. Controls really execute the native CLI and alter exactly exit or source observation. New probes used reviewer-owned files outside the candidate and unchanged production comparators.
- CI/docs/evidence: all three named native OS jobs, always-uploaded positive/control JSON receipts, ADR, contract matrix, retained initial/expanded/nonexecutable failures, explicitly non-acceptance dirty exploratory replay, final validation and raw logs. Source fixtures are instrumentation rather than real product/Swift acceptance. No frozen Go production or fixtures were edited; all touched production files remain below 400 lines.

Brain owns this optional-module build/install orchestration; it introduces no Guard approval/risk/grant authority and no Brain MCP exposure. Explicit source requests write only the managed directory, preserve foreign PATH/Homebrew/unrelated managed ownership and stamp installed payload identity. Go Browse and Swift Operate/Scope builders remain historical transitional workers; this is not a Go-free worker cutover or full #765/#783 completion. Invalid full typed config diagnostics remain on Go. The inherited Doctor receipt's older broad source-lifecycle wording is superseded only for the new explicitly documented orchestration scope.

## Required disposition

Fix the four findings on a new immutable source, retain these original actual failures and original candidate, expand rather than weaken the process gate, and obtain an independent full-layer re-review plus fresh required native Linux/macOS/Windows CI. Windows full-CLI cross-check is recorded red for missing MinGW; narrow Windows/Darwin signature-stub checks are static only and cannot satisfy native runtime acceptance. Native Swift worker/hardware/signing/full Rust-Browse cutover and full invalid-config loader acceptance remain open issue scope.

All reviewer processes have finished. The existing target is released back to the author/root for the focused follow-up; no duplicate target was created or removed in this review.
