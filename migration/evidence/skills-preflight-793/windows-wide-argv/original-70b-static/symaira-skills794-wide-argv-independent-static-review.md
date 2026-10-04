# Skills Windows argv independent static review and gate plan

Immutable candidate `70b76390f372c6f05983f01f5a2b8147356ca39e` in
`/workspace/symaira-skills794-wide-argv`; author794 and independent reviewer768
are different authors. Parent `01f41906e2e021db3c693ec97617bb701e4dad8a` normally
includes main `2b6d49f250a81650eda1b93cec7d859a171873bb`.

**Static phase only, not approval.** No target was accessed, no Rust/Go compiler
or candidate/oracle CLI was run, and no source or GitHub state was edited.
Runtime validation awaits explicit target/resource allocation. New312 Windows
pairs and three actual controls are prepared, not passing observations.

## Full source review

The production chain is `run_with_executor -> run_in_process ->
output::extract_format -> skills_cli::run -> run_sync -> sync_flags::parse`.
Unmatched output arguments clone their original `OsString`; sync does not run
the inherited lossy Windows `normalize_flags` first. Local normalization reads
native Windows wide units, encodes lone surrogates as Go WTF8, and normalizes
the complete byte vector once until the first bare `--`, including separated
flag values. The parser and all diagnostic name/value spans then retain bytes.

`wide_bytes` pairs high/low units through `char::decode_utf16`; each lone unit
is encoded as its original three WTF8 bytes. This matches the pinned SDK
`decodeWTF16` control flow on inspection. U+FFFD stays a distinct valid Unicode
scalar. Empty arguments, Unicode values, ordinary flag booleans and repeated
flag values retain the original parser semantics. The byte-only target trimming
algorithm is the exact earlier Unix algorithm moved to the focused module;
invalid boundary bytes stop trimming and valid surrounding whitespace is
removed. Scope validates its trimmed valid string but quotes the untrimmed
original operand on error, matching `resolveScope`.

Go parser/help/syntax/boolean errors precede postparse validation; target errors
precede scope errors; validation precedes `resolve_skills_dirs`, configuration,
library discovery and install operations. Known target names retain the same
registry and order. Preflight still executes natively rather than falling back
because sync's existing `requires_go_fallback` branch remains false.

The Core formatter change is an additive public export/`must_use` attribute,
with no algorithm change. It escapes each invalid byte separately and preserves
valid Unicode through the existing Go-printability table. The new local codec
does not alter Core OsStr conversion, other commands, path resolvers, marker
safety, install/sync implementations or the shared normalizer. Full Skills
model/install/sync/status and bounded marker code remain the unchanged parent
and must still be exercised by inherited gates. Go source, Cargo manifests/lock,
dispatcher, output extraction and Core path/config codecs are unchanged.

Recognized global output flags still have inherited lossy/raw diagnostic gaps;
they run before the local parser. They are outside this bounded correction and
must be separately compared to an archived parent, not treated as new Go parity
or normalized away. Console output is separate from pipe-based process proof.

## Provenance and corpus review

`...-independent-static-provenance.json` records391 current source hashes,
490 original snapshot hashes, all24 original evidence files unchanged, eight
exact pinned Go1.26.7 SDK hashes and four byte-exact SDK copies/license/version.
The actual SDK preserves lone UTF16 units using WTF8; the old replacement
assumption is explicitly corrected without rewriting original reports.

All315 original argv byte vectors are preserved case by case:245 exit2 errors
and70 exit0 empty-library cases. Existing459-test and native-three-OS reports
retain their original source identities. I independently verified the archived
124 ELF paths/92 unique byte sequences via gzip/length/SHA without reading old
live target paths. The new Windows corpus statically expands to312 unique IDs
with recorded original UTF16LE units and no NUL operands. The launcher uses
Windows Python Popen/CreateProcessW and immutable executable copies; the recipe
does not simulate a Windows runtime on Unix.

