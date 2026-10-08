# Independent Copilot/Kimi review (#768)

Disposition: REQUEST CHANGES, one P2 group on immutable clean7e0b6335,
validated sourcef75993eb. Source and actual target remain unchanged.

## P2: clean credential joins lexically before selecting their owner

Newly admitted literal file tokens can come from the wrong account. With owned
HOME=<root>/link/../home, link points to owner/nested. Frozen Go filepath.Join
selects <root>/home; Rust PathBuf::join leaves parent components and reads
<root>/owner/home. Copilot apps and Kimi current-home credentials differ, and
KIMI_CODE_HOME=<root>/link/../home/.kimi-code reproduces the same issue.
Both providers are admitted natively. Real production constructors with owned
canned401 transports capture Go's Bearer env://LEXICAL_OWNER but native's Bearer
env://PHYSICAL_OWNER. Four ordinary-path controls and two literal controls match.
Every tested file remains unchanged; no operator credentials or live endpoints.

Locations: provider_config/files.rs:43, kimi_nous.rs:8/21 and the new routing
admission in routing.rs:38/58. Use Go-equivalent lexical clean/join at each
credential probe/read boundary, preserving raw OS paths and platform volume
semantics. Filesystem canonicalization is unsuitable: it resolves the symlink
and selects the wrong physical owner again. Keep constructor and eligibility
selection consistent. Retain these complete failing observations and add real
owned symlink controls to the source-bound gate.

Actual archived parentCLI9aed proves the classification: ordinary tokens were
already admitted, so that portion is inherited. Literal env:// tokens retained
Go in parent (absent-Go exit1) but are admitted in current (native flag exit2).
Five new literal constructor pairs give three owner mismatches/two controls;
five actual Go/parent/current routing pairs verify this newly admitted scope.
Do not describe every ordinary path mismatch as a new regression.

## Full fresh verification

Exact-head actual Go1.26.7 gates: Copilot/Kimi89 inputs,81 complete reports and
requests/eight explicit gates,111 CLI comparisons/five actual rejection controls;
Files86/85full/151CLI, Hermes96/104CLI, Reference44/110CLI, each five controls.
Fresh original97 Go inputs replay unchanged; all66 retained inputs and31 native
route-only observations are individually accounted for. Gate-only rows never
claim constructor parity. All59/55/53/49 manifests and46 shared entries agree
with current source and authored proof. All source hashes and CLI digest4d0417cf
match. Ordinary all-target/all-feature tests pass355/zero failures/four ignored
oracle entries, each explicitly executed by the four actual gates. Strict Clippy,
formatting, actionlint and diff checks pass. Ten additional actual malformed JSON
CLI pairs match and remain read-only; a suspected scanner issue was not reproduced.

Reviewed all changed production decoding/routing, raw duplicate/null/type/depth
and ignored-number behavior, environment priority, literal-token handling,
read-only access, CLI and actual constructor comparators/controls, all97
accounting, CI and ADR/migration statements. Verified112 actual archived parent
executables/74 unique digests, seven current authored executables,61 authored
retained artifacts and18 unchanged tracked historical proof files. Original
unit and no-subreaper failures remain preserved. Tracked Go source/modules and
frozen historical fixtures remain unchanged.

Native exact-head Windows/macOS remain required. Automatic host Keychain,
ambiguous map selection, unsafe/nonregular access, unsupported device/header
bytes, architecture/base/workspace and Windows HOME gates stay explicit. Full
#768 remains open. This review changes no production/source or GitHub state.

Full receipts/raw observations: /tmp/symaira-usage768-f759-root-independent-review-receipt.json,
all four root reports and .evidence directories, owner/literal/parent-route JSON,
malformed JSON pairs and complete test/check logs. Supplemental Go uses unchanged
frozen constructors; the Rust probe links the exact current production rlib.
