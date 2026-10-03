# Usage credential routing: share the bounded resolver

Decision recorded on 2026-10-03 for #768 under the user's instruction to choose
long-term technical behavior and document its rationale.

Environment-sourced usage credentials use the existing native shared secret
reference resolver. The provider wrapper contributes only `resolve ENV_NAME:`
and the source tag. This preserves the immutable Go resolver's complete error
chain, deprecated `vault://` alias, platform rejection of `keychain://`, argument
separator and deadline. It removes the credential-only reason to delegate these
sources to Go, while preserving gates for unrelated unproven sources.

The previous usage-only resolver collapsed diagnostic chains and attempted
`security` references on non-macOS systems. Keeping two resolver implementations
would let administration and usage disagree about the same credential reference.
The shared resolver maintains subprocess isolation from the independently usable
Vault service; Brain receives no master key and adds no secret store.

The shared native resolver retains its existing bounded-output and UTF-8 secret
policy. Invalid UTF-8 is rejected instead of silently replacing authentication
bytes, and oversized child output is rejected. The current Go oracle corpus
proves valid synthetic UTF-8 secrets and the recorded failure shapes; it does not
claim byte parity for arbitrary non-UTF-8 child output. Such compatibility
exceptions need their own explicit contract evidence before complete cutover.

The 2,053-line credential implementation is split into focused source fragments
of fewer than 400 lines. `include!` keeps their single private module and existing
visibility, so organizational changes do not expand the public API or introduce
state copies. Resolver, command runner, credential files, Keychain, routing and
provider constructors each have a reviewable source file.

Frozen Go and old fixtures remain unchanged. Fresh evidence comes from real Go
constructors, strategy requests with canned responses, and actual CLI children
in disposable HOME/XDG roots. CI must independently prove this candidate on
Linux, macOS and Windows before merge. Five actual failure controls prevent
acceptance of missing, incomplete or corrupted reports.

# Remaining decision boundary

#768 stays open. Typed duplicate/case-aliased credential-file decoding, ambiguous
JWT expiry, automatic Claude Keychain service discovery and other conservative
routing gates have not been accepted by this slice. Those contracts need ports
and fresh native evidence before the routing functions themselves can be removed.
No real provider accounts or system Keychain entries are needed for the current
reference-adapter gate, and their absence is not represented as a passing host
integration receipt.

## Own executable and temporary-path selection on Windows

Actual native Windows CI on published f241 (run37155540582, job111298062507)
failed before credential/provider/Hermes comparisons: rustc emits both
symvault.exe and its symvault.pdb, and the fixture copy glob selected both
for a single security.exe destination. Copy only the explicitly selected
platform executable. Keep its fixture behavior and all corpus/control assertions.
Normalize each of the three owned temporary roots with cygpath -u before Bash
path operations; convert fixture paths back to native form at process boundaries.
The complete original CI log is retained byte-identically under
migration/evidence/usage-files-windows-runner/. Repeat all three real Linux
gates on the clean corrected source; native Windows/macOS must still pass.
This fixes test ownership/path handling and does not widen production routing.
