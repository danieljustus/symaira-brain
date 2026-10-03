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
empty provider result and legitimate security exit44 can fall through. Denial,
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