The Windows workflow builds the actual frozen Go revision with VCS provenance,
uses an explicit `.exe` path and MSYS scratch-root conversion, compares complete
stdout/stderr/exit and owned final state, and uploads all raw JSON reports even
on failure. Each input mutant must actually return mismatch1 and pass the
verifier's exit2/empty stdout/readonly/nonidentical stderr checks. The additional
independent review should also check each intended diagnostic family, because
the verifier establishes broad preflight mismatch rather than validating every
possible stderr family. No new dependency or unlicensed SDK source was added.

The process snapshot records file content SHA, mode, type and symlink target;
it does not currently record mtime. Independent new probes will include mtime,
size and directory metadata so a create/delete or identical-byte rewrite does
not silently count as a readonly result. This is an evidence limitation, not a
proven production write.

## Outstanding precise source risk

`rust/symbrain-cli/tests/skills_windows_wide_flags.rs` imports the private
`process` helper but calls only `run_os`. That helper also defines `pub fn run`
at `tests/support/skills_preflight_process.rs:11`, without a dead-code allowance.
On native Windows, this newly compiled integration-test module may trigger
default `dead_code` and therefore fail `--all-targets ... -D warnings`. Linux
excludes the whole integration test with `cfg(windows)` and cannot establish
this path's acceptance. This is a focused static lint risk to verify; no native
Windows failure or final REQUEST/APPROVE decision is asserted here.

## Full next gate plan after explicit allocation

Preserve the allocated target's original executable bytes and source receipts
before reuse, with no duplicate target. Coordinate compiler priority,700MiB
floor and any shared11434 process controls with Root. Use explicit Go1.26.7,
debug0/testdebug0/incremental0/jobs2, isolated HOME/XDG/PATH and subreaper.

1. Full unchanged historical affected graph: `symbrain-audit`, `symbrain-cli`,
   `symbrain-skills`, all targets/features and strict Clippy. Recompute outer
   test counts without counting child summaries twice. Expect the old459
   acceptance plus actual newly executed platform tests, not a fictional old
   exact count. Add Core all-target/features tests and strict Clippy because
   the byte formatter is newly public; Core was not a standalone test package
   in the retained459 run. Workspace fmt and actionlint are required.
2. Actual clean full frozen public Go/native CLI build, with source/VCS/module
   hashes and retained ELF copies. Fresh original315 Unix vectors, historical
   Go results and full before/after state must all match. Run all three actual
   input controls separately and check exact intended flags/quoted errors.
3. Preserve original Skills loader/install/status/runner fixture bytes and
   source mappings. Run affected real fixture tests/frozen checks; new output
   paths only, never regenerate a frozen input in place.
4. Independent novel plan contains152 cases:99 Windows,40 Unix raw-byte and13
   portable/inherited-global cases. It adds high/low endpoints, maximum valid
   pairs, noncharacters, reversed/adjacent surrogate histories, literal
   quote/backslash/space/newline command-line quoting, Unicode-space boundaries,
   repeat overrides, normalization-once, help/terminator/positional stops,
   boolean-before-validation and target-before-scope precedence. Actual seeded
   malformed config/library/marker state must remain byte/mode/mtime-identical
   for all preflight errors. Inherited global output results are separate parent
   attribution, not admitted equality exceptions.
5. SDK copied-code reference and exhaustive local wide codec tests may support
   source semantics but are explicitly not native Windows acceptance. Execute
   actual Windows312 plus independent Windows cases on a genuine Windows CI
   runner, with original UTF16 command-line units/streams/full state/provenance.
   Linux/macOS exact-head protected/native jobs also remain required.
6. Full final different-author report/receipt must cover source scope, original
   data, actual tests/controls, new probes, archive byte bindings, remaining
   global-output limits and platform status. No issue closure or full Skills
   migration claim follows from this preflight increment.

Prepared artifacts: `/tmp/symaira-skills794-wide-argv-independent-prepared312.json`,
`...-independent-novel-plan.json`, `...-independent-static-provenance.json`.
The source remains clean and immutable; runtime review is not started.
