# One resolved Brain configuration value

Status: source preparation; no build, tests, differential execution or cutover
accepted for this successor. Refs #765, #769. The user delegated implementation
choices and requested their reasons in docs.

## Decision and evidence

Brain owns one typed, per-invocation `symbrain_core::config::resolved::BrainConfig`
for its complete historical 13-field configuration. Its consumers receive actual
values, rather than independently checking validity and rereading partial maps.
Core depends on no installer, Broker, Gateway, Guard, Vault or Memory package.
Broker and Harness gain additive native-path seams and depend on Core; their
existing String APIs continue to delegate. No new third-party dependency or
version change is introduced.

The actual pinned Go/DCdd oracle and the source58fe native executable were
compared in private roots before implementation. The final census contains 115
groups /269 process executions, including39 fallback sentinel invocations. These
are diagnostic observations, with mismatches retained, not passing tests. The55
original proof files, receipt, actual native CLI,49 native source snapshots,
CoreKit/SDK sources and immutable Go contract are retained in
`migration/evidence/brain-config13/initial-58fe/retention.json`:131 mappings,
113 unique lossless gzip payloads. Existing failed/provisional scripts and their
outputs are retained separately. No original is rewritten by this successor.

## Typed value and admission

The field table follows Go struct declaration order, independent of TOML key
order. Defaults merge with the global file, cwd project file, then the nonempty
`SYMBRAIN_*` environment values. Each load is fresh; there is no process cache.
An absent file is skipped only for Go-equivalent not-exist admission; other stat
failures proceed to the actual open/read operation. Failed cwd omits the project
stage. HOME admission precedes file stages, even with an absolute XDG directory.
Only absolute XDG values select Brain's global config directory.

| Order | Field | Default | File behavior |
| --- | --- | --- | --- |
|1|default_profile|empty|plain string; empty value does not overwrite|
|2|audit.enabled|true|pointer bool; every present value applies|
|3|audit.verbose|false|plain bool; zero value does not overwrite|
|4|gateway.identity_injection|true|pointer bool|
|5|updatecheck.enabled|true|pointer bool; validate/store, no invented updater|
|6|servers.vault.binary_path|empty|plain string|
|7|servers.operate.binary_path|empty|plain string|
|8|servers.scope.binary_path|empty|plain string|
|9|patterns.enabled|true|pointer bool; recording only|
|10|patterns.promotion_threshold|3|plain Go signed64 int; nonpositive resolves3|
|11|modules.browse|false|plain bool|
|12|modules.operate|false|plain bool|
|13|modules.scope|false|plain bool|

Plain string/int/float/bool zero values do not overwrite prior file values;
pointer bool fields do. Scalar/array values for nested structs and unknown keys
are ignored. Bool aliases and signed decimal integer parsing preserve Go's
syntax/range priority. Float-to-int conversion has architecture-specific Go
instructions: AMD64 invalid/out-of-range conversion returns the minimum signed
value; ARM64 behavior must be proven on its native runner. The prepared tests
include actual frozen-Go type-conversion stderr bytes for all13 fields.

| Consumer | Load boundary and resulting values |
| --- | --- |
|Stored config path/get/set|global map API only; no resolved project/env validation|
|Plain Setup / repair|after existing argument/managed-root/manifest admission; Browse release selection|
|Source Setup|after source-root admission, before explicit/default module selection; all3 optional modules|
|Doctor report|complete typed parsed/error status, distinct from handshake load|
|Doctor fix|existing checked human write boundary first, then typed admission|
|Doctor handshake|one separate load; failures default binary discovery as Go|
|Install|after harness resolution; explicit profile bypasses load, otherwise resolved default_profile|
|Vault passthrough/admin|whole-load failure ignores configured override; existing credential policy stays|
|MCP/serve|after profile admission, before children and Memory construction; typed audit/identity/pattern options|

Browse remains the only optional release core. Source install retains its full
Browse/Operate/Scope selection and existing provenance/ownership/build contracts.
Invoking historical Go source builders from Rust does not prove a Go-independent
source installation; locked native Browse builds remain a later accepted cutover.

## Native path ownership

Environment admission reads relevant `var_os` values, never the full Unicode
`env::vars()` iterator. This removes the census-proven raw DEFAULT_PROFILE panic
before typed admission. Filesystem selectors remain raw Unix bytes or native
Windows UTF16 through resolution, configured discovery, lazy spawn and restart.
Windows codec follows Go1.26 syscall WTF8, including lone surrogate units; it
does not replace them with U+FFFD. HOME uses USERPROFILE alone on Windows.

On Unix, Go accepts an absolute PWD spelling only when stat identifies the
same current directory. That spelling is retained until lexical project-file
Join; following links first can select another owner. Windows ignores PWD.
The pinned SDK source and owned symlink/negative case definitions are prepared;
no successor PWD runtime result is claimed.

Shared lexical Join/Clean follows Go's byte-addressed lazy buffer, including
stale allocated slots inspected by Windows postClean. It never follows symlinks
or canonicalizes owners. Harness uses its own historical nonempty-XDG contract,
not Brain's absolute-XDG admission. Its existing trusted-root/AtomicFile limits
remain enforced. Unicode repair happens only where existing harness metadata
formats require it; it cannot select an owner or configured child.

## Gateway options and boundaries

CLI audit is enabled when either profile or resolved config enables it; verbose
logging uses the resolved flag. Identity injection is the existing routed-memory
option with caller-wins semantics, separate from authoritative embedded Memory
attribution. Pattern recording captures completed tool server/name pairs only,
including errors; bootstrap/patterns and unknown/hidden tools are excluded.
Episodes are flushed once at EOF/cancellation; empty episodes do not write;
store errors remain best effort. The configured positive promotion threshold
feeds the existing read-only pattern tool. No conduct policy, secret arguments,
results, master keys or credential grants enter this path.

