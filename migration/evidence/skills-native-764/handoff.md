# Skills764 source checkpoint

Source: **aebb98b11316c8006b6284bf5f53650411dfca63**.
Branch: `issue/764-native-skills-surface` in the isolated Skills764 worktree.
Parents: main 5e232700bb9031fc34d4995465a4037837540abb, published preflight
cec81cfc0acdc9f4bb567f85fa6a27c03cb74467, reviewed render observation
4f3c0032b122c29804ccb0aea674202a5165f7a6. Normal merge 5f90b184 and original
retention 86118925 precede implementation; no original candidate was edited.

The source port removes Skills eligibility fallback and implements the six
actual CLI verbs plus eleven gateway tools. Metadata precision, per-target
installs/last-used, configuration/scope/target flags, raw argument decoding,
profiles, real render/install, discovery and forward-only Git restore share
existing Skills functions. The original raw sync parser remains unchanged.
Specific read-only marker decoding and the legacy omitted-ProjectDir MCP base
contract do not relax writer guards or other existing callers. The route
inventory and ADR record exact Go caller defaults, error order and decisions.

Completed **source-only** checks: 55 changed Rust files parse/format and each
has fewer than 400 lines; four Python ASTs parse; the native3 workflow passes
actionlint; manifests/lock parse; diff whitespace check passes. All 34 original
gzip payloads round-trip with their retained hashes, all 1217 Go source maps
still match, and all 88 matrix IDs are unique. SKL-008 retains render drift;
the duplicate raw-Windows-sync row is SKL-009. SKL-010/011 remain preparation.
The full affected source map binds 232 files, not only the new files.

The gate prepares 567 unique real process pairs: 276 CLI, 66 platform raw CLI,
42 CLI config, 162 MCP, 21 MCP config; three real changed-input controls. All
case IDs and original evidence are tracked. It preserves raw process records,
full payload/error/catalog fields, filesystem modes/directories/links/raw hashes,
validated timestamp and empty regular owner-lock distinctions, and timeout
cleanup/failure records. These are **planned cases, not passed executions**.

Candidate builds, candidate processes, Go SDK executions, ports and targets
created: **zero**. Cargo locked graph/type/strict-Clippy/tests have not run.
Neither independent approval nor Linux/macOS/Windows runtime parity is claimed.

Before acceptance, resolve/prove the open SDK conditional stack/reporting
GODEBUG boundary (the private universal matcher must not be treated as complete
SDK compatibility), exact Windows argv0/PATHEXT/BAT-CMD/Lstat identity and raw
path presentation, typed TOML diagnostics and bounded new Git error/ownership
paths. The workflow and prepared gate cannot waive these. Required fresh gates
also include original raw argv/preflight/render-drift/concurrency/resource
contracts and proper-fixture full workspace tests, followed by independent
full review. No fallback or report projection can hide a failing form.

No GitHub mutation, issue closure or release/cutover completion is authorized by
this source-only handoff. Heavy compiler allocation remains with Root.
