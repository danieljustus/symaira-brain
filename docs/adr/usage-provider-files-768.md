# Claude and Codex credential-file migration (#768)

## Work-in-progress checkpoint, 2026-10-03

The initial Claude/Codex changes in this branch are an unvalidated checkpoint.
They introduce shared capability-rooted read-only file access and ordered raw
JSON object decoding, typed Claude map merging, and exact-key generic Codex
JSON decoding. No compiler, test, Clippy, or fresh oracle result is claimed for
this checkpoint. Existing routing tests and oracle coverage still require work.

The lane was paused while independent review found a Hermes backing-slot
shrink/regrowth bug in immutable `273a93c0`. Preserve this checkpoint before
integrating the focused `9b37c53` repair; retain the original failing evidence.
The completed implementation must keep automatic Claude host-Keychain discovery
and nondeterministic selection among distinct nondefault account tokens on Go.
It must not read operator credentials or contact real provider endpoints.
