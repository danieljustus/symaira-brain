# Daemon state-key bridge and authenticated destructive cleanup

Status: implementation candidate for #772; independent review, all six native
OS/architecture receipts and release/default cutover remain open. This decision
extends PB-2026-09-09 without moving credentials or cryptographic responsibility
into the daemon or gateway.

## Problem and actual baseline

The independently reviewed registry/autostart parent supplies native lifecycle
and CLI behavior, but its runtime constructed every Store without a key. The
original b363 executable starts successfully in all seventeen owned scenarios,
never queries the owned provider, and cannot inspect the three encrypted
historical v1/v2/v3 files. Actual pinned integrated Go starts in ten scenarios,
rejects seven before IPC, and makes eight provider queries. Raw inputs, outputs,
file hashes and original executables remain retained under
`migration/evidence/browse-state-key-772/original-b363`; these are failures, not
approval. All six historical files and their manifest remain unchanged.

The candidate normally integrates approved registry publication b42730b,
actual main e3dbda6 (regular PR808 squash), and the independently reviewed d4cb8e
Windows fixture correction. Integration commits a274b88 and 95efd50 record the
parents. Before target reuse, all 84 actual CF CLI/test/helper ELF files were
compressed and every decompression SHA verified. The archival receipt is
retained with the original evidence; the original Brain target stays untouched.

## One provider decision and one store

Resolve existing Core `KeyResolver<SystemKeySources>` once during runtime
construction, before creating an IPC listener. Vault precedes macOS keychain,
then environment, then absence. Missing vault, missing/uninitialized entry,
empty provider result and legitimate security exit 44 can fall through. Denial,
malformed keys and bounded provider failure stop startup; they cannot silently
save plaintext. The existing Core runner owns the fifteen-second bound, output
limit and child-tree cleanup. The daemon adapts fixed public error contexts and
output modes, not key bytes or cryptographic algorithms.

Keep the resolved KeyMaterial inside one Store owned by DispatchRuntime. Both
browser save/load and state metadata/cleanup use that Store. Repeated operations
cannot re-query a provider or choose another key midway through the session.
The existing Core KeyMaterial zeroization and codec remain responsible for key
lifetime, random nonces, AES-GCM and AAD. IPC metadata contains counts and key
source labels, never cookie/storage values or encryption material.

Tests use owned provider executables, private HOME/XDG roots and public disposable
AB/CD keys. On Darwin an owned security command that actually exits44 proves
absence. A missing executable is a failure; PATH-empty is not used to pretend
that macOS keychain absence is known. Affected workspace tests prepend owned
absence providers before inherited toolchain commands and clear inherited Browse
keys. No test reaches the operator's vault or keychain.

## Authenticate destructive metadata independently of display

The first actual keyed replay exposed a previously unreachable difference.
Go removes five sorted expired files, then rejects plaintext-v3 during cleanup;
initial native Core removed all six. The exact initial observation and original
YAML quoting failure are preserved under `initial-bridge`, including a roundtrip
CLI archive. This evidence does not approve the first candidate.

For destructive cleanup, a configured key must authenticate **every** v3 body
with the exact header as AAD before trusting saved/expiry timestamps. An
untrusted key_source=none selector cannot disable that authentication. Use the
existing decrypt helper, without requiring authenticated plaintext to parse as
JSON: authentication and value decoding are separate contracts. This matches
the pinned Go readHeader behavior and protects forged selectors and timestamps.
Metadata show/load preserves existing plaintext compatibility as a distinct
non-destructive operation.

Clean processes sorted files individually and stops at the first read,
authentication or removal failure. Already completed removals remain completed;
the failing and later files remain unchanged. Expired inspection stays read-only
and has its own public error context. Actual Go/native cases cover the six
historical versions, keyed plaintext-v3, forged-none AAD, later corrupt body and
a genuine Go-generated authenticated non-JSON payload. Wrong/no-key failures and
all pre/post file hashes are retained literally. This is a bounded existing
contract correction, not an E012 exception or weaker cryptography.

## Same-name encryption downgrade remains an explicit separate decision

Actual Go Store.Save and the pre-existing native Core API both allow a fresh
snapshot with no key to overwrite an existing encrypted same-name file. The
native loaded-snapshot key-required guard remains stronger and unchanged. Do
not invent a blanket exception or silently change the shared guard in this
bridge.

The newly admitted native path emits the observed warning and state/source
attributes. Its bounded 64KiB regular/no-follow prefix read is warning-only
untrusted metadata, never authorization or authentication; Unix FIFO reads are
nonblocking and symlink targets are never followed. The warning's envelope uses
native stderr, while the Go logger's timestamp envelope is retained separately.
The API observation compares actual persistence/loaded source and requires the
warning message/attributes; it does not project away an error or assert identical
logging backends.

