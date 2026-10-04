# Independent full-layer review: macOS release signing/read-back #730

Disposition: **APPROVE the scoped release verification orchestration**, subject to fresh required CI at the published head. No actionable findings identified. This does not certify native Apple signing, Keychain import, notarization-service acceptance, Gatekeeper behavior, or a real uploaded release. #730 remains open for the Rust release matrix, tap/Scoop distribution, actual signed/notarized release acceptance, and publication/rollback completion.

## Immutable artifact

Base: `e8c7f7990ab61967db0a3cbadaf6e485b4a33d99`.
Reviewed clean source: `e1266694a1a454d1d9182351a4bc7c17cb388999` in `/workspace/symaira-release730`.
All six changed files were read, and source SHA-256 values checked against independent probe receipt `/tmp/symaira-release730-independent-probes.json`. The candidate remained clean and unchanged. Reviewer did not author the patch or perform signing, upload, secret import, publishing, push, PR creation, or merge. Additional probes and reports are outside the candidate in `/tmp`.

## Full-layer findings

The certificate import now requests PKCS#12 format and leaves item type inference intact, rather than forcing certificate-only `-t cert`. It explicitly trusts `/usr/bin/codesign` through `-T` and removes the permissive `-A` option. The imported identity is selected from the owned temporary keychain, requires a valid ten-character team, and exports the keychain path before creation so the unconditional cleanup step can handle partial import. Private `umask 077` applies before writing both encoded-secret files; cleanup removes PKCS#12 and API private-key files and deletes the temporary keychain. Existing protected release environment and action permissions are unchanged. The actual imported identity and ACL behavior still require macOS verification.

The app is signed through the existing hardened-runtime/timestamp Xcode settings. The new helper first verifies deep/strict signature validity and then an explicit `codesign -R` requirement: Apple developer anchor, team OU, and the app's stable `com.symaira.brain` identifier. The app is submitted as a preserved-parent ZIP; the DMG is explicitly signed and submitted as a DMG. Apple's submit JSON must have `status=Accepted` and a UUID submission ID. An independent info call for that same ID must also be Accepted and identify exactly the same submission before stapling proceeds. Missing/malformed/non-Accepted receipts fail closed. Accepted status is not inferred merely from notarytool's process exit.

Stapling is followed by ticket validation, both signature checks again, and native Gatekeeper assessment. Apps use execute assessment; DMGs use open assessment with `context:primary-signature`. The downloaded release copy uses verify-only mode, which repeats those native checks without resubmitting or restapling. Shell `set -euo pipefail` propagates each failure. Independent post-stapling controls confirmed both the third and fourth signature commands can fail after successful stapling/ticket validation and still prevent the following publication marker.

The actual workflow upload/download step downloads the exact asset name into a fresh owned directory, compares complete bytes with `cmp`, and invokes verify-only on that downloaded copy. An independent execution of the exact YAML `run` block with owned synthetic gh/Apple commands proved that download failure or one changed final byte prevents every native verification command; invalid ticket or Gatekeeper failure blocks subsequent success. Upload happens before this read-back, so a failing read-back blocks the dependent Homebrew job but does not remove an already uploaded asset. Release rollback/publication completion remains open; this scoped patch does not promise atomic rollback.

The existing app/dmg creation chain and pinned `project.yml` AppKit URL/version (`0.14.2`) were inspected. Removing the stale automatic rewrite to `0.6.0` preserves the committed pin; failed XcodeGen installation now surfaces as a job failure. No production Go, frozen fixtures, dependency lockfile or action SHA was rewritten. Existing native runner labels and release dependency structure remain unchanged. The new portable CI job executes the actual shell orchestration controls. JSON notarization receipts are retained for 14 days on failure as well as success; absence before submission is explicitly not accepted. ADR accurately separates this increment from full #730 completion and genuine Apple release evidence.

## Official CLI semantics checked

Apple's published Code Signing Guide confirms `-R` tests a single explicit requirement, while lowercase `-r` is used to set requirements. The guide's verification example is `codesign --verify --deep --strict --verbose=2`; its app assessment example uses `spctl --assess --type execute`. Apple's requirement-language documentation specifies `anchor apple generic`, certificate leaf matching, subject.OU as the developer Team Identifier, and the optional equals sign in the identifier expression. Apple's TN2206 documents the DMG assessment syntax `spctl -a -t open --context context:primary-signature -v MyImage.dmg`. Apple's open-source security(1) manual explicitly documents aggregate inference and shows `security import /tmp/mycerts.p12 -f pkcs12 -k newcert.keychain` without `-t`; it distinguishes `-T` from insecure `-A`. Apple notarization documentation confirms accepted containers, Accepted response status/UUID, and stapling individual app/DMG artifacts rather than ZIPs.

References read during this review:

- https://developer.apple.com/library/archive/documentation/Security/Conceptual/CodeSigningGuide/Procedures/Procedures.html
- https://developer.apple.com/library/archive/documentation/Security/Conceptual/CodeSigningGuide/RequirementLang/RequirementLang.html
- https://developer.apple.com/library/archive/technotes/tn2206/_index.html
- https://raw.githubusercontent.com/apple-oss-distributions/Security/main/SecurityTool/macOS/security.1
- https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution
- https://developer.apple.com/documentation/security/customizing-the-notarization-workflow

The notarization documents were read through Apple's corresponding public tutorial-data JSON endpoints. Apple native executables are absent in this Linux environment. The unchanged existing notarytool credential aliases and platform-native invocation behavior are therefore not asserted as freshly executed; actual macOS release acceptance remains required.

## Independent verification

- Independently executed the committed suite: **5 unittest methods passed**, representing **14 owned shell orchestration processes** with explicit synthetic commands. Receipt is `/tmp/symaira-release730-independent-tests.log`. These do not execute Apple tools or authenticate real certificates.
- Independently executed **12 additional owned synthetic processes**: four post-stapling app/DMG signature failures (verification commands 3 and 4), three verify-only signature/ticket/Gatekeeper failures, and five executions of the exact workflow read-back block (success, changed byte, failed download, invalid ticket, rejected Gatekeeper assessment). All expected results passed. Trace arguments, exits, outputs and receipts are retained in `/tmp/symaira-release730-independent-probes.json`; source is `/tmp/release730-independent-probes.py`.
- Independent actionlint for both modified workflows and shell syntax check passed. The existing test suite also validates Accepted/read-back mismatch handling, app/DMG assessment kinds, invalid-team rejection, and verify-only no-resubmission sequence.
- Verified all six candidate source hashes and clean HEAD after every probe. No actual secret, signing identity, Keychain, Apple service call or external asset publication was used.

Inspected changed files: `.github/workflows/ci.yml`, `.github/workflows/release.yml`, `scripts/notarize-and-verify.sh`, `scripts/verify-notary-receipt.py`, `scripts/test_macos_release_verification.py`, `docs/adr/release-signing-readback-730.md`. Direct surrounding files inspected include `AGENTS.md`, `project.yml`, `scripts/set-app-version.sh`, `scripts/create-symaira-dmg.sh`, native build/signing settings and downstream Homebrew dependency gates.
