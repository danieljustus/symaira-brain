# Rust Skills, Memory and Browse continuation

## Goal and source identity

Repository: `danieljustus/symaira-brain`. Remote branch: `handoff/20261012-rust-wip`.
Base code commit: `dfa42dc32859b4da636054d215dc6a80d619a959` plus the 24 exact WIP files listed in `2026-10-12-rust-inputs.json`. Related existing draft PR: [#813](https://github.com/danieljustus/symaira-brain/pull/813). This new snapshot does not replace that PR's head or authorize integration.

The committed layer admits reviewed corrected Skills references for native process collection. Preserved WIP implements Memory conflict/write semantics and Hamming/prefilter behavior, plus Browse root-session dispatch/help/daemon handling and regression inputs. These are source checkpoints, not compiled or process-parity acceptance.

Required corrected Go source: branch `oracle/skills-control-nofollow-v1`, commit `ce909711d6e7125d281840d14744e7aa987f626f`, in this same repository. Fetch and verify this exact ref separately. Its immutable original reference remains `dcddcef0df5789123c7c9a7ebe6e01f10e941f2c`. Root Rust LLM dependency pins public CoreKit `04d1411adb57aa602b992509121011aa7666ff1a`.

## Concrete next task and current failure

Run the following existing noncompiling admission controls first:

```sh
python3 -B -m unittest discover -s scripts/skills-native-oracle -p test_reference_admission.py -v
```

Actual closeout result on the preserved source: 9 tests, 1 error, exit 1. `test_compiler_collector_reads_selected_embeds_modules_and_entire_sdk` raises `ValueError: corrected compiler must be the pinned native unmodified SDK environment` at `scripts/skills-argv-oracle/compiler_inputs.py:57`. Investigate the actual test-fixture/collector contract before editing. Do not loosen production provenance checks to accept the test. The other eight controls pass; this is not native evidence.

Then reconcile the 24-file source manifest, obtain independent review of the integrated WIP, and run the affected Memory/Browse compiler and actual Go/Rust process gates. `scripts/memory-cli-oracle/WRITE-CONTRACT.md`, `PREFILTER-CONTRACT.md`, `browse/port/harness/root_session_cli.py` and `scripts/skills-native-oracle/README.md` describe those contracts. The historical 209 mismatches among 1,112 Skills comparisons are retained failures, not accepted corrected-reference results.

## Unresolved inputs and acceptance gates

- Exact corrected-reference build receipts and binaries are deliberately hash-admitted. Use the existing public artifact fetch route documented in `scripts/skills-native-oracle/README.md`; this handoff has not established indefinite retention of all referenced Actions artifacts. Missing/expired artifact or wrong receipt must fail closed, not trigger arbitrary rebuild-and-trust.
- Historical preservation for previously removed `prsweep-810`/`issue773` working copies remains unresolved. This source snapshot preserves the current candidate but does not prove recovery of those old untracked files or clear that integration hold.
- Current Memory/Browse WIP has no full compiled/native acceptance here. Full Skills CLI/MCP and startup/security native gates, independent review, integration, release and Go retirement remain open.
- Target cloud runtime and permissions: not checked. Source checkout and focused checks alone do not establish the matrix.

## Shared boundaries

This is preserved work in progress, not migration completion or merge/release approval. Preserve exact Go/Rust byte contracts, original failed captures, source/compiler provenance, security policies and independent native-platform gates. Cross-compilation and source-only tests do not prove native execution. Never manufacture expected results, weaken a deadline, discard a mismatching row or change an oracle only to make a comparison pass. Signing, releases, productive cutover, Go retirement and destructive cleanup require separate authorization. Do not start paid models or services.

Local automatic continuation has been disabled and the owned unfinished work parked. This document does not restart automation. Existing remote CI may run from the ordinary push; no workflow dispatch, merge, tag or release is requested.

## Reproduction environment

Start at the repository root of a direct GitHub clone and check out the immutable final HEAD provided by the external verification receipt. Do not substitute the branch's later tip. Use Python 3 (closeout host: 3.14.2), Git (closeout host: 2.54.0) and the exact Rust 1.98.0 components in `rust-toolchain.toml`. `Cargo.toml`/`Cargo.lock` and, for Go oracles, `go.mod`/`go.sum` remain authoritative. GitHub and the public package registries must be reachable. No package installation or cloud runtime is implied by these instructions.

For local Mac development, respect the host's existing storage/build launcher and reserve policy. The host's mounted-drive paths and private launcher are not cloud inputs: use ordinary owned output/temp/cache directories on a suitable cloud filesystem instead, without weakening product permission checks. Use isolated HOME/XDG fixture state, never real user stores. Native Apple GUI, Keychain/Touch ID, signing and filesystem tests require the real supported OS/hardware and authorization. Native Linux and Windows observations must run on those systems.

Fresh-checkout verification at the final handoff HEAD is pending until the separate PR comment is published. The external comment records exact commands, exits, failures and runtime limits without recursively changing this commit. A successful source inventory or Python test is not a full application build.

## Retained and excluded inputs

Build outputs, package caches, Python bytecode, local control databases, private orchestration reports and the completed local cache-cleanup script are not shipped. None is an allowed replacement for missing source. Originals remain local. Historical local acceptance receipts are not silently treated as public or independently verified by this document; any needed unpublished immutable receipt remains an explicit blocker below. No credentials, `.env` values, production data, personal memory or signing keys are included. Provision target credentials only through the operator's approved secret manager if a later explicitly authorized operation needs them.

No submodules or Git LFS inputs were found in the inspected source inventory. Check locks and any downstream platform-specific dependencies again before their own gates. The portable source checkpoint is separate from native-runtime, access and acceptance readiness.