A silent encryption downgrade is unsuitable as a long-term default because it
can replace protected session data with plaintext. A future fail-closed rule for
fresh same-name overwrites needs its own explicit contract, migration behavior
and actual preservation proofs. It must distinguish deliberate conversion from
provider failure, keep the existing loaded-snapshot guard, and receive review;
this increment neither authorizes nor hides that later change.

## Public diagnostics and evidence boundaries

Keep the literal SYMBROWSE_ENCRYPTION_KEY identifier in its complete fixed
validation diagnostic. An exact non-value-bearing pattern exemption avoids the
generic redactor hiding the word key. Actual assignment, environment and JSON
secret surfaces still redact material. Startup text/JSON/YAML failures preserve
actual Go modes and final JSON precedence. YAML colon-containing diagnostic
strings use Go's observed single-quoted style. Two additional actual read-only
probes preserve forged-none and authenticated non-JSON first-byte diagnostic
failures from f1. Serde still decides acceptance; the diagnostic adapter quotes
only the actual already-decoded first invalid byte, using exact JSON whitespace
and the shared Go formatter. It never guesses from ciphertext or decrypts again.
These state.show frames are additive to the authenticated-cleanup layouts. Shared Core Go quoting reuses the
already scalar-digest-verified daemon implementation; no duplicate Unicode table
or Rust Debug approximation is introduced.

Windows directory-file error mapping follows pinned Go SDK MkdirAll and ENOTDIR
(ERROR_PATH_NOT_FOUND=3), using the native Windows message. Retained SDK excerpts
are source evidence only. They do not claim native Windows execution.

The additive gate has 31 Linux/Windows or38 Darwin startup pairs, repeated
six-file inspection, all raw protocol fields and retained file hashes, real
Go/Core Store API warning/persistence observations, and three actual executable
controls. Controls alter public fixture inputs in real child processes to prove
wrong environment key, wrong vault precedence and denial-to-absence cannot be
accepted. Only owned path prefixes and validated provider PIDs vary; failed
startup stdout/stderr is literal. The complete raw reports retain successful Go
policy-warning stderr too, independently of the startup/state contract.

All existing 510 Unix/294 Windows registry CLI cases, raw frames, concurrent
clients, baseline63 requests, MCP13 byte outputs, workspace assertions and
rejecting controls remain additive gates. Local Linux checks establish only
Linux behavior. Full independent candidate review, protected checks and six
fresh exact-head native receipts remain required before merging or claiming
#772/release acceptance.

### Scoped startup provider ownership (2026-10-04)

The immutable 4cd candidate passed all 31 startup observations and the existing
full gates, but an additional actual autostart probe exposed a provider orphan.
The real client returned the same daemon_unavailable envelope after 5 seconds.
At 16.5 seconds the actual Go provider was gone; the native provider was still
alive, adopted by the probe's subreaper after its daemon had been killed. Both
providers were private owned fixtures. Core's actual provider timeout is 15 s;
the fixture's 30 s sleep is a separate observation bound. The original failing
inputs, literal outputs, executable hashes, repeat with a retained executable
provider and full 4cd proofs remain in candidate-4cd.137 ELF paths / 107 unique
executables were gzip-retained and roundtrip-SHA verified before target reuse.

Choose an explicit CLI-daemon-startup supervisor and a private stdin lifetime
pipe. The existing resolver still resolves once and constructs the same shared
keyed Store before IPC. Only CLI daemon startup supplies its own executable as
a provider owner. Standalone Core sources, library defaults and key provisioning
keep the previous runner; its entire run_command function body is byte-identical
to 4cd, apart from signature visibility needed by the file move. The tracked
standalone-and-lock receipt pins that exact body and dependency metadata.

The daemon retains the pipe writer in its lookup stack frame. It passes only
the reader as the supervisor's stdin, through Rust's explicit stdio handles;
the parent writer is not passed to child processes. Daemon death closes it.
The supervisor watches EOF/read failure and cancels only its own provider
operation. Provider output remains bounded and captured privately. Fixed GET
selectors are checked before spawn, and result/status uses a separate captured
stderr channel, so a real provider exit code cannot become a fabricated absence.
No key bytes enter argv. On Unix the internal helper also refuses an inherited
process group. Public help, IPC authorization and provider authorization remain
unchanged. This channel establishes lifetime ownership, not additional authority.

On Unix the scoped runner retains the existing per-child process group and kills
only that group, including a descendant holding stdout after its provider has
exited. Cancellation polls every 20 ms in both process-wait and output-read phases.
The parent 15 s deadline first closes its writer and permits at most 2 s for owned
cleanup before terminating the supervisor. The helper has a 16 s fallback deadline;
it cannot race the parent's authoritative 15 s diagnostic. Successful lookup also
ends its scoped provider group. No global signals or process-tree search change.

