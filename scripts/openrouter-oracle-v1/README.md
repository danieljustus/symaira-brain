# OpenRouter executable oracle v1

This separate generator executes the production `NewOpenRouterProvider` and
`BuildReport` using scripted, ordered HTTP responses. Inputs are authored test
scenarios, not provider captures. Outputs are actual Go-process observations.
No credentials, provider traffic or paid model calls are used.

`capture.py FULL_SOURCE_COMMIT NEW_EVIDENCE_DIRECTORY` requires a clean caller,
checks ancestry, creates a retained detached clean checkout, builds the versioned
generator with Go 1.26.7, records commands/stdout/stderr and binary/source identity,
and runs twice to require deterministic bytes. Only `snapshot.fetched_at` is
normalized to Unix epoch. Requests retain every header and body; reports retain
all auth, snapshot and error fields. The raw receipt is retained outside Git.
The fixture/receipt acceptance commit is necessarily later than the captured
source commit: they are not interchangeable identities.

The original generators, provenance validators, fixtures and digest anchors are
historical evidence and must not be refreshed by this generator. Historical
OpenRouter report assertions run through an explicitly scripted `/credits` 404
before the unchanged legacy response, not by sending legacy data as credits.
Current acceptance directly compares every Rust report and request with v1 Go
observations. It does not certify live endpoint parity or real HTTP deadline
behavior; existing transport/deadline tests are separate gates.

## Accepted local source and retained evidence

Captured source: `f0fb64902ba55cc3bc046af992df9ee2e81ebfb6`.
The 27-case raw output SHA-256 is
`cfe8edcb7f1e3facc8fed10801e92e0ad6fee8177d914e5268bd47931b24dbb1`.
`verify.py` independently anchors the raw receipt and capture, checks the retained
clean detached checkout and executed binary, compares current Go build inputs
with the captured revision, then requires the exact named Rust test to execute
once and compares its saved observations to all Go cases. Default Rust tests
embed a crate-local exact-byte copy and enforce the same capture digest. This
is a reviewed capture anchor, not resistance to an attacker changing both code
and anchors. Synthetic controls are separate from product-test totals.

From the assigned worktree, with the retained capture available:

```sh
export OPENROUTER_V1_EVIDENCE=/Volumes/1TB_NVMe_SN850X/Dev/evidence/symaira-cockpit/openusage-parity/meters/current-contract-v1/capture-immutable
PYTHONPYCACHEPREFIX="$HOME/.hermes/cache/scratch/openrouter-pycache" python3 scripts/openrouter-oracle-v1/test_verify.py -v
# Each run needs a new output basename; raw observations are never overwritten.
python3 scripts/openrouter-oracle-v1/verify.py "$OPENROUTER_V1_EVIDENCE" differential-final
```

The capture fixture is privacy-reviewed synthetic account data and an invalid
oracle-only credential. Raw command logs, failure diagnostics, source checkout
and binary remain external. Two earlier launcher-context failures are retained
as failures; neither is treated as a capture. Seven pre-existing ignored crate
tests require separate fresh credential/native evidence and remain unverified.
No push, merge or release is part of this local acceptance.
