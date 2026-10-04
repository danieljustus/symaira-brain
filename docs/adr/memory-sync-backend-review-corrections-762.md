# Correct Memory sync header ownership before runtime allocation

Status: source-only correction, not compiled or runtime-approved. Refs #762.
The user delegates implementation choices; the original three-P2 independent
review is preserved before this correction, not replaced with a passing subset.

Immutable original `8a3226828d045b4307d2c3601e283d1ecfa4ce4f` remains in its
original worktree. A separate successor normally descends from it. Preservation
commit records all61 reviewer proof files, the full report, receipt and final
manifest as64 exact gzip roundtrips under
`migration/evidence/memory-sync-762/independent-8a/retention.json`. Its original
native/frozen/SDK/dependency maps, prepared cases, copied faulty source and raw
static logs remain intact. No observed compiler/HTTP failure is invented: the
three findings were independently traced in source, without runtime execution.

Remove the unregistered `serde(skip)` attribute from RelayBlob's manual wire
model. Its boolean still distinguishes nil from allocated-empty bytes, and the
existing manual decoder/encoder and regression stay unchanged. Adding Serde
derives solely to register this helper would create an unnecessary second
serialization contract; the existing owner should remain explicit. Rustfmt
parsing does not prove attribute resolution, so full compilation remains open.

Copy only the frozen client's explicit nonempty Bearer header across redirects.
Go1.26.7 creates the original-header copier before `send` clones the request and
Header to synthesize URL Basic. Each send derives Basic from the current
resolved URL only when no explicit Authorization remains. Absolute Location
references may remove or replace userinfo, whereas relative references inherit
it. Basic from the initial URL must never enter original-header copy state.
This matters even when the raw Host is unchanged. An explicit Bearer still
preempts new URL Basic until the original sensitive header is stripped; current
URL Basic may then be generated independently. No header/body is logged in
production, and no reusable secret store is added.

Preserve the SDK outer `initial.URL.Host != destination.URL.Host` decision and
sticky stripping. Already-ASCII hostname spelling is returned unchanged by
`idnaASCII`; `isDomainOrSubdomain` compares it case-sensitively, despite the same
listener being reachable as `localhost` and `LOCALHOST`. Do not lowercase these
identities. Port-only changes still permit a matching hostname, proper dotted
subdomains remain allowed, and IPv6/zone suffix tricks remain excluded. Once
stripped, returning to the original host cannot restore the original Bearer.
Unsupported non-ASCII/zone URI families remain refused before any actual send;
this bounded helper does not claim unproved IDNA equivalence.

The focused Usage-independent helper stays inside `memory::sync::http` and is
used by the concrete request path. It has no Core/Brain dependency or authority
change. Store/cursor/oplog/runner, JSON/crypto, deadlines/body/Referer, request
limits, dependencies and lockfile remain unchanged. Actual workspace SHA2 is
0.10.9; HMAC0.12.1/PBKDF2.12.2 use Digest0.10.7. The independent review found no
SHA2 compatibility failure, and this correction does not alter that graph.

Two new prepared tests cover literal ASCII host/subdomain/IP boundaries,
port-only delegation, sticky stripping, absolute/no-userinfo/changed-userinfo,
relative inheritance, raw percent-decoded Basic bytes and Bearer precedence.
They are source-stage tests, not executed evidence. A separate64-case plan
crosses16 owned loopback redirect recipes with all four public client methods;
three intended real-process mutation controls target case-folding, cached
initial Basic and restoring a stripped header. All header expectations are
pinned-SDK source projections. Future allocation must materialize raw actual
Go/native requests, complete bodies/responses, readonly owned HOME state and
intended failure assertions; a URL-policy/bootstrap error is not a control pass.

Original230/four controls and additive98/four controls remain byte-identical;
neither plan is replaced or shortened. Original14 plus new2 unit functions and
all original/new process definitions remain uncompiled/unexecuted. Existing
historical source inventories and verifier remain immutable; verify them in
the original8a worktree. The successor verifier binds this source separately
and verifies inherited files against their old Git blobs and dependencies.

Native Run still refuses userinfo-bearing remote identity before cursor access
to avoid the frozen literal-remote persistence/output behavior. Standalone
client-only synthetic Basic probes do not waive that refusal. No HTTP501 or
CLI fallback seam is opened. Nullable-memory panic policy, destructive
tombstone continuation, equal-clock relay pagination, outer relay identity,
malformed diagnostics, HTTP2/proxy/TLS, Store historical schema and all earlier
protocol boundaries stay pending. There is no silent behavioral fix or new
approval/capability policy.

No Cargo, compiler, target, process/HTTP/crypto endpoint or port is allocated to
this correction. Standalone source formatting/hash/AST checks are the only
available local verification. Require different-author full source review,
allocated full Memory/CLI/MCP/parent compilation/strict checks, every original
and new materialized protocol/state/control case, native Linux/macOS/Windows
and full independent runtime review before any admission or #762 closure.
