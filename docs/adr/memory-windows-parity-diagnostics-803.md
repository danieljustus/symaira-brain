# Durable Windows differential and owned Go health diagnostics

Actual Windows run37181040219 checks out synthetic merge
7b2b85e58a2650f1138e654e96b21547bc9b82c9 of PR803 source
5dcac5b9a8add850b0985352ddc649b34b51f46f into main5e232700.
This successor starts at immutable PR source5dc, not at that ephemeral CI
merge; the merge object is unavailable locally. Preserve complete raw jobs
111373504111 and111373504349, pre-edit Python/workflow/Rust sources and every
1219 tracked Go source/module file with SHA/length/gzip roundtrips under
`migration/evidence/memory-windows-parity-803/original-5dc/` before edits.

The general Windows job concretely reports Go exit0 with a healthy stdio
`harness_health_initialize` row and native exit0 with `MCP initialize failed`.
The later JSON case is actually accepted by the unchanged strict HAR-007
validator, requiring healthy=true, outcome=healthy, initialize+ping and valid
latency plus complete legacy identity. It is not permission to accept a failed
human probe. The native-init job instead times out in the actual Go command
`memory list --json` after the unchanged10 seconds, before that case invokes
Rust. The prior PASS ordering suggests the empty-store case but does not
establish its case identity without a journal. The logs contain no durable
fixture state, parent phase chronology or health-peer stderr.

Source facts: both Go and native initialize probes have a5-second budget;
native shares it with ping after initialization and reports all initialize
broker error classes as the same fixed, redacted diagnostic. Go inherits the
hermetic fixture environment; native filters that environment through its
existing safe allowlist. The fixture uses the actual Go `mcp --profile-file`
process as peer. That Go gateway opens embedded memory during startup, before
its MCP serve loop; Go `memory list` also opens the memory database. SQLite
startup is consequently a shared possible phase, not a demonstrated root
cause. CPU, caching, scheduling, environment and database hypotheses remain
unproven. Neither timeout nor fixture acceptance is changed.

Durable fsync logging adds measured-process surroundings and can influence
timing; it is instrumentation, not evidence for a scheduling or performance
claim. Preserve the original uninstrumented failures and assess a newly
successful run without treating it as proof of their cause.

Add an opt-in journal in a separate module. The original full differential
suite, case inputs/setup functions, projections/assertions, child run function
and ten-second timeout remain unchanged. When enabled, bind actual Go/native
binary SHA, fsync setup/child/comparison phases, full raw parent child stdout
and stderr, timeout partial streams, and final fixture metadata to an output
directory outside the temporary roots. Rethrow all original exceptions and
preserve nonzero suite exit. The journal does not enter child environments or
filesystem comparisons. When disabled it makes no diagnostic filesystem writes.

Additionally prepare a native-Windows-only test of the actual public native
Broker Client against the same Go profile peer, in separate private roots,
first using the fixture environment and then the current native safe allowlist.
Its owned test subprocess inherits the fixture's project working directory.
It durably records spawn PID, initialize concrete Result/elapsed time,
ping/deadline remainder, captured broker stderr tail, and close/kill/drop phases.
The existing broker limits stderr to1MiB: this is not a claim of complete raw
grandchild stderr or wire capture. Full test child stdout/stderr reaches the
retained CI console. The peer is an actual Go process; no server tools/call,
operator credentials, public endpoints or paid requests are involved. Keep
initialize+ping within5 seconds and the enclosing child within10 seconds;
owned-tree timeout cleanup is diagnostic-only. The explicitly ignored test
requires allocated binaries/output and is explicitly invoked by both Windows
jobs; it is not counted as ordinary compiled acceptance.

Both Windows workflows retain the full journal and scoped test output as
artifacts even after parity fails. The diagnostic test still requires init and
ping success and propagates failure; all original full-workspace/parity gates
remain. This additional reproduction compares peer environment variants; it
does not reproduce Go's broker implementation or prove an equivalent healthy
native health report. Actual future Windows evidence must classify that boundary.

This checkpoint is source-only: syntax/format/actionlint and immutable metadata
checks are available, no Cargo/Go build, product invocation, target, API, endpoint
or port execution is allocated. Fresh exact-head native runtime, strict affected
graph, all original gates and different-author review remain required. General
CI failures and full #803 completion remain open; successful narrower native
Memory CLI evidence does not supersede the failed full gates.
