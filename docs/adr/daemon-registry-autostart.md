# Preserve daemon registry and client ownership contracts

Status: accepted implementation decision for the registry/autostart increment
of #772. This does not complete RUST-006 or authorize a migration cutover.

Actual observations against immutable integrated Go `dcddcef0` and the original
Rust candidate `a49b2528` found absent session inspection commands, lost daemon
error hints/details, unchanged list activity, divergent constructor roots,
and schema-1/empty-array state listings instead of Go's schema-3/null slices.
Actual Go-produced v3 state files with `cookies: null` failed Rust metadata
inspection. Keep these original failures and probes in migration evidence.
The historical `652453d` oracle and frozen fixtures remain unchanged.

The registry uses the existing Go-compatible OS cache/temp resolver, cleans
explicit roots lexically and creates directories only on ensure. Session
state and reference maps remain separate; copied reference tables cannot
mutate the registry. A restart clears in-memory session/reference state and
preserves profile files. Listing touches the selected existing session as Go
does. New Unix profile/log directories are private and log files remain 0600;
no permission change is applied to preexisting parent directories.

Ordinary CLI clients make one operation request. Adding a mandatory preliminary
status round trip changed the default CLI contract and could reject an otherwise
working request. Explicit engine/policy clients still verify before dispatch,
including requests that prohibit autostart; incompatible owners are stopped
before mutation. The existing MCP proxy's stronger status verification stays
intact. Missing-daemon inspection does not start a daemon. Real autostart uses
the resolved config's timeout/log path, the daemon argv and bounded readiness
retry; fatal startup failures terminate and reap the child the client owns.

Session/state inspection renders the original error code, message, hint and
details in JSON/YAML and the Go CLI message/exit classification in text.
State metadata uses the existing schema version 3. Go null cookie slices decode
as empty cookies while stored origins and local/session storage remain intact.
This is a decode compatibility correction, not a state rewrite. The actual
process gate verifies that metadata and cleanup do not change retained Go file
bytes or expose stored values. Empty clean results preserve Go's distinction
between raw/JSON null and the CLI's typed YAML empty sequence.

The process comparator validates owner PIDs by lifecycle phase, UTC activity
within the observation interval, and owned root prefixes before projection.
Stable authorization, policy, error and path suffix fields remain comparable.
Go API probes use an overlay, a readonly module graph and disposable files.
Actual native Linux/macOS/Windows on both architectures must execute the gate;
constructor-only cases do not claim CLI acceptance. Original Windows ARM Go
shutdown and macOS long-socket failures stay visible and do not waive CI.

## Remaining state-key bridge

Both runtime state inspection and browser state operations still construct the
core Store with no key. Consequently this increment cannot claim complete
Go-equivalent encrypted-state behavior, and #772/RUST-006 remain open and
in_progress. Connect the existing core key resolver in a separate focused
successor, verify actual Go key-resolution precedence and existing encrypted
migration data, and fail visibly when keys are absent or wrong. Do not invent
a second cryptographic system, silently convert protected state to plaintext,
or transfer vault-owned secrets through a gateway. Fresh independent review
and all six exact-head native receipts remain required for each candidate.

An additional actual process probe on candidate `dab8bdac` found that Rust Debug
quoting changes invalid-session messages: NUL/ESC use different escapes and a
combining mark is escaped even though Go prints it. The original paired failure
is retained. Session error quoting therefore uses the pinned Go 1.26.7 Unicode
15 printability observations and Go escapes, with no additional runtime library.
The generated range table is independently checked by the actual Go digest of
quoting every valid Unicode scalar. The runtime process gate also exercises all
four original failing strings. Future toolchain Unicode changes require a fresh
Go observation; changing to Rust's Debug tables would reintroduce the contract
mismatch. This quoting change affects diagnostics, not session validity or
authorization, and does not permit otherwise invalid IDs.