## Explicit gates before acceptance

The locked `toml_edit` parser owns its inner malformed-syntax wording. One
native parser avoids maintaining a parallel private Go grammar/formatter solely
for legacy phrasing. The retained old44-group source280 observation for exact
`[bad config` bytes proves an existing diagnostic boundary: Go exits2 with its
parser wording, old native exits1 with a line1/column5 native parser message;
stdout and full file state are identical. The old native exit1 is not accepted.
The final115-group source58 census has typed/error-order evidence, not malformed
TOML grammar evidence. The full unchanged old observation and its precise
successor contract are retained in the separate parser decision receipt.

This wording choice is narrow: the raw selected file and outer stage chain,
fatal2, parser-before-type and global→project→environment ordering, no child/HTTP/
filesystem mutation, supported syntax acceptance and UTF8 priority stay mandatory.
All13 typed conversion/field chains remain byte-exact. Native location/reason is
reported without repeating the entire input line. Actual current process pairs
must prove this choice only on named malformed inputs; it cannot cover syntax
accepted/rejected differences, UTF8 priority, typed values or SDK discovery.

`resolved/document.rs` still has unaccepted syntax/duplicate/UTF8-priority gates.
In particular, full UTF8 validation before parser admission may report a later
invalid byte ahead of earlier grammar; that case is prepared and must be resolved
before cutover. This source checkpoint is not publishable with that gate open.
Existing repair/source/Vault fallback predicates remain intact.

The additive Broker path API also requires complete SDK exec.LookPath parity:
relative PATH/ErrDot, Windows implicit CWD/PATHEXT and same-file continuation.
Existing lookup code is not promoted as proof of that contract. All affected
consumer, lifecycle, raw owner, error and negative-control process gates must run
against one clean source-bound executable after a target is allocated.

The census also proved native MCP currently constructs Memory/JWT conditionally,
where Go constructs them eagerly even for disabled profiles. That is separate
#759 runtime ownership work; no filesystem projection hides it and this loader
does not change the store/provider. Whole-MCP acceptance therefore remains open.
Legacy schema/apply/backfill gaps remain #649/#758. Native Linux/macOS/Windows
runtime, independent review and protected CI remain required. Neither #765 nor
#769 is closed by this precompile checkpoint.

## Follow-up to the independent e63 source review

The immutable e63 publication and its independent report/projections are kept
in `migration/evidence/brain-config13/independent-e63e/retention.json`, alongside
the unchanged initial131 mappings/113 compressed payloads. The review found
three source defects; it did not execute the uncompiled candidate. This isolated
successor normally merges main5e. No original observation is rewritten as a
passing result. The source-only follow-up does not have compiler, target, port
or product-execution allocation.

Vault admission and override loading now delegate to the same public resolved
`Sources` implementation as the other Brain consumers. Only the injected Vault
environment-variable name changes for existing tests. The existing private
13-field validity parser was removed. Maintaining another parser or calling
physical `current_dir` here would let Doctor/Setup admission and Vault select a
different project file from Go's same-inode absolute PWD owner. All selectors
stay native OS paths; a failed full load still ignores the Vault override, as Go
does. Existing repair/source fallback predicates remain in force.

Install keeps the resolved profile byte-valued until its specific harness
encoder. JSON metadata uses Go's one-invalid-byte-per-replacement behavior;
Codex TOML writes the original bytes using the pinned BurntSushi1.6 quoting
rules. This intentionally preserves historical raw strings even when the
result is not UTF8; it is not a new permissive decoding rule. A small module
replaces only the generated profile token at its parser-provided span. It uses
no sentinel, global text substitution or new dependency. Existing equal literal
U+FFFD values, comments and other servers cannot become replacement targets.
Overwriting/removing an entry clears its byte override. Existing String-valued
server metadata remains a repaired view and cannot be used as a path selector.
The retained MIT quoting license accompanies the source-derived rules.

Dry-run needs the same raw bytes as a real write. The additive byte-valued
unified-diff API therefore retains path/content bytes while keeping the existing
line/cell budgets, three-context grouping and deletion tie-break. Its String
wrapper preserves the existing API for Unicode callers. Otherwise a successful
Codex write would silently change into repaired bytes in its preview, or compare
an invalid byte as equal to a literal replacement rune.

The resolved parser strips exactly one leading FF FE, FE FF or EF BB BF marker,
as the pinned Go parser does, before validating the remaining UTF8. This does
not decode UTF16, recursively remove prefixes or accept later invalid bytes.
Supported syntax and all13 ordered field conversions remain required. The
single native parser still has the separately documented earlier-grammar versus
later-invalid-UTF8 priority gate; the accepted inner diagnostic wording does not
waive that gate, typing, owner selection, admission order or any mutation.

The original102 CLI/152 Unix/174 Windows definitions and actual older proofs
remain byte-identical. Additive prepared regressions cover182 Unix/157 Windows
consumer cases and146 loader cases. They include all13 fields behind markers,
valid/invalid owners, raw/literal profile collisions, checked positive exits,
fatal zero-write/no-child boundaries and three actual fault executable
definitions. Five new Rust test functions cover marker/type order, backend
serialization, collateral values and raw diffs. They are uncompiled/unexecuted;
static formatting, Python syntax/case enumeration and archive/source hashing
cannot substitute for actual Go/Rust pairs, strict Clippy, parent lifecycle
gates, native Linux/macOS/Windows or independent runtime approval. All previously
listed #765/#769 and parser/lookup/eager-Memory boundaries remain open.
