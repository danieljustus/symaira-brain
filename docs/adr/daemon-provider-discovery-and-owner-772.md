# Preserve provider discovery before owned daemon startup

Decision, 2026-10-04: keep the scoped lifetime supervisor and raw native Path/OsStr transport, but resolve a bare provider with the pinned Go discovery policy before running it. An empty or relative PATH must not silently expand execution to an unrelated project-local symvault. The actual default CLI reproduces this owner change even when command output matches. Explicit paths remain exact; standalone users retain their existing public contract. Preserve first-relative executable refusal, documented Go opt-in and platform-specific Windows rules rather than treating all relative text as equally executable.

Reason: executable discovery chooses the process entrusted with a key lookup. Adopting Rust's different search behavior would change that trust boundary during an otherwise compatible cutover. A complete independently passing standard corpus cannot substitute for the concrete failing owner observation. Original observations, failures and binaries remain immutable; source-only SDK/cross checks do not replace real native acceptance.

Full independent review of sourceccc/publication2b275 remains REQUEST_CHANGES until the scoped discovery successor passes the complete original and additional actual process/control gates. See migration/evidence/browse-state-key-772/independent-ccc/review.md and receipt.json. Full #772 and release cutover remain open.

## Scoped successor preparation

The isolated successor starts from publication2b275 and normally merges the
Root's immutable review publication4798; it does not replace the newer State
source with the Root's older daemon branch. All57 complete reviewer raw proofs,
the original ten CLI executions/query ledgers, original failed archive guard
and verified cleanup, and the current247/187/61 executable receipt remain exact.
The released Osargs target was independently reverified and retired by Root to
recover shared capacity. Source preparation is not a build or runtime claim.

Only `Ownership::Provider` in the opt-in lifetime supervisor performs the new
bare-name lookup. Explicit paths containing a native separator (or Windows
volume colon) retain the exact OsString. The supervisor program and the entire
standalone runner file keep their original bytes/semantics. On Unix, the scoped
runner also retains the original provider argv0 via CommandExt::arg0.

Pinned Go1.26.7 Unix lookup has zero entries for empty PATH. A nonempty empty
entry means cwd; the first executable relative candidate returns ErrDot, even
if a later absolute provider exists. Effective X_OK access is checked with
existing rustix1.1.4; only ENOSYS/EPERM fall back to111 mode bits, matching the
SDK. Directories and inaccessible/non-executable candidates do not win. The
last explicit GODEBUG execerrdot=0 opts into relative discovery; it is neither
injected nor enabled globally. Go LookupVault treats every LookPath error as
provider absence before invoking the vault runner. Denied/invalid provider
results and a failed supervisor remain their existing fail-closed errors.

Windows uses PATHEXT order/default extensions, empty-list behavior, native PATH
units and implicit cwd unless NoDefaultCurrentDirectoryInExePath exists (even
empty). It remembers relative candidates and permits a later absolute candidate
only when Go's Lstat identity permits it. Canonical path strings would miss
hardlinks and confuse symlink/reparse identities. Target-Windows-only
same-file=1.0.6 provides safe file IDs from a read-attributes handle opened with
OPEN_REPARSE_POINT/BACKUP_SEMANTICS; its exact checksum is
93fc1dc3aaa9bfed95e02e6eadabb4baf7e3078b0bd1b4d7b6b0b68378900502.
The locked winapi-util0.1.11 uses existing windows-sys0.61.2; no new unsafe
product code or non-Windows dependency is introduced. The same-file package is
already pinned by Brain, but that does not substitute for the native SDK tests.

PATH and explicit provider paths never round-trip through lossy UTF-8. In
PATHEXT only, Go converts its Windows WTF-8 environment string through
strings.ToLower: invalid UTF-8 consumes one byte per replacement, and simple
Unicode15 case mappings do not use Rust's different full/newer expansions.
The small187-range lowercase data is extracted from the pinned SDK with source
SHA receipt and the SDK's complete Go BSD notice retained beside the data. It
affects only this Windows extension field. Raw unpaired PATH
units, raw PATHEXT units and Lstat hardlink/symlink controls are required native
observations, not source-only proof.

The new actual default-CLI gate retains the five original Linux inputs per
binary, plus search-order/permission/directory/explicit opt-in observations.
Every autostart owner is identified, sent its own stop request and waited for
before its root is deleted, including the actual false-owner child control.
Darwin fixtures append an owned absolute security-exit44 path; their exact PATH
values are recorded as platform-appropriate observations rather than called
the original Linux empty-PATH inputs. Windows additionally records extension
order, implicit cwd/same-file, hardlink/symlink, unpaired PATH and PATHEXT.
The public-AB provider records complete argv0 and native GetCommandLine UTF16
vectors independently of its get arguments and executable-owner path. Product
code performs no unsafe argv rewriting. Windows safe absolute program ownership
does not prove bare argv0 equivalence; the raw distinction needs explicit
independent/native assessment before merge. Rust's automatic .bat/.cmd dispatch
is additionally blocked for a bare name selected by the scoped lookup: report
Go's failed CreateProcess fork/exec context and real Windows193 message, rather
than silently launching cmd.exe. Explicit paths and standalone dispatch retain
their existing behavior. Two actual pinned-SDK/native public-resolver script
vectors must compare the complete cause and verify no owned shell marker exists;
static SDK/API inspection is not a native Windows runtime result.

