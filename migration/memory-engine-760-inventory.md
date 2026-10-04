# Memory engine #760: source census and remaining full port

Base: published Memory803 `a30bba8bbd52323fa5e000207f1c391d576ac86c`.
Actual issue: <https://github.com/danieljustus/symaira-brain/issues/760>.
This is a source census, not an actual-process compatibility report.

| Go owner | Actual production reachability | Native prerequisite / remaining work |
|---|---|---|
| extractor | CLI set constructs EmbeddingsGenerator and MemoryService; service/prep Store calls PatternExtractor and embeddings. CLI search constructs embeddings. MCP NewServer constructs the same service. Importer Registry composes patterns and summaries. | Existing `embedding.rs` uses shared LLM one-shot request + local hash fallback. Native CLI admission refuses potential secondary-fact patterns. New fixed PatternExtractor reuses #758 evidence; raw-source bridge, governed secondary writes, cache/cooldown/metrics/cancellation remain. |
| consolidation | `NewEngine` has no constructor caller in non-test cmd/internal. Working Evictor receives an Engine; importer adapters set prompt-family metadata. Exported Engine and undo journal contracts still apply. | No native Engine. Port scope grouping, partial scope failures, one-row consolidation, prompts/schema/JSON salvage and validation, injected UUID/time, per-scope atomic save/archive/evidence reparent, association/entity/provenance handling, journal and undo. Actual Go map iteration requires evidence before any order projection. |
| aging | Library `Run`, DefaultConfig, FromConfig and DecayFactor; bench calls DecayFactor. No application aging pass caller in frozen cmd/internal. | Prepared exact-input curve. Native Store candidates/update/retire/unretire, full-row scan errors, staged-row inclusion, no deletes, dry-run, per-row partial commits and elapsed report remain. Preserve SQL constraints/defaults owned by #649. |
| summarizer | Importer Registry extractFacts uses limit 5. Context Assembler ProduceSessionSummary uses limit 5; the Assembler has no application constructor caller. | Prepared byte-valued algorithm. Registry source/session metadata, sanitizer, redaction, content-length and evidence ordering remain. #761 owns importer DTOs, not a duplicate summarizer. |
| contextassembler | Public library and tests; NewAssembler has no application constructor caller in frozen cmd/internal. | Prepared default token estimator and hard budget only. Full assembly priority/degradation, injected tokenizer, working memories/turns, summary/pinned/profile/session layers, temporal retrieval, snapshots/deltas/eviction, expandable sources, recall receipt minting/validation/redaction and profile inference remain. |
| entity | db ResolveEntityCandidates calls Normalize/IsPII/BestMatch; MCP entity_resolve calls candidates. Memory writes and other entity tools also use name/alias resolution. | Native entity.rs lists/relates/graphs with separate simplified resolution. Port Unicode NFD/diacritic/lower/fields semantics, PII rejection and rune limits, ordered match strength/reasons/ties, type filtering and limit. Do not substitute SQL LIKE or create a different entity owner. |
| temporal | MCP memory_search calls extractTemporalFromQuery → temporal.Extract when configured. Explicit bounds have separate parsing/admission. | No native extractor. Preserve ordered EN/DE patterns, ASCII word/whitespace classes, named UTC calendar boundaries, approximate relative durations, Go64 numeric/duration overflow and nil matches. Wire alongside explicit-bound priorities, config and search filters. |
| working | Exported Evictor gets database, embeddings and Engine through constructor. No application NewEvictor call in frozen cmd/internal. | Native writes persist working tier/expiry and reads filter expiry, but no full compaction. Port zero/one/many expired cases, dry-run counts, single-row delete, consolidation error-before-eviction and configured context inclusion. |
| conflict | CLI set and MCP NewServer construct Checker when config enables it. Staged writes exclude checking; memory prep Store invokes Checker and then handles resolutions. | Native CLI routes nonstaged conflict-enabled writes to Go. Preserve exact active hash duplicate, LSH/cosine candidate namespace/order, near-duplicate short-circuit, bounded contradiction band, LLM verdict parser/failure degradation, actor policy winners and audits. Existing error means no decision, not a failed write. |
| discovery | Exported metadata-only Scan takes Providers; no cmd/internal application Scan caller. Provider adapters currently belong to importer/library contracts. | No native Scan. Preserve provider failure continuation, stable logical source IDs, group/provider ownership, last duplicate reference, raw tool/session/path key, zero-time fallback, UTC RFC3339, empty suppression and tool/source sort. #761 supplies SessionRef/discovery DTOs; #760 owns composition. |

The complete function census is
`migration/evidence/memory-engine-760/go-api-census.json`; imported-call anchors
are in `production-callsite-census.json`. Constructor absence is scoped to
tracked non-test `cmd/` and `internal/`, not a claim about downstream library
clients. No CLI import/aging/consolidation/context command is invented.

## Public native consumers which must ultimately share this engine

- CLI `memory set`: `memory_cli/write.rs::direct_write_supported` currently
  refuses nonstaged conflict-enabled writes and secondary-fact triggers. Keep
  these predicates until governed write/evidence/audit/sync/error proofs pass.
- CLI search: existing embedding/retrieval pieces cover prior accepted one-shot
  behavior; persistent cache, failure cooldown and metrics are additional
  observable Engine contracts, not proved by one-shot CLI gates.
- Embedded MCP: `symbrain-gateway/src/embedded/memory.rs` dispatches set/search,
  entity_resolve and other memory tools directly to the Store. Its simplified
  paths do not establish parity with Go MemoryService/config/security/engine
  constructors. Reuse the same complete service instead of adding another
  extraction or conflict implementation in the gateway.
- Importer Registry: interface *presence* selects Transcript handling. No
  application Registry constructor exists in the frozen tree. Library intake,
  sanitizer/redaction, summary, stored provenance, embedding and evidence
  contracts are still required. Coordinate only the shared bridge with #761.

## Full validation plan after resource allocation

1. Compile strict affected Memory/Gateway/CLI plus prepared engine probes in one
   allocated target; keep all original Store/Evidence/CLI/HTTP/schema proofs.
2. Build actual immutable Go package tests in disposable owned source trees,
   adding only the retained helper. Verify original file/tree/mode/SDK/dependency
   hashes before/after, and bind helper, probes, input vectors and executables.
3. Execute every prepared deterministic case; preserve initial mismatches and
   full streams/state. Inspect Unicode/NaN/calendar/overflow/order boundaries
   with real Go processes before expanding any compatibility claim.
4. Execute genuine changed-input, zero-selected-test and write-failure controls
   and verify nonzero comparator outcome. No hardcoded result substitution.
5. For complete Store/service port, use fixed IDs/clocks and local fake LLM/HTTP
   replies: success, malformed schema/JSON, null/case/duplicate fields, injected
   adversarial content, 4xx/5xx, bounded stalls/cancellation, partial scopes,
   rollback, partial committed effects, namespace and all audit/evidence rows.
6. Native Linux/macOS/Windows package and actual CLI/MCP gates are mandatory.
   Source-only preparation or cross-compilation cannot close #760.
