# Author clean-source verification9603, not independent approval

Validated clean source9603d24e6749ee67e5eac1b7ddda6900af022cd5 descends from
approved8e3 PR805 and main e3db; source/helper/workflow/ADR/ledger hashes are
bound in the verification. Later commits retain evidence only. All100 current
inputs and1217 frozen Go sources/module inputs are verified against immutable
objects/current filesystem. Original Go/fixtures/lock/CoreGoText/crypto/policy/
audit/dispatcher code is unchanged; only a Go JSON utility wrapper is added
outside typed doctor decoders. No new dependency or authority is introduced.

170 ordinary Rust tests pass:105 Core/65 Guard,0fail/0ignore,13 non-doc summaries.
The separate kernel feature graph also runs41 tests/7 summaries; these overlap
and are not added to170. Both strict all-target/all-feature Clippy passes,
fmt and actionlint pass. Exact fresh pinned Go1.26.7/native runner executes:
124 old cases/121 full comparisons/3 TOML gates;63 byte-path cases/63 equality;
78 new cases/65 equality/13 explicit TOML gates;8 real output controls rejected.
All80 original inputs match;94 original inputs have91 matches/3 unchanged gates.
Original31 ordered probes retain27 equality/4 decoder gates, original4 raw and4
Unicode paths all match, and all35 Root mixed raw/HTML/JS/control pairs match.
Six real audit filesystem probes retain private modes/sentinel safety/denials;
three legacy Go unsafe-path allowances remain recorded native safety differences.

The new65 equality cases include actual healthy forms and semantic errors,
with14 confirmed baseline healthy forms newly native:9 TOML and5 anchor Unicode.
Remaining warning-only healthy Go states are explicit native gates, not reported
as decoder errors or skipped equality passes. Fifty original repeated invalid-
default Go observations remain unmodified:2 distinct reports, no exception.

The actual fresh Go binary is removed by the runner's owned-tree cleanup;
its real digest,1217 sources and supplemental entry remain in process proof.
The separately preserved immutable SDK-matched Go CLI/source is used for extra
probes; different builds/digests are not conflated. All65 baseline ELF archive
entries (including14 current author/Root tests+CLI) and33 Root retained proof
objects were byte/hash round-trip verified. Older53 ELF/raw-failure proofs stay
unchanged. The retention manifest decompresses every current JSON to its exact
original source bytes/SHA; logs/helpers retain literal bytes.

Native Linux processes run with subreaper, umask022, disposable HOME/XDG and
empty PATH. Operator configuration/credentials and real providers/MCP children
are untouched. Native macOS/Windows remain pending, Unix filename probes are
explicitly inapplicable to WindowsUTF16, Windows audit wording still scoped.
No selfapproval/GitHub write/#770/#769 closure or complete Brain cutover claim.
