# Memory sync oracle — prepared source stage

No runner, native binary, Go probe or listener has been executed for this
checkpoint. `cases.json` is a source-derived test plan, not a result fixture.
The native CLI still delegates real `memory sync` invocations to Go. The
canonical model codec is not yet the complete Go JSON wire decoder; neither
an HTTP transport nor a concrete encrypted-relay codec is claimed complete.

`go/main.go` is a supplemental probe importing actual frozen public database,
sync client and crypto APIs. It must be copied into a new private package in
an owned, unchanged copy of `dcddcef0`, verified before and after the build.
The original source and its existing tests must remain unchanged. Build only
after resource allocation, with the pinned Go1.26.7 SDK and owned module/cache
directories. Direct probe observations do not replace actual CLI process
observations, HTTP transcripts, database states or native Windows/macOS CI.

Every future case needs private HOME/XDG and a fresh synthetic JWT. The fake
sync server binds an ephemeral loopback port and records raw method, path,
query, header bytes, body bytes, connection order and response bytes. It must
never bind default Ollama11434 or use operator credentials/remotes. DB copies
and raw state queries are owned fixtures. Clock/UUID variation must be bound
to the exact application step, not dropped from comparisons.

Required evidence includes the original input and full raw failure even when
a correction is needed; actual source/binary/module/SDK SHA manifests;
all old Memory CLI/DB/MCP parent cases with their historical receipts retained;
complete case accounting; process exit/stdout/stderr; cursor/oplog/audit/entity/
evidence/FTS state; four substantive mutation controls; and different-author
review on the immutable final source. Native three-OS execution is required.

The case plan distinguishes canonical wire cases, actual Go quirks, malformed
wire families and security/HTTP admission boundaries. Admission cannot be
opened by a stub transport, a crypto-free identity codec, one passing unit
test, zero cases, skips, cross-compilation or a source-reference algorithm.

The successor source checkpoint adds concrete ureq HTTP, ordered typed Go JSON
and pinned RustCrypto AES-GCM/PBKDF2 backends. See
[backend decision](../../docs/adr/memory-sync-concrete-backends-762.md) and
`backend-cases.json`. The original230 cases and four controls above are retained
byte-for-byte. The new code and prepared tests have not been compiled/executed;
no route has been admitted. The opening description records the original c4
foundation, not a claim that the successor has passed runtime acceptance.

`go-backends/main.go` adds prepared public-client operations and typed SDK
response decoding over actual exported frozen types, plus a process-owned
synthetic crypto sequence/entropy override. It neither substitutes frozen code
nor proves transport via decoder-only observations. `backend-cases.json` adds
98 specific JSON/HTTP/crypto plans and four intended controls; all remain
unexecuted. Run `python3 scripts/memory-sync-oracle/verify-backends-static.py`
for source/lock provenance only. It does not compile, launch a probe, test a
 cipher, bind a port or establish acceptance.

The isolated review successor preserves the complete8a three-P2 source review
and all61 proof files before correcting model helper registration, ASCII host
identity and URL Basic/header-copy ownership. See
[correction decision](../../docs/adr/memory-sync-backend-review-corrections-762.md).
The original verifier above binds the immutable8a source; run it with `--root`
pointing to that original worktree. Use `verify-corrections-static.py` in the
successor for its separate source/dependency/preservation map. Neither command
builds or executes a product. `redirect-correction-cases.json` separately plans
64 full public-client comparisons and three intended mutation controls, with
all original230/98 cases and eight control definitions untouched. These and
the two new unit functions are prepared, uncompiled and unexecuted. Native
Run's userinfo refusal and CLI/HTTP admission gates remain unchanged.
