# FETCH-002 primp wire spike

This isolated candidate captures primp 2.0.1 on a hermetic loopback TLS/HTTP2
server and compares it with the source-bound Go/AzureTLS capture generated at
the same checked-out SHA. It records raw TLS ClientHello records, raw HTTP/2
SETTINGS payloads, raw HPACK header blocks, and ordered decoded header names.
An unconfigured primp client is the honest negative control.

Candidate mapping is explicit and deterministic: Chrome 153/Windows, Edge
153/Windows, Firefox 151/Windows, Safari 26.4/macOS, Opera 135/Windows, and
Safari 26.4/iOS for Go's `ios` profile. These are candidate settings, not a
claim that the Go and primp versions are equivalent. The test certificate and
invalid-certificate override are confined to requests to `127.0.0.1`; no
external host is contacted.

The workflow resolves one Cargo lockfile and shares it across six native
targets. It retains that lockfile, cargo-deny license/source results, the
cargo-geiger report, raw captures, and comparison report under artifact names
containing the exact GitHub SHA. A green spike job means only that the
candidate built and the observations were captured and compared. The JSON
always sets `parity_acceptance: false`; FETCH-002 remains open until integrated
transport and the migration ledger's independent acceptance gates pass.

Local invocation after a native Go capture is available:

```sh
cargo run --locked --manifest-path browse/port/spikes/primp/Cargo.toml -- /tmp/primp-capture.json
python3 browse/port/spikes/primp/compare.py \
  --go-capture /tmp/go-capture.json \
  --primp-capture /tmp/primp-capture.json \
  --sha "$(git rev-parse HEAD)" \
  --report /tmp/primp-comparison.json
```

Local Cargo builds are intentionally not part of the current verification.
