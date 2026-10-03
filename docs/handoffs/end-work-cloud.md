# Current continuation checkpoint: skills preflight (2026-10-03)

This is the active entry point. The earlier 2026-09-30 record is preserved below as historical context, not a current branch or merge instruction.

## Goal, source and status

Continue the unfinished native Rust skills preflight correction without this computer, local reports, chat history or installed agent skills.

- Repository: `danieljustus/symaira-brain`, public, non-fork.
- Active branch: `agent/skills-preflight-793`; draft PR [#794](https://github.com/danieljustus/symaira-brain/pull/794), associated issue [#793](https://github.com/danieljustus/symaira-brain/issues/793).
- Immutable **base code commit** before this documentation/evidence checkpoint: `3a85990fa3e91c403c5058d386af068b443cfe41`.
- Integration base and unchanged frozen Go source for the flag comparison: `dcddcef0df5789123c7c9a7ebe6e01f10e941f2c`.
- State: **saved, not complete or approved for merge**. Keep #794 draft with auto-merge off. Publication is not a release or a claim that the complete issue queue is finished.

The final documentation/evidence HEAD is supplied by the publication report and verified GitHub ref, rather than recursively embedded in this commit. Commands run from the repository root. Never choose one of the historical preservation branches below as the implementation base.

## Completed integration in this work period

These records were re-read from GitHub at closeout. The reviewed pre-merge revisions remain available through their PRs; the integrated code is in the main ancestry of the active branch.

| Issue | PR | Integrated commit | Disposition |
|---|---|---|---|
| #559, move advisory checks off PR critical path | #784 | `48ce970926e81f6e01731054368591f95d8f7390` | merged; issue closed |
| #406, shared mobile tool registry | #785 | `4a522ff5f5e849cadb52c8c485179c5d943140fa` | merged; issue closed |
| #597, typed Scope discovery diagnostics | #786 | `54d59382a5bb91cec5cd02f176a596ec90b98cd8` | merged; issue closed |
| #594, Doctor module provenance | #787 | `dfd55866a80e2c45509abf6f6bc17b897707d659` | merged; issue closed |
| #598, measured protocol-aware MCP health | #788 | `bde974c48a2ec7f6aca80eb8597fb12b9e0cdb97` | merged; issue closed |
| #457, cross-process install/registry locking | #789 | `7a37e601c34195e30d9297b737573d58eabea55c` | merged; issue closed |
| #476, bounded Rust input slice | #791 | `c6b49ce4fb9c5293a06af37323a99d003dc576e5` | merged; broader issue deliberately **open** |
| #490, confined in-root directory symlinks | #792 | `dcddcef0df5789123c7c9a7ebe6e01f10e941f2c` | merged; issue closed |

#476 shares a typed aggregate I/O budget across repeat reads, clones, bundles and inventory/runner callers. #490 retains directory capabilities and rejects escapes/replacement races, including trusted-root/ancestor aliases and native Windows extended-path/rename semantics. Their changes are Rust-only; the Go implementation, fixtures and oracle remain frozen. #594 does not change setup repair or fallback lifecycle. #598 retains legacy health fields with only the documented HAR-007 exceptions. Preserve these contracts.

## What #794 implements and the remaining defect

The seven-file code layer consists of `MIGRATION-RUST.md`, `migration/contract-matrix.csv`, `rust/symbrain-cli/src/skills_cli.rs`, `rust/symbrain-cli/src/skills_sync_flags.rs`, `rust/symbrain-cli/tests/skills_preflight_native.rs`, `rust/symbrain-skills/src/install/marker.rs`, and `rust/symbrain-skills/src/install/status_compare.rs`.

It terminates missing-value flag parsing before filesystem work; uses the shared bounded, no-follow marker reader with nonblocking Unix final opens; rejects unsafe marker inputs before fallback; keeps ordinary malformed/future-schema fallback; uses an existing-only directory opener so a concurrent removal cannot recreate a status path; and retains raw argument bytes and whole-vector Go normalization.

The latest independent full-layer review of `3a85990fa3e91c403c5058d386af068b443cfe41` has the literal verdict **`changes_requested`**, with one remaining low-severity parity finding. At `skills_sync_flags.rs:100`, `raw_target.to_str().map(str::trim)` trims only valid UTF-8. On Unix, inline and separated target bytes `20 62 61 64 ff 20` therefore quote `" bad\xff "` instead of the Go result `"bad\xff"`.

This was reproduced at closeout against the frozen production Go binary, in fresh disposable HOME/XDG roots. Both programs return exit 2; their stderr differs. See [the actual two-case mismatch](../../migration/evidence/skills-preflight-793/known-padded-target-mismatch.json) and [the unaltered latest review verdict](../../migration/evidence/skills-preflight-793/review-code-head.json). The thirty earlier flag cases pass exactly but **do not prove full flag parity**.

Next bounded code task:

1. Trace the existing Go-compatible whitespace/byte helpers and `resolveTargets` callers. Reuse an existing helper if it correctly handles Go `strings.TrimSpace` semantics without losing invalid bytes; otherwise add the smallest safe implementation.
2. Trim **target** bytes before validation/quoting. Keep invalid **scope** diagnostics untrimmed, as Go does. Do not reintroduce lossy conversion, nonterminating parsing, filesystem writes or unsafe fallback.
3. Extend the existing native raw-value regression with both padded non-UTF-8 target forms, including relevant Unicode whitespace boundaries. Compare actual stdout/stderr/exit with the unchanged frozen Go binary. Do not overwrite recorded failing evidence and call it a success; add a new verified revision record.
4. Rerun format, three affected packages, strict Clippy, and a fresh independent **full-layer** review at the new code HEAD. Retain literal reviewer verdicts; exit 0 from the review process is not approval.
5. Require fresh exact-head acceptance CI and normal repository rules before readiness/merge. Close #793 only after verified delivery of its acceptance; never close #764 or #476 from this narrow fix.

## Open boundaries and blockers

- [#764](https://github.com/danieljustus/symaira-brain/issues/764): all remaining native skills CLI/MCP shapes and fallback removal are a separate larger task. #794 does not satisfy that acceptance.
- [#476](https://github.com/danieljustus/symaira-brain/issues/476): the broader Go/Rust/fallback scope remains open. [#790](https://github.com/danieljustus/symaira-brain/issues/790) records a real macOS external-volume oracle failure reopening `/dev/fd/<fd>` before Rust runs. Do not change Go/fixtures or claim a complete differential from a setup failure.
- [#621](https://github.com/danieljustus/symaira-brain/issues/621): unresolved library/render drift and symlink-mode semantics. Prefer the existing read-only status-drift alternative when accepted; do not invent a pull/force design or weaken root confinement without the necessary product decision.
- [#649](https://github.com/danieljustus/symaira-brain/issues/649) must remain open until its repair is shipped. [#783](https://github.com/danieljustus/symaira-brain/issues/783) is gated by #781 Go-independent contracts and #782 stable cutover/observation. No Go removal, release or observation claim is authorized here.
- Real iPhone/TestFlight acceptance (#409), and the replacement/deprecation decision for the Browse compat transport (#775), remain outside this bounded continuation.

## Evidence, exact branches and publication inventory

The public evidence directory is [migration/evidence/skills-preflight-793](../../migration/evidence/skills-preflight-793). It contains real flag comparisons, the literal review, two known failing comparisons, code-head CI, preservation-source hashes and an [artifact inventory](../../migration/evidence/skills-preflight-793/inventory.json). The inventory classifies every top-level external report artifact and the rebuilt oracle subtree. It is not a raw export of private reports, chat, memory, credential settings or real user state.

The following separately pushed refs preserve superseded work, **not accepted implementation candidates**. Their source bytes were compared with the original archives/worker commit before publication; [preserved-checkpoints.json](../../migration/evidence/skills-preflight-793/preserved-checkpoints.json) records all source hashes. Originals were retained without modification. No test/approval is asserted for these historical variants, and no PR was opened to propose merging them.

| Preservation branch | Exact HEAD | Meaning |
|---|---|---|
| `handoff/20261003-skills-457-partial` | `cc12e8aeb9fbfcf73d7db9984c4d0dc92f494298` | eight originally uncommitted files, superseded by #789 |
| `handoff/20261003-skills-457-worker` | `4e368f94b66ace1c0d677436e9129b09a5fe203d` | original worker commit, superseded by #789 |
| `handoff/20261003-skills-476-interrupted` | `2703b37c757234f0172b641a2d8b0bd4590c8313` | eight interrupted source files, superseded by #791 |

No stash exists. Older unrelated local codex/migration/release refs and old worktrees are excluded and untouched, not required inputs. Clean session checkouts at merged main ancestors need no separate source branch. Generated binaries, Cargo/Go/Swift caches, archive containers, coverage, Finder metadata and raw build logs are rebuilt or excluded; no ignored directory is force-added. Temporary one-off check/watch scripts are replaced by existing tracked workflows and the reproduction recipe below. The full oracle source copy/tar/binary is reconstructed from the immutable Git ref, not uploaded. The session's owned review/watch processes have exited; no running local worker is needed for continuation.

## Setup and portable scoped commands

Local observation at closeout: macOS 27.0.1 arm64, Git 2.54.0, gh 2.102.0, Rust/Cargo 1.98.0, Go 1.27.1, Python 3.14.2, Xcode 27.0, XcodeGen 2.45.4. Use `rust-toolchain.toml` (1.98.0 plus rustfmt/Clippy), `Cargo.lock`, `go.mod` (Go 1.26.7) and `go.sum`, not unpinned latest dependencies.

Required tools: Git, Bash, Python 3, Rustup with the pinned Rust toolchain, and a C build toolchain for bundled SQLite/TLS. Install missing Rust components with `rustup toolchain install 1.98.0 --profile minimal --component rustfmt --component clippy` if authorized. This is a setup instruction, not a claim that an install was needed at closeout. Go is needed only for unchanged oracle/differential checks; `GOTOOLCHAIN=go1.26.7` pins the replay. GitHub, crates.io and the Go module proxy must be reachable.

Clone directly from GitHub and verify the exact final HEAD supplied by the publication report:

```sh
 git clone --branch agent/skills-preflight-793 https://github.com/danieljustus/symaira-brain.git
 cd symaira-brain
 git rev-parse HEAD
 git ls-remote --exit-code origin refs/heads/agent/skills-preflight-793
 git status --porcelain=v1 -uall
```

For a disposable CI-shaped checkout, the repository's supported `CI=1` storage path removes any dependency on this computer's mounted storage. This does not simulate cloud execution or product runtime. Use a fresh Cargo home and output directory when checking repository completeness, not copied worktree sources, stashes, generated fixtures or source-path tricks:

```sh
export CI=1
export RUNNER_TEMP="$PWD/target/runtime"
export CARGO_HOME="$PWD/target/cargo-home"
export CARGO_TARGET_DIR="$PWD/target"
export CARGO_BUILD_JOBS=2
export CARGO_PROFILE_DEV_DEBUG=0
export CARGO_PROFILE_TEST_DEBUG=0
export GIT_TERMINAL_PROMPT=0
export CARGO_NET_GIT_FETCH_WITH_CLI=true
mkdir -p "$RUNNER_TEMP" "$CARGO_HOME"
bash scripts/run-external-env.sh cargo fetch --locked
bash scripts/run-external-env.sh cargo fmt --all -- --check
bash scripts/run-external-env.sh cargo test -p symbrain-skills -p symbrain-cli -p symbrain-gateway --locked
bash scripts/run-external-env.sh cargo clippy -p symbrain-skills -p symbrain-cli -p symbrain-gateway --all-targets --locked -- -D warnings
bash scripts/run-external-env.sh cargo build -p symbrain-cli --locked
bash scripts/run-external-env.sh target/debug/symbrain version
```

`version` is a safe start smoke, not permission to start MCP/HTTP or access production devices, provider accounts or credential stores. Native Windows uses the `.exe` binary; raw-byte/FIFO cases are Unix-only while Windows runtime must pass its own native tests.

The workspace pins public `symaira-corekit` commit `04d1411adb57aa602b992509121011aa7666ff1a`. Go pins CoreKit v0.17.0; the Swift lockfile pins AppKit through `SymBrain.xcodeproj/project.xcworkspace/xcshareddata/swiftpm/Package.resolved`. No submodules or Git LFS pointers are needed. Cargo fetching validates actual pinned Git/registry inputs; no provider/API credentials are required for these mock/unit checks. GUI builds require full Xcode and Apple platform SDKs, and signing/Keychain/Touch ID/real iPhone behavior needs native hardware/permissions. They are not proved by this scoped checkout test.

### Reconstruct the frozen Go flag oracle

Use the full immutable source tree, never copy only cmd/ or modify production fixtures. The following commands leave source/configuration intact and write only generated target outputs:

```sh
mkdir -p target/go-source
 git archive dcddcef0df5789123c7c9a7ebe6e01f10e941f2c | tar -xf - -C target/go-source
 (cd target/go-source && GOTOOLCHAIN=go1.26.7 CGO_ENABLED=0 go build -mod=readonly -o ../symbrain-go ./cmd/symbrain)
```

On Unix, run this standard-library replay from the repository root. It checks all thirty recorded cases and verifies that the two padded-target comparisons still demonstrate the documented open defect. A mismatch in a recorded passing case is a new blocker, not permission to rewrite the evidence.

```python
# parity-replay
import json, os, subprocess, tempfile
from pathlib import Path
root = Path.cwd()
evidence = root / "migration/evidence/skills-preflight-793"
go = root / "target/symbrain-go"
rust = root / "target/debug/symbrain"

def compare(args):
    with tempfile.TemporaryDirectory(dir=root / "target/runtime") as home:
        env = {"HOME": home, "USERPROFILE": home, "XDG_CONFIG_HOME": home + "/config",
               "XDG_DATA_HOME": home + "/data", "XDG_CACHE_HOME": home + "/cache",
               "PATH": "/usr/bin:/bin", "LANG": "C.UTF-8", "TZ": "UTC"}
        outputs = []
        for binary in (go, rust):
            p = subprocess.run([os.fsencode(binary), b"skills", b"sync", b"--json", *args],
                               cwd=home, env=env, capture_output=True, timeout=4)
            outputs.append({"exit_code": p.returncode,
                            "stdout": p.stdout.decode(errors="backslashreplace"),
                            "stderr": p.stderr.decode(errors="backslashreplace")})
        return outputs

cases = json.loads((evidence / "parity-30-cases.json").read_text())["results"]
for case in cases:
    args = ([bytes.fromhex(x) for x in case["raw_args_hex"]] if case["raw_args_hex"]
            else [os.fsencode(x) for x in case["flags"]])
    actual_go, actual_rust = compare(args)
    assert actual_go == case["go"] and actual_rust == case["rust"] and actual_go == actual_rust
known = json.loads((evidence / "known-padded-target-mismatch.json").read_text())["cases"]
for case in known:
    actual_go, actual_rust = compare([bytes.fromhex(x) for x in case["raw_args_hex"]])
    assert actual_go == case["go"] and actual_rust == case["rust"] and actual_go != actual_rust
print(f"{len(cases)} recorded cases match; {len(known)} known padded-target mismatches reproduced")
```

## Actual verification and remaining acceptance

The three-package suite passed **422 top-level tests, 0 failed, 2 ignored**, both in the original code-head log and the fresh clone. The previously reported **435** incorrectly included filtered child test-harness runs launched by concurrency tests. The corrected count uses only complete parent-suite summaries with zero filtered tests; the literal historical reviewer artifact is preserved, not rewritten. Format, host strict Clippy, diff checks and skills-only Windows cross-Clippy passed. The latter is not native Windows proof. Thirty isolated actual Go/Rust flag probes matched stdout, stderr and exit codes byte-for-byte. At closeout two additional padded raw-target cases **failed parity as expected**, reproducing the remaining review defect.

[CI run 37078394459](https://github.com/danieljustus/symaira-brain/actions/runs/37078394459) was re-read for code head `3a85990fa3e91c403c5058d386af068b443cfe41`: all eleven acceptance checks succeeded (build-test, stdio-hygiene, govulncheck, gui, pin-check, Linux/macOS/Windows migration, and the three native init jobs). This does **not** override the reviewer finding. Documentation publication creates a new HEAD; no old-head CI or approval is substituted for a new code revision.

Fresh GitHub-checkout verification **passed locally on macOS** at published checkpoint `f8a6bb8a91630bf03b7c6c10c0fdbedec9d2ba79`, with an initially empty Cargo home and no copied source/configuration, stashes or local input overrides. The documented fetch, format check, three-package tests, strict Clippy, CLI build and version smoke all exited 0. The exact pinned CoreKit Git commit and locked registry dependencies were fetched successfully. The frozen source archive rebuilt with Go **1.26.7**, without Go/fixture edits. The verbatim documented Python replay exited 0: **30 recorded cases match; 2 known padded-target mismatches reproduced**. All three preservation refs were fetched into this fresh clone; all sixteen archived source files matched their recorded SHA-256 and size. The final recording changes only documentation/evidence, not the tested code.

See [verification.json](../../migration/evidence/skills-preflight-793/verification.json) for the normalized execution record. This proves source/input completeness and these scoped checks, not complete product acceptance. Target-cloud runtime, cloud permissions/secrets/network and native GUI/hardware beyond the recorded historical CI: **not checked**. No manual cloud-job dispatch, model request, release or paid provider is initiated by this handoff. Normal repository push/PR CI may run automatically.

The repository's normal acceptance gate remains mandatory. Before any merge, re-read the exact PR head, fully paginated relevant review/comment threads and current ruleset (last observed id `19398168`), obtain independent full-layer review, and verify all eleven relevant acceptance checks on the actual candidate revision. No admin bypass, weakened protections, force-push, invented native results, fixture rewrites or premature issue closure. Do not wait indefinitely for obsolete CI watchers; a watcher deadline is not a code failure. Use direct completed-job logs when needed, pinned to the relevant revision. No paid model/API fallback without separate explicit permission.

## Copyable continuation request

Work in `danieljustus/symaira-brain` on `agent/skills-preflight-793` at the exact final publication HEAD. Read `docs/handoffs/end-work-cloud.md`, verify the remote HEAD and run the portable setup/checks. First repair the byte-preserving Go whitespace trimming defect for padded non-UTF-8 targets in `skills_sync_flags.rs`, extend and actually compare the native regressions, then obtain fresh full-layer review and exact-head native three-OS CI for #794. Keep #794 draft and #793/#764/#476 open until their respective acceptance is proved. Preserve frozen Go/oracle inputs, status read-only behavior, no-follow confinement and normal merge gates. The three historical preservation refs are not merge candidates.

---

# Historical code continuation checkpoint: symaira-brain (2026-09-30)

> This record describes the `handoff/20260930-cloud` checkpoint at `f40974a274555015450de9a9a601d34411740807`, published on 2026-09-30. As of 2026-10-01, PRs #753, #754, and #755 have merged; current `main` at `8a56f124678432069850634e90f7c597ea07d747` is the integration base. The publication constraints and PR states below are historical. The recorded scoped checks are preserved as evidence and have not been expanded or rerun against the current integration base.

## Goal and immutable starting point

Continue the code and integration work from the published repository, without needing a local chat, private reports or installed agent skills.

- GitHub repository: `danieljustus/symaira-brain`.
- Branch: `handoff/20260930-cloud`.
- Base code commit before this document/checkpoint: `d7845e38cc390531a5a442137168a87b99b4eedc`.
- Working directory for every command below: the checked-out repository root.
- Publication does not authorize a merge, release, tag, destructive cleanup or paid service.
- Continuation draft PR: #756. Keep it draft until its code/acceptance gates are independently satisfied.

Preserve the code base of PR #753. PR #754 is a separate reviewed documentation candidate that must remain separately reachable. No automatic integration of either PR is authorized by publication.

## Requirements, decisions and next task

Independently review PRs #753 and #754 at their actual heads. They are separate candidates, not automatically combined. Check exact-head required CI and current protection before any explicitly authorized integration.

Keep products and their optional modules standalone. Preserve exact dependency pins, snake_case contracts, data integrity, authorization and MCP stdout discipline. Keep frozen fixture evidence and original Oracle ancestry unchanged until an explicit preservation design is accepted. Do not rewrite history, force-push, bypass branch protection, delete unique work, close unproven issues or reinterpret a passing subset as complete acceptance.

- Existing PR #754: OPEN, recorded code head `896143093756aebe83c330717dd56783d0c779ce`. Re-read the current PR before integration.
- Existing PR #753: OPEN, recorded code head `d7845e38cc390531a5a442137168a87b99b4eedc`. Re-read the current PR before integration.

## Setup and scoped verification

Clone the existing public repository, checkout `handoff/20260930-cloud`, verify its current remote HEAD, and read this file before making changes. Never substitute another branch or silently mix the alternatives.

```sh
git clone --branch handoff/20260930-cloud https://github.com/danieljustus/symaira-brain.git
cd symaira-brain
git rev-parse HEAD
git ls-remote --exit-code origin refs/heads/handoff/20260930-cloud
git status --porcelain=v1 -uall
```

Locally observed toolchains: Git 2.54.0, gh 2.102.0, Rust/Cargo 1.98.0, Go 1.27.1, Node 22.22.3, Ruby 2.6.10, Swift 6.4, regular Xcode. Rust repositories pin their toolchain in `rust-toolchain.toml`; honor the checked-in manifests. Go Oracle regeneration must use the exact Go version required by its own manifest/generator, not this observed machine version. Native Swift requires full Xcode. Package-manager caches are rebuildable, not required private inputs.

Scoped reproduction commands, not a claim of the complete product suite:

```sh
cargo test --locked -p symbrain-policy
```

Build command (not claimed executed unless listed in verification):

```sh
cargo build --locked --workspace
```

Start/help command (not executed for live services/devices):

```sh
cargo run --locked -p symbrain-cli -- --help
```

For Rust, optional resource limits are `CARGO_BUILD_JOBS=2`, `CARGO_PROFILE_TEST_DEBUG=0`, `CARGO_PROFILE_DEV_DEBUG=0`. `CARGO_TARGET_DIR` may name a fresh build-output directory on stable storage; it is never a source, fixture or configuration input. Do not reuse a build-target directory between code variants when validating changed tests. Each fresh verification uses its own build output. No provider/API secret is required for these scoped mock/unit checks. Do not use real credential, document, broker or router state. Do not enable paid model fallback.

## Dependencies and exclusions

Tracked lockfiles, manifests, generators and fixtures are the reproducible input. Build outputs (`target`, `.build`, `node_modules`, `dist`), dependency caches, coverage output and generated binaries are deliberately excluded and rebuilt. Older unrelated branches, private audit/planning reports, harness settings, personal records, real credential contents, local stores and original unrelated credential-store WIP are excluded, not hidden dependencies of the checks above. No raw chat or private memory is published.

Pinned Git dependency commits found in the selected top-level manifest: none in the inspected top-level manifests. Package managers must resolve these through public repositories; a fresh-checkout failure to fetch any is a concrete reproducibility blocker, not permission to alter a pin.

Native GUI/Keychain/Touch ID, signing, notarization, and real user-permission behavior need macOS/hardware and remain unverified by generic cloud execution. Network access to GitHub and applicable package registries is required for dependency setup. Production access, signing credentials and live-service secrets must be separately supplied through approved secret management, never this repository. No cloud job is launched by this document.



## Verification record

Prepublication secret-pattern/outgoing-history scans succeeded for the selected base. Exact WIP path/byte comparison is required for checkpoint variants. Product-acceptance and target-cloud runtime are **not checked** by these records.

No new source code was changed on this branch; the fresh remote-clone command results will be recorded below.

Fresh remote-clone verification was executed locally on macOS at published checkpoint `f40974a274555015450de9a9a601d34411740807`. The repository was cloned directly from GitHub, without copied worktree files, stashes or source/configuration overrides. The following scoped command chain exited **0**:

```sh
cargo test --locked -p symbrain-policy
```

Rust compilation used two jobs, disabled dev/test debug info and a distinct build-output directory for each variant. Those output directories contained no required source or fixture inputs. Package manager dependency caches were allowed; application state and credentials were not supplied. This verifies repository-contained inputs and these scoped checks, not every product test or native acceptance criterion. Final documentation changes do not change the tested source; the published final HEAD must still be verified before continuation. Target cloud runtime, permissions, secrets and network gates: **not checked**.

## Copyable continuation request

Work in `danieljustus/symaira-brain` on `handoff/20260930-cloud`. Verify the exact remote HEAD given by the final publication record, read `docs/handoffs/end-work-cloud.md`, run the setup and scoped checks, then: Independently review PRs #753 and #754 at their actual heads. They are separate candidates, not automatically combined. Check exact-head required CI and current protection before any explicitly authorized integration. Respect all preservation and integration gates above.
