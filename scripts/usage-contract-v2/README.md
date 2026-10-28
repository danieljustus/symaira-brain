# Combined usage contract v2 (local, offline)

This is separate from the frozen OpenCode fixture and OpenRouter v1 receipt.
Neither older evidence nor a schema projection approves the current source.
The historical OpenCode test binds the unchanged validation and all mutation
assertions to exact Git objects from `b807eae643c627ae31166b90176d5a0de6f4e647`.
Only its input-root entrypoint changed; the historical generator, fixtures,
independent anchors and validation predicates remain unchanged. The original
entrypoint is also runnable on that immutable checkout.

Current approval requires `accepted.json`, an independently reviewed anchor of
a separately retained v2 receipt. The receipt binds clean detached source,
complete pre/post input inventories, actual Go/Rust observations, built binary
hashes and every raw artifact. Its `source_commit` is the measured source,
not the later acceptance HEAD that adds this anchor. All measured Go/Rust and
harness inputs must remain byte-identical at acceptance HEAD. The unfiltered
Rust usage suite includes the current source-freshness and mutation tests;
missing v2 acceptance is a failure, not an ignored test.

Create a clean committed source checkpoint (acceptance may still be pending),
then capture to **new** retained evidence and supported external build paths:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 scripts/usage-contract-v2/capture.py \
  "$(git rev-parse HEAD)" "$NEW_EVIDENCE" "$NEW_BUILD"
```

Do not edit old captures. Review the receipt and actual observations before
adding `accepted.json` with `source_commit`, `receipt_sha256`, and `inputs`
from the independently reviewed v2 receipt. There is deliberately no automatic
anchor-refresh mode. Then on the clean acceptance checkout:

```sh
make usage-contract-current-check USAGE_V2_EVIDENCE="$EVIDENCE"
PYTHONDONTWRITEBYTECODE=1 USAGE_V2_EVIDENCE="$EVIDENCE" \
  python3 scripts/usage-contract-v2/test_verify.py
make usage-opencode-historical-check HISTORICAL_USAGE_ROOT="$HISTORICAL_CHECKOUT"
```

`usage-opencode-check` now owns current v2 acceptance; the historical target
requires a clean checkout at the exact original source before invoking its
unchanged generator/check. Existing CI callers must supply retained v2 evidence;
absence fails closed rather than falling back to v1. No workflow publication
or full-repository acceptance is implied by local execution.

Evidence classes: 27 scripted OpenRouter production-report/request walks,
fresh OpenCode generator cases compared to the preserved legacy corpus,
17 synthetic fixed-calendar Grok histories, and 28 real native CLI executions
under a network-denying macOS sandbox. Inputs are synthetic, outputs are from
actual binaries. Complete Go/Rust JSON is compared without dropping history,
quota errors, null/omitted fields or measured zeros. Only the existing oracle's
snapshot acquisition-time normalization is used. No provider credentials,
user ledgers, live endpoints or paid calls are permitted. This is not live
account acceptance or a cross-platform matrix.
