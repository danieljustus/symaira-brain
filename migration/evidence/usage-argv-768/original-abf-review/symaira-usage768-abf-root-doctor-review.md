# Full independent corrected Copilot/Kimi review (#768)

Disposition: REQUEST CHANGES, one P2 group on unchanged clean publication
abf20713bacdab562644256e27616a9dd7acb81e / validated source
00bcc6f51a2fbc00581ca7f6b7af0e868d168971. The original credential-owner P2 closes.
No candidate edits, publication, merging or self-approval occurred.

## P2: preserve Go argv parsing and byte diagnostics on newly admitted routes

Newly native literal Copilot and Kimi credentials now reach an existing Rust
Usage parser that loses the Go argv contract. With an owned file token
env://OWNED_LITERAL, actual frozen-Go, archived accepted parent9aed and current
CLI processes reproduce twelve mismatches across both providers:

- `usage ----help`, `----json` and `----not-a-usage-flag`: Go and parent reject
  the malformed syntax with `bad flag syntax: ---...`; current strips every
  leading dash and emits `flag provided but not defined: -...`.
- Raw Unix unknown flags ending ff or e282: Go and parent write the original
  bytes; current replaces them with U+FFFD before normalization/diagnosis.
- A raw positional argument ending ff: Go and parent quote `\xff`; current
  uses Rust's `\xFF`. Complete stderr bytes differ.

The accepted parent matches Go for every one of the22 actual triples (twelve
new-route mismatches and ten matching controls). This establishes the newly
admitted scope rather than labeling every inherited ordinary-token behavior a
new regression. Every process returns2 with empty stdout; these inputs do not
send provider HTTP or write credentials. Their complete argv, stdout/stderr/exit
bytes, source-state checks and binary hashes remain in
`/tmp/symaira-usage768-abf-root-doctor-cli-extra.json` and its .py/.log.

Locations: rust/symbrain-cli/src/usage_cli.rs:58–62,124–129 and debug_arg:87.
Preserve raw OS argument bytes through Go's precise one-time dash normalization
and FlagSet grammar, and use Go-compatible quoting/output. Retain the failing
triples unchanged and expand the source-bound invalid-argv gate beyond one
ordinary ASCII flag. Do not solve this by weakening byte comparisons, returning
every literal file to Go, or making a positive provider request.

## Fresh full verification and closed original finding

All four actual source-bound Go1.26.7 process gates pass on the unchanged HEAD:
Copilot/Kimi89 inputs,81 full reports/request walks,eight retained gates,
111 CLI comparisons and five genuine rejected controls; Files86 inputs/85 full
reports/151 CLI; Hermes96/104; Reference44/110, each with its five controls.
Original97 inputs replay with exact66 retained/31 routing-only mapping. All
62/57/55/51 source manifests and48 common entries bind the same source and CLI
91e25046826107a456fdbe9308b60193312edd2c51a222f355b02bb1fdff2d97.

The stronger16 full owner report/request/device/read-only cases,16 actual owner
CLI cases,22 actual Unix Clean/Join outputs and separate wrong-owner exit101
control pass. The original seven ordinary and five literal constructor inputs
were independently replayed with their exact original file bytes against the
unchanged actual Go constructor binary and freshly linked current production
rlib: all twelve select Go's lexical owner. Another32 actual owned Unix pairs
cover raw ff/e282 HOME, relative and leading-parent paths, repeated separators,
symlink-parent selection, erased regular/missing parent components, empty/unset
HOME, Unicode, raw explicit KIMI_CODE_HOME, legacy selection and environment
preemption. Their configuration/auth/Authorization observations all match and
all owner files/links remain read-only. These supplemental public probes have a
narrower observation surface than the full16 report/request/device gate.

Ordinary all-target/all-feature CLI/Usage tests pass355/zero failures/four
ignored oracle seams, each of those seams freshly executed by its actual gate.
Strict all-target/all-feature Clippy, workspace/included-fragment formatting,
actionlint and diff checks pass. All lifecycle executions used the container
subreaper with explicit owned target and Go SDK path, after explicit quiet release.

Reviewed complete changed decoder/routing/read paths, typed null/duplicates,
map replacement/ambiguity, Go field folding and invalid Unicode, numeric/depth
limits, literal/environment precedence, conservative unsafe-source gates,
capability reader integration, CLI admission and output, full comparators,
read-only state and negative controls, exact97 accounting, three-OS workflow,
bounded ADR claims, licensing and original failure provenance. Go production,
module files and frozen historical fixtures remain unchanged.

## Separate Windows algorithm observation and proof boundary

Host execution of mechanically extracted unchanged Go1.26.7 Windows
Clean/Join/WTF16 code against the production pure UTF16 helpers covers6065
deterministic unit pairs, including unpaired/paired surrogates. It reports6013
matches and52 differences. They occur at malformed root-?? or non-ASCII-colon
forms: Go postClean tests actual lazybuf allocation, while Rust tests final
output inequality; Go's empty device-volume return also retains original slash
bytes. These raw failed comparisons are preserved, not projected away, under
`/tmp/symaira-usage768-abf-root-doctor-path/`. No valid reachable credential owner
or native Windows process discrepancy was established, so this is not a second
P2 or a Windows acceptance claim. Native exact-candidate Windows/macOS CI,
including the21 actual Windows platform path cases, remains required.

No actual Windows filesystem/environment/ACL run was performed here. Go1.26.7
uses lossless WTF8/WTF16 conversion for Windows environment/path units; invalid
surrogates must not be treated as mandatory U+FFFD replacement based on older
SDK assumptions. Full #768 and unsupported source/host gates remain open.

## Retention and handoff

Verified73 original retained review artifacts, eleven original archive entries
(seven actual test/CLI binaries plus Go/native public probes, production rlib
and parent CLI),72 corrected author artifacts and current authored binaries by
SHA. The earlier accepted parent112 archived executable paths/74 unique digests
also verify. Original unit/no-subreaper failures and original wrong-owner
observations are unchanged. Review writes are confined to /tmp.

The first supplemental CLI harness preparation used an incorrect archive path
and failed before starting a candidate process. Its exact script/log are retained
as `-cli-extra-first.*`; the corrected runner uses the original preserved parent
CLI and verifies its receipt-bound hash. This is separate from the twelve actual
product discrepancies. Complete review receipt and all raw evidence are named
`/tmp/symaira-usage768-abf-root-doctor-*`.

All reviewer compilers and processes have finished. Source remains unchanged
and the exclusive target is released for the parent-coordinated successor.