The new gate writes an incremental raw receipt after each completed CLI and
owner cleanup, before parsing provider vectors or asserting pair equality. The
receipt binds the candidate's source files and actual binary hashes, retains
complete stdout/stderr, native discovery environment, raw query ledger and
cleanup errors, and starts with complete=false. Complete is set only after all
pairs and the actual false-owner control pass. CI preserves this receipt even
if the summarized gate fails; a failing first native case must not disappear
with its temporary directory or a final-only report.

All original209+16/310 tests, key31, process63, Registry510, MCP13, owned lifetime
controls and original three raw API paths remain required. The current source
checkpoint is preparatory only: no compiler/target/port/runtime allocation has
been used. A fresh allocated target, complete process/control gates, different
author review, six real native CI lanes and full772/release acceptance remain
open. Doctor's separate c672 path-only checkpoint is unchanged and uncompiled.

## Lexical candidates and scoped GODEBUG suffix correction

The full independent source review of clean publication48fc/source64a found
two P2 defects before any new compiler or runtime allocation. Retain the exact
review, receipt and inert filesystem/source projections under
`migration/evidence/browse-state-key-772/discovery-request-48fc`; these are not
compiled Go/native observations. The original source and all14 prepared source
maps/all57 CCC raw proofs remain unchanged. The correction uses a new isolated
successor, with no modification to the reviewed worktree or executable archives.

Pinned filepath.Join cleans candidates lexically before Stat and execution.
For `/owned/missing/../bin`, this selects `/owned/bin` without requiring the
missing directory to exist. For `/owned/link/../bin`, this selects that same
lexical owner rather than traversing the symlink to a different directory.
Filesystem canonicalization would make the latter defect permanent, so it is
not an acceptable substitute. The private native-unit join now preserves the
SDK's dot/dot-dot/root rules before discovering an executable. Explicit caller
paths bypass this code and keep their exact native units; the standalone
runner remains byte-identical. Windows has separate drive/UNC/device-volume
and postClean protections and uses native GetFullPathName only to own a
relative selected executable after admission. Native unpaired units survive;
Unix raw bytes do not round-trip through Unicode. Real default-CLI owner/query
controls for both findings and relative-refusal remain required on each OS.

Read the complete pinned internal/godebug.parse, Setting.Value and
internal/bisect.New/Stack/Hash implementation. The last exact setting wins;
the first # splits text from pattern. An empty suffix produces a nil matcher.
An invalid suffix also produces a nil matcher because Go deliberately ignores
bisect.New errors. Quiet universal patterns, including qy/qn and complete
suffix partitions such as q0+1, have an exact stack-independent owner decision.
The private parser classifies the full SDK pattern grammar using a low-bit
suffix trie, not a sampled collection of invented stack hashes. It retains
modifier/negation/order/hex/64-bit limits and nil-matcher semantics. GODEBUG
is read only when relative/implicit-cwd admission needs Setting.Value; a
conditional setting does not break an unrelated absolute Unix provider.

Go's conditional bisect decisions hash at most16 actual Go runtime caller PCs,
normalized to their first PC, and optionally print real Go marker/stack reports
with deduplication. A Rust stack, a constant hash, frozen symbol addresses or
an unrelated helper stack cannot reproduce those identities. For text0
patterns requiring a conditional stack decision or an actual Go marker report,
the owned native provider now returns an explicit failed-provider diagnostic
before invocation. It does not silently choose provider absence, environment
fallback or universal opt-in. This is an **unported compatibility boundary**,
not Go equivalence, a waived conditional case or independent approval. Nonzero
text never admits a relative owner; its Go bisect diagnostic side effects are
also not certified. Full discovery acceptance and #772 remain open until this
boundary is explicitly resolved. Ordinary standalone users are unaffected.

The prepared typed fixture copies the exact SDK bisect package into an owned
temporary source directory, records its source/binary hash and actual typed
enable/print/Stack vectors, and compares13 supported cases with the native
public owned-source probe. Eight conditional/reporting patterns retain the
actual Go observations and verify native failure before provider invocation;
they are labelled UNPORTED rather than counted as parity. The helper's Stack
belongs to that actual Go helper, not frozen LookupVault or Rust. No helper is
built or invoked during source preparation. The main report separates
supported_matches from full_discovery_compatibility=false.