Windows needs a handle for the entire owned family: taskkill cannot reliably
find descendants once their parent has exited. Use exactly pinned process-wrap
10.0.1, Windows-only, defaults disabled, std/job-object/creation-flags features.
Its safe JobObject wrapper suspends the provider, assigns the private job and
then resumes it, preventing descendants from racing assignment. The scoped owner
terminates that job on cancellation and completion, including exited-parent
cases. Existing CREATE_NEW_PROCESS_GROUP flags remain explicit. Ordinary Core
Windows subprocess cleanup is unchanged. The lock checksum and chosen features
are recorded; adding this small OS wrapper avoids introducing unsafe Rust or
reimplementing Windows process creation. Compilation/source inspection proves
only API compatibility; actual Windows handle and descendant behavior still
requires both native Windows lanes.

Reject inheriting the daemon process group for providers: a provider timeout
would then threaten its daemon owner. Reject a global signal handler, process
name/PID scan or blanket Vault behavior change: they cannot establish scoped
ownership. A client-only kill cannot cover a separately created provider group.
The private lifetime pipe plus owned process group/job handles addresses that
actual failure while leaving separate same-name providers alive.

The additive actual ownership harness tests normal success and denial, a closed
writer, an exited provider with a stdout-holding descendant, bounded helper
fallback, daemon termination, the actual 5 s autostart cancellation and a genuinely
held-writer control. A separately started same-name owned provider must survive.
It rejects invalid provider commands/references/timeouts before any lookup.
The same harness runs in all six native lanes, with portable Windows termination
and Unix signal observations described separately; Linux success cannot waive
Darwin or Windows execution. The original 31/38 key cases, original 17 baseline,
state.show closure cases,207 affected assertions,63 process requests, registry
cases and full MCP/workspace gates remain required on the final clean source.

The separate inherited root-prefix spelling observation is also retained:
--session S state list (and session/daemon variants) already fails in native b363
while actual Go accepts it. This ownership correction does not disguise that
pre-existing parser gap or claim full #772, native or release acceptance. It needs a
separate explicit parser contract and actual compatibility correction.


### Missing owner is a failed ownership boundary

The otherwise green immutable 407 candidate still classified a missing explicit
supervisor as a missing vault. An actual Core resolver probe with an owned
exit 4 provider and public environment key selected environment without querying
the provider. Its original statically linked executable, source and exact result
are retained with all 407 gates and executable archives. Treat supervisor spawn
absence as Failed; only a successfully running supervisor may report genuine
provider absence. This changes no ordinary Vault or provisioning fallback.

The real Core API probe now tests missing supervisor, malformed result with an
unknown field, honest denied provider and legitimate provider absence. Missing
and malformed owners must leave the provider unqueried and return an error;
denial must return the observed exit 4 failure. Genuine absence still permits
environment fallback. An actual owned executable control falsely reports vault
absence and demonstrates rejection of the resulting wrong environment selection;
on Darwin it separately reports the legitimate owned security-exit 44 absence.
No real keychain or credentials are queried. The probe resolves only, so it never
constructs a Store or writes state data. These five cases supplement, rather than
replace, the eight lifetime cases and all original startup comparisons. All six
native receipts and independent full review remain required.

### Preserve native provider paths through the private supervisor

Independent full review of source3db found one additional API defect: composing
`SystemKeySources::with_programs(PathBuf)` with the explicit startup owner
rejected an absolute Unix provider path containing byteFF before querying it.
The same owned executable succeeded through the supervisor's internal route;
ASCII and Unicode paths succeeded through both routes. This is a new public API
restriction, not a claim that default Go CLI PATH search regressed. The original
three paired observations, all thirty root-review proof files and the separate
current-ELF retirement map remain byte-for-byte under `review-3db-root`.

The private owned-command boundary now accepts native `OsString` arguments.
Provider paths retain their native representation; fixed public provider
references still pass the existing route validation. No UTF-8 replacement,
normalization or lossy conversion is appropriate for a filesystem path. The
standalone provider runner and public API semantics remain unchanged.

Permanent actual observations compare the public resolver probe with the exact
same provider's internal supervised route: ASCII and Unicode on every native
platform, plus byteFF on Unix. Each successful route must actually query the
owned provider with the same public reference. An actual Go child executable
corrupts the Unicode/raw path before delegating to the real resolver probe; the
gate rejects its incorrect fallback and missing provider query. This controls
path identity rather than merely mirroring the new argument type. Fresh builds,
full gates, independent review and all six native lanes remain required; source
preparation during the shared compiler hold does not constitute runtime proof.
