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
