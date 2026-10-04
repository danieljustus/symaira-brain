# Bounded native Memory HTTP owner (#763)

This is a complete positive **owner/API slice**, not admission of the full
`symbrain memory serve` command. The actual example constructs production
`http::Server` on the same `Arc<Store>`, owns its loopback listener and signal
lifecycle, and never starts Go. The existing CLI keeps the complete serve route
on Go until the remaining configuration, sync, rotation and policy contracts
are proven. No UI asset-only listener, alternate store or master-key store is
introduced.

On the owned Linux workspace, build with the explicitly assigned target:

```sh
CARGO_TARGET_DIR=/workspace/symaira-memory763-ui/target \
CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 \
CARGO_BUILD_JOBS=2 cargo build --offline --locked -p symbrain-memory --example http_probe
python3 scripts/memory-http-oracle/replay.py \
  --native /workspace/symaira-memory763-ui/target/debug/examples/http_probe \
  --report /tmp/memory-http-proof.json
```

The direct runner verifies the approved immutable Linux Go executable by SHA, executes
its actual governed CLI to create a fresh complete schema and real 768-vector
row against an owned OpenAI-compatible embedding peer, and seeds deterministic
current rules/entities/profiles. SQLite's backup API copies committed WAL state;
nonempty memory/entity assertions prevent empty-data false positives. Go's real
`memory serve` and the real native owner receive separate owned HOME/XDG/cwd,
literal synthetic JWT keys, PATH empty, and absent Go fallback paths. All
request/reply bytes, headers, actual key/argv fixtures, embedding requests,
SQLite state bindings and shutdown observations are retained.

The 52 paired observations cover exact HTML, CSS whitespace preservation,
intentional JavaScript bug corrections, protected reads/public status, Host and
CSRF ordering, CORS/preflight, JWT verification/expiry/issuer/typed claims,
profile write denial, nonempty list/rules/entities, Get access feedback, nested
nonempty search, direct set/delete and audit identity, bounded JSON and revoke.
Comparison preserves status and selected protocol/security header semantics and
parsed JSON values; complete raw wire bytes remain visible. It does not certify
header ordering, Date, chunking/Content-Length, Range, redirects or byte parity
for the intentionally corrected JS/CSS. Per-process UUIDs, wall-clock timestamps
and version have narrow explicit normalizers with identity/time validation.
Seven native-only unsupported operations must return 501 before any application
state mutation, including unsafe persisted Get/search data. They are explicit
limits and never count as Go/native parity.

`--control wrong-secret` and `--control empty-state` run the actual native owner
with a deliberately wrong owned key or missing seeded memory. The full replay
must fail its report comparison. These are real negative executions, not edited
reports or precomputed output doubles.

For an actual DOM/JavaScript/network runtime, install **jsdom 26.1.0 only in an
owned test directory**, then run `dom.py --native <binary> --node <node>
--jsdom <owned node_modules/jsdom> --report <new path>`. It exercises authentication,
list/scope, nested search/score, rules/entities and actual add/delete against the
native backend. It also executes the unchanged frozen original script to
reproduce public-status false authentication and the nested-search exception.
No graphical rendering, browser CSP enforcement, screenshots or native Windows/
macOS acceptance is claimed. All resources and fetches are constrained to the
owned loopback origin.

Use a new report path for every execution; preserve failures and earlier evidence.
The original Go-only 13 CLI / 17 HTTP inventory stays separate. Native three-OS
execution, complete serve admission and independent review remain pending.

`run.py` is the portable native-CI entrypoint. It archives unchanged `dcddcef0`,
builds its actual command with native Go 1.26.7 in an owned build HOME, binds the
new executable/source hashes, then runs the positive HTTP gate, both actual
negative executions and the DOM/original-defect gate. It records native OS/SDK,
module and binary identity. Windows peers get their own process groups and an
actual CTRL_BREAK signal; Go maps it to Interrupt and the native seam selects
Tokio ctrl_break/ctrl_c. This is CI wiring, not proof of an unexecuted Windows
signal or console environment. The three-OS `memory-http-native.yml` job must
actually pass on the final source before the slice's platform gate is green.
