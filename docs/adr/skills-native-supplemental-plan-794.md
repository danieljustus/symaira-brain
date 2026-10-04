# Skills supplemental argv acceptance and native parent ownership

The immutable 152-case independent plan retains its original bytes and platform
labels. The tracked runner selects 53 Unix cases (51 required Go/native exact
pairs and two inherited global-output cases) or 112 Windows cases (98 Windows
plus 11 portable required pairs, and three inherited global-output cases).
Every selected case executes the frozen Go CLI, current native CLI and actual
native parent under separately seeded private roots. Exit status and complete
stdout/stderr bytes are compared without projections. Every execution must
retain the original content, mode, size, mtime and symlink-target snapshot.
Malformed Brain/Skills/project configuration, a malformed skill and managed
marker are seeded before preflight, so successful parsing cannot masquerade as
early rejection. The absent fallback and empty PATH prevent child admission.

The three global-output cases existed before the Skills change. They remain
strict acceptance requirements against an actual immutable native parent;
they are not waived or relabeled as Go parity. Go output is retained alongside
both native outputs. The unchanged global extraction owners `lib.rs` and
Core `output.rs` must match the parent byte for byte. Unix requires the actual
archived `faa8f12f6c77aad694003c9fa8c5e66d94919ba0` CLI and its verified
round-trip archive receipt. Windows builds actual parent
`01f41906e2e021db3c693ec97617bb701e4dad8a` on the native Windows runner.

The Windows-only CI invocation first saves the current CLI bytes, then builds
the clean parent in an owned local Git clone with the same pinned Rust SDK and
existing Cargo target/cache. The build is locked, offline and limited to two
jobs. Complete source/index/blob/native-mode maps are checked before and after.
The current CLI is restored and SHA-checked in `finally`, including failed
builds. The original 312 Windows vectors and three genuine controls execute
unchanged before this step. Current, Go and parent binaries plus full reports
are uploaded, and later CI gates use the restored current binary. A separate
target and an assumed parent output would both weaken provenance and consume
unnecessary resources, so neither is used.

Two additive controls change a real current or parent process input. Both must
produce a retained mismatch with exit 2, empty stdout and unchanged owned state;
bootstrap errors, timeouts and unexpected writes cannot satisfy these controls.
Portable preparation tests prove vector transport and snapshot sensitivity.
They do not establish Windows execution. This source-only successor requires
independent actual Unix replay and actual native Windows CI before publication
or acceptance. Product Rust, the central Go builder and original corpus/control
assertions are unchanged.
