# Require verified macOS release publication

Status: implemented orchestration; actual Apple signing/notarization acceptance
remains unproved until a real configured release. Related issue: #730.

The delegated long-term decision is to require observable signing postconditions
at every transition rather than treating an exited signing command as sufficient.
The existing GUI workflow remains the supported distribution path while the
complete Rust release matrix, Homebrew/Scoop updates and rollback are unfinished.
This increment neither publishes a release nor closes #730.

Import the PKCS#12 aggregate using format inference and permit the codesign tool
explicitly; do not misclassify it as a certificate-only import. Secret files use
private permissions and unconditional cleanup. Keep the committed AppKit URL and
exact version; remove CI/release's stale rewrite to 0.6.0. XcodeGen installation
must succeed, and build/signature failures must stop publication.

Before submission, verify the app signature, Apple anchor, certificate team and
stable app identifier. Sign the DMG and verify its team too. Require Apple's
JSON status to be Accepted, then independently query the same submission ID and
require that read-back to be Accepted. Stapling success is followed by ticket
validation, another signature verification and native Gatekeeper assessment.
The app and DMG use their respective execute/open assessment types.

Only a verified DMG is uploaded. Download that exact release asset again, compare
its complete bytes with the verified local file, and repeat native signature,
ticket and Gatekeeper checks on the downloaded copy. Keep nonsecret submission
and read-back receipts for 14 days, including available failed-stage receipts.
Missing receipts before submission do not count as acceptance. Optional volume
icon decoration remains optional; signing, notarization and cleanup do not.

Five portable orchestration tests execute fourteen owned shell processes using
explicit synthetic command doubles. They prove that seven individual tool
failures, non-Accepted status, mismatched read-back ID and invalid team prevent
publication, and that app/DMG success and downloaded-asset verification preserve
the required sequence. They are not native Apple certificate, Keychain,
Gatekeeper, notarization-service or published-asset proofs. Those checks require
macOS, configured release secrets and an actual release. Native CI labels and
existing merge protection remain unchanged.

Current-main integration: normal-merge main31de722 after Activity PR800,
then repeat five portable orchestration tests, workflow lint and shell syntax.
The release scripts and their reviewed source hashes are unchanged. The three
new Activity acceptance/artifact blocks integrate in CI; the release contract
job is byte-identical. This binds evidence to the current combination while
retaining the native release and actual Apple acceptance requirements.

Normally integrate main `e3dbda6c` after reviewed profile publication. Clean
combined source56db48 passes the same five portable tests/fourteen owned
synthetic processes, strict actionlint and shell syntax. The release workflow
and all executable/test scripts remain byte-identical. The previous ADR hash
predates its main31 rationale; an initial metadata assertion mistakenly
expected it to be unchanged. Retain that verification error and correct only
the comparison scope, preserving actual successful product observations.
The new source-bound record is `maine3-integration.json`. Fresh protected CI
remains mandatory. No actual Apple authentication or published signed release
is claimed; #730 remains open.


## Main documentation integration, 2026-10-04

Normal integration of main2b6d49f changes only documentation and preserved historical evidence. The release workflow, all scripts, test cases, dependencies and contract matrix remain byte-identical to reviewed87261c3. Retain the existing complete orchestration/negative-control proof; no repeated local build is needed for unchanged code. Require the new published head's protected CI before merging. Real Apple signing, native publication and full730 acceptance remain open.