Additional prepared cases raise the default-CLI corpus to26 Unix/35 Windows
pairs; the optional five retained original pairs remain unchanged. Three
Unix/two Windows real child mutants cover cwd owner expansion, quiet-suffix
refusal, and Unix physical symlink/dot-dot traversal. The Windows batch/cmd
SDK comparison expands to six absolute, relative-opt-in and raw-wide error
vectors. The private result retains both the absolute executable owner and
the lexically discovered Go spelling, so diagnostics do not silently replace a
relative/raw failure path with an unrelated absolute text. Windows argv0/raw
GetCommandLine remains explicitly unproved and unwaived.

Windows file-ID admission still uses safe same-file handles with
FILE_READ_ATTRIBUTES and Rust's default sharing, while the SDK's lazy ID opener
uses access0/share0 and closes each ID handle sequentially. Those ACL/sharing
and identity-race boundaries are not certified by common hardlink/symlink
vectors or static flag inspection. Owned native permission/sharing/reparse
controls and a precise decision remain required; no byte-identical ACL claim
is made. Original thirty-minute Windows cancellations remain unrelated,
unresolved historical observations. No workflow timeout, fixture assertion,
core key resolver, crypto or global lookup behavior is relaxed here.

All preparation remains source-only under the compiler hold. Fresh locked
compiler/strict affected/workspace gates, all original key31/process63/
Registry510/MCP13/raw-path/lifetime controls, all new actual owner/settings/
mutation controls, different-author review and all six real native lanes are
mandatory before acceptance. The original209+16/310 counts refer to their
original immutable source, not a claim that these new unit cases have run.


The independent718 review finds a separate exact P2: accepted hex patterns such
as qxyf retain bits15 while the leading y sets their suffix mask to0. SDK
`matchResult` compares `id & mask == bits` without masking bits. This valid
condition never matches; it is not an invalid parse, nil matcher, conditional
stack decision or reporting exception. Preserve the complete request, both
projection/provenance tools and raw results (including5600 patterns/14 original
source-projection mismatches), matcher/harness/control bytes and pinned SDK
sources before correction in `discovery-request-718`. The original381/718
source, earlier48fc/CCC failures and runtime ELF archives remain immutable.

Correct only the private trie assignment boundary: if any stored bit lies
outside the suffix mask, leave the current set unchanged. Apply this to every
width below64, including width0. Width64 can contain every uint64 bit. Do not
mask away the extra bits, reject this accepted syntax, or make a token-specific
exception. Ordered additions/subtractions and inversion remain exact: qxyf
selects nothing, q!xyf selects everything, and q-xyf leaves its initially
universal set untouched. Later valid rules still win; valid mask0/bits0 remains
universal. Invalid qxy0 retains its distinct nil-matcher behavior. Two new
private units cover these combinations and impossible bits at all widths, with
a full-width satisfiable positive control; neither unit has run yet.

Keep all previously prepared contracts and add14 default-CLI owner pairs and17
supported typed settings. Totals are40 Unix/49 Windows CLI pairs,30 supported
typed settings plus the unchanged eight explicitly unported stack/report
observations, unchanged optional five originals and six Windows script vectors.
Add one real owner-control child that changes the impossible mask to quiet-all
before delegating actual CLI output, so an otherwise identical result cannot
hide a forbidden provider query. Prepared controls become four Unix/three
Windows; their execution remains mandatory. No original compare/assertion is
removed. These exact nonprinting cases are repaired rather than placed behind
an unported/full-compatibility waiver. Existing separate native Windows/raw
argv0/file-ID/conditional-reporting limitations remain unresolved obligations.

A specifically authorized offline SDK-only proof uses owned private HOME/cache,
Go1.26.7, p2 and a700MiB floor. Copy internal/bisect byte-exact, then build and
run a typed fixture without any provider or Rust candidate. Its5618 patterns
retain every original5600 finite pattern plus18 additional unique controls.
All56180 concrete enable/print observations match the separately bound SDK
predicate projection. The native source projection has zero mismatches across
all bounded suffix equivalence classes; this is a source projection, not Rust
execution. A second actually executed owned SDK copy deliberately masks away
stored bits in the predicate and produces230 incorrect typed observations,
including qxyf. Both actual executable payloads, compiler commands/logs, exact
input/output/source bytes, binary/SDK hashes and the full mutation result are
retained in `discovery-mask-prepared/actual-sdk-only`. The actual typed proof
does not invent a Go stack hash or imply native Windows/provider equivalence.
The strictly owned temporary SDK build cache is retired after proof, preserving
all actual binaries and reports. No SDK installation, frozen Go source, shared
Core/standalone runner, provider dependency or global lookup behavior changes.
Fresh candidate Rust gates and native six-platform runtime are still required.

