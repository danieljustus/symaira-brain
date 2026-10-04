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
