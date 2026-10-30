# Combined usage contract v2 (local, offline)

This is separate from the frozen OpenCode fixture and OpenRouter v1 receipt.
Neither older evidence nor a schema projection approves the current source.
The historical OpenCode test binds the unchanged validation and all mutation
assertions to exact Git objects from `b807eae643c627ae31166b90176d5a0de6f4e647`.
Only its input-root entrypoint changed; the historical generator, fixtures,
independent anchors and validation predicates remain unchanged. The original
entrypoint is also runnable on that immutable checkout.

This capture is an optional local evidence tool. It is not a CI gate and no
committed anchor depends on it: `make rust-check` runs the self-contained
`usage-opencode-check` against the pinned OpenCode oracle revision, and the
Go/Rust usage fixtures stay covered by `usage-oracle-check` and the Rust tests.
Unrelated repository changes therefore never invalidate CI.

A retained v2 receipt binds clean detached source, complete pre/post input
inventories, actual Go/Rust observations, built binary hashes and every raw
artifact. Its `source_commit` is the measured source. Create a clean committed
source checkpoint, then capture to **new** retained evidence and supported
external build paths:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 scripts/usage-contract-v2/capture.py \
  "$(git rev-parse HEAD)" "$NEW_EVIDENCE" "$NEW_BUILD"
```

Do not edit old captures. Review the receipt and the actual observations, record
the reviewed `receipt_sha256` outside the repository, then verify on a clean
checkout whose tracked files match the capture:

```sh
make usage-contract-current-check USAGE_V2_EVIDENCE="$EVIDENCE" \
  USAGE_V2_RECEIPT_SHA256="$REVIEWED_SHA256"
PYTHONDONTWRITEBYTECODE=1 USAGE_V2_EVIDENCE="$EVIDENCE" \
  USAGE_V2_RECEIPT_SHA256="$REVIEWED_SHA256" \
  python3 scripts/usage-contract-v2/test_verify.py
make usage-opencode-historical-check HISTORICAL_USAGE_ROOT="$HISTORICAL_CHECKOUT"
```

The historical target requires a clean checkout at the exact original source
before invoking its unchanged generator/check. Local verification is evidence
for the measured commit only; it implies no workflow publication or
full-repository acceptance.

Evidence classes: 27 scripted OpenRouter production-report/request walks,
fresh OpenCode generator cases compared to the preserved legacy corpus,
17 synthetic fixed-calendar Grok histories, and 28 real native CLI executions
under a network-denying macOS sandbox. Inputs are synthetic, outputs are from
actual binaries. Complete Go/Rust JSON is compared without dropping history,
quota errors, null/omitted fields or measured zeros. Only the existing oracle's
snapshot acquisition-time normalization is used. No provider credentials,
user ledgers, live endpoints or paid calls are permitted. This is not live
account acceptance or a cross-platform matrix.