## Native provider lease diagnostics and filename admission

Provider lease acquisition retains each confirmed process identity separately.
A failed Windows image query records its original error before further API calls,
closes the unconfirmed handle without terminating that process, and leaves earlier
confirmed leases available for cleanup. Linux image-validation failures close the
owned pidfd. Darwin uses `proc_pidpath` to check executable identity instead of
depending on truncated process-list text. Native Windows and Linux acceptance
remains required; mocked API tests are not native process observations.

The startup path fixture keeps the original ASCII, Unicode and POSIX raw-byte
inputs. Before executing the exact `raw-\xff` provider path, an owned kernel probe
records actual filename bytes, errno, directory state and cleanup. Only a complete
Darwin EILSEQ92 rejection permits an explicitly unexecuted unavailable case, with
no parity claim. Linux still requires the positive raw-byte case. The Unicode
path-corruption control must execute, and every requested input must be accounted
for in the final receipt. No product lookup or byte-preserving contract is relaxed.

## Batch providers: intended divergence from Go

Decision, 2026-10-07 (PR #801): on Windows, Rust keeps refusing a `.bat` or
`.cmd` startup-key provider found by the scoped bare-name lookup. This is an
intended, documented divergence from Go, not a parity gap. The refusal and its
text are unchanged: `symvault entry "symbrowse/encryption-key": fork/exec
<resolved path>: <Windows error 193 text>`. Explicit provider paths and the
standalone runner keep their existing behaviour.

This supersedes the premise in "Scoped successor preparation" that Go's
CreateProcess fails for a discovered batch provider. The first native run of
the script-vector stage (windows-11-arm, run 37584912332, case bat/absolute)
showed that the pinned Go SDK oracle discovers `bin\symvault.bat` and runs it.
CreateProcess launches batch files through `cmd.exe`, so the oracle reported
`lookup_error ""` and `invoke_error ""`, and the owned shell marker existed
(`shell_executed true`). Rust refused the same script and executed no shell.

Rationale: a provider is trusted with the state-encryption key lookup. Running
it implicitly through `cmd.exe` hands its arguments to cmd's own parsing. That
is the BatBadBut command-injection class (CVE-2024-24576 for Rust std), which
is why Rust std hardens or rejects batch-file arguments. The scoped lookup
therefore never adds an implicit `cmd.exe` owner, whatever Go does.

Gate binding: `daemon_provider_discovery.py` records Go's actual outcome
(lookup_error, invoke_error, shell executed) and asserts Rust's refusal with no
shell. It accepts the difference only for the allow-list
`ACCEPTED_SCRIPT_DIVERGENCE`: extensions `bat` and `cmd` by modes `absolute`,
`relative-opt-in` and `raw-wide`, six cases. Any other case or shape still fails
the gate: Rust configured or running a shell, Go failing lookup, a different
Rust error, or a different script. When Go itself fails CreateProcess, Rust
must still report the identical cause. Go's `exit status N` is not a launch
failure: it means CreateProcess succeeded and the script itself exited nonzero.
Native evidence (windows-2025, run 37596792546): bat/absolute and
bat/relative-opt-in ran (invoke_error "", marker written); bat/raw-wide launched
and exited 1 (invoke_error "exit status 1", no marker); Rust refused all three
without a shell. The cmd cases are recorded by the next Windows run.

Batch detection follows Win32 name normalization: trailing periods and spaces
are stripped before the extension check, so `PATHEXT=.BAT.` (selecting
`symvault.bat.`) or `.CMD ` is refused like `.bat`/`.cmd`. Without that,
`Path::extension()` sees "" and std would hand the name to CreateProcess, which
opens the batch file and runs it through cmd.exe.

## Launch spelling and non-Unicode configuration

Rust std launches any program path not ending in `.exe` as `<path>.exe` when
that sibling exists; Go runs exactly the file LookPath chose. Every discovered
owner not ending in `.exe` is therefore handed to std with a trailing period,
the Win32 "no extension" spelling that CreateProcessW normalizes away.
Explicit paths are unchanged. Verbatim `\\?\` owners skip Win32
normalization, so they are passed unchanged and keep std's `.exe` probe: a
known limitation for verbatim PATH entries.

`std::env::vars` panicked on any non-Unicode variable (raw-wide-path). The
configuration environment copy is now lossy only for variables configuration
never reads. Go's os.Getenv keeps a raw `SYMBROWSE_*` value (Unix bytes,
Windows WTF-8) and uses it as a path. Rust configuration stores `String`s, so
a non-Unicode `SYMBROWSE_*` value (STATE_DIR, CONFIG_DIR, CACHE_DIR,
DAEMON_LOG, UPLOAD_DIRS, ...) fails closed with a typed configuration error
instead of resolving a different U+FFFD path. XDG_* homes are read raw.
