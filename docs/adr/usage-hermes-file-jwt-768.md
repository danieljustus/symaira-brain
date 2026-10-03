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

The fresh gate requires 92 constructor/report/request/no-write cases, 100 actual
CLI table/JSON byte comparisons for credential-free cases, and five actual
rejection controls. Positive provider requests receive canned 401 responses;
no real account or live endpoint is used. Native Linux, macOS and Windows CI
retain full and partial receipts/logs. Other credential-file families, Claude
host-Keychain discovery, numeric overflow and remaining routing gates keep #768
open until their own accepted ports and native evidence exist.

The independent review of immutable `273a93c0` found one token-selection
regression: a nonempty duplicate `providers` array could shrink and subsequently
regrow. Go keeps the hidden backing slots, whereas Rust's `truncate` destroyed
them. The original actual failing replay is retained in
`migration/evidence/usage-hermes-768/linux-slice-regrowth-failure.json`.

The correction models visible length independently of retained visited slots.
Selection sees only the visible prefix. Nonempty shrinkage preserves hidden
values for regrowth, including null/object entries and scalar-null no-ops; both
null and an empty array reset retained slots. This follows the pinned Go SDK's
`encoding/json` slice decoder and `reflect_growslice`, which preserves the old
capacity even on reallocation. Rust allocator capacity need not match Go:
previously unvisited slots are always default values in either implementation.
Twelve fresh report/request/no-write cases cover repeated shrink/regrowth,
hidden-tail exclusion, clearing fields, null/empty resets, preferred invoke
credentials and growth beyond previously visited slots. Frozen fixtures remain
unchanged; the original eighty supplemental cases are retained verbatim.

## Integration decision

Publish the independently reviewed Hermes correction in the existing #804 Usage
PR together with the independently reviewed credential-reference seam. Both
changes resolve Usage credentials and share the same bounded read-only routing
contract. Normal integration retains the published parent and both original
reviews; the fresh Hermes gate also executes the complete parent-reference gate.
One final combined native three-OS CI avoids treating two overlapping credential
PRs as separately validated integration states. No force push, review bypass or
full #768 closure follows from this grouping. Provider-file work remains a
separate unaccepted successor.

The original rejected `273a93c0` and corrected scoped approval `9b37c53f` are
retained under `migration/evidence/usage-hermes-768/`, including thirteen
independent regrowth inputs and full actual-process receipts. The current
publication only integrates the parent's review evidence and this documentation;
reviewed production and comparator bytes are verified unchanged.
