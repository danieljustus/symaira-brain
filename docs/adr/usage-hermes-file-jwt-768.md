# Native read-only Hermes credentials and JWT expiry

Decision recorded on 2026-10-03 for #768 under the user's instruction to choose
and document sustainable technical behavior.

Hermes/Nous credential files receive an ordered typed decoder matching the
immutable Go constructor. Field matching retains Go's case and Unicode folding,
duplicate ordering, scalar-null no-op and reused slice-element behavior. Unknown
metadata stays raw, including valid numbers outside float64 range. The decoder
normalizes invalid UTF-8 and surrogate escapes with the existing source-bound
Go JSON compatibility helper. It chooses the first exact `nous` entry, prefers
`invoke_jwt`, and treats malformed files and expired/unparseable JWTs as missing.

The file adapter neither refreshes nor rotates credentials, takes no locks, and
writes no store bytes. Typed parsing avoids accidental acceptance of a file Go
rejects because an unrelated provider has a wrong field type. Fresh evidence
compares full constructor reports and literal request credentials, and verifies
store bytes before/after both implementations.

JWT expiry uses Go's unverified read-only presence check: ordered case-insensitive
`exp` matching, pointer-null reset, numeric float64 values truncated to Unix
seconds, and RawURLEncoding's CRLF/noncanonical trailing-bit behavior. Provider
HTTP responses remain the authority on authentication. Out-of-range float-to-int64
expiry retains a narrow Go routing gate because conversion depends on native
architecture. This slice does not claim uniform parity for that remaining case.

Hermes reads use the workspace's already pinned `cap-std` directory capability,
with the same selected parent-root boundary as Go's `os.OpenRoot`. This dependency
prevents final symlink escape on platforms that permit in-root symlinks. Unix
final opens reject symlinks and use nonblocking mode; every opened handle must be
a regular file and remain within the existing 64 KiB limit.

The nonblocking special-file rejection is an explicit native safety contract.
A bounded actual subprocess probe retains immutable Go's owned FIFO timeout and
native immediate missing-credential result. Go production stays frozen; the
receipt distinguishes this safety behavior from byte-equivalent regular-file,
symlink and directory observations. Native Windows root-confinement proof still
requires its own CI execution; Unix probes are not counted as Windows evidence.

The fresh gate requires 80 constructor/report/request/no-write cases, 90 actual
CLI table/JSON byte comparisons for credential-free cases, and five actual
rejection controls. Positive provider requests receive canned 401 responses;
no real account or live endpoint is used. Native Linux, macOS and Windows CI
retain full and partial receipts/logs. Other credential-file families, Claude
host-Keychain discovery, numeric overflow and remaining routing gates keep #768
open until their own accepted ports and native evidence exist.
