# Code continuation: symaira-brain

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
