# Native usage credential references (#768)

`run.sh OUTPUT_JSON` creates an owned disposable checkout of immutable Go
`dcddcef0df5789123c7c9a7ebe6e01f10e941f2c`, adds an untracked supplemental test,
and executes its real provider constructors and authenticated request strategies.
It never modifies production Go or the frozen fixture tree. Synthetic private
`symvault` and `security` executables replace credential services; provider HTTP
requests receive canned 401 responses through an injected transport. No operator
credentials, system Keychain entries or live endpoints are read.

The gate requires all 44 constructor/report/request comparisons, all 110 actual
CLI stdout/stderr/exit comparisons with the Rust Go executable absent, and four
actual failing replay controls. The ordinary workspace suite marks the fresh
oracle integration test ignored; this gate explicitly executes it with
`--ignored`. The fresh replay requires its fixture and 44 cases, and cannot pass
by skipping or using absent fixtures.

On macOS the reference resolver invokes the private `security` fixture. On Linux
and Windows it must return the same unsupported-platform error as Go, even when
a private `security` executable exists. This is reference-adapter coverage; real
Claude Keychain discovery and host ACL behavior remain outside this slice.

The receipt and companion `.evidence` directory retain logs and available
partial reports on failure. CI uploads both on all three native operating
systems with a 14-day retention policy and refuses missing evidence.

This slice does not close #768 or delete all usage routing predicates. Ambiguous
credential files/JWTs, unsupported base/workspace overrides, mismatched Windows
home roots and automatic Claude Keychain discovery retain their gates.
