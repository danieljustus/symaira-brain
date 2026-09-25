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

## Four-target diagnostic (PR head `e233f863`, 2026-09-25)

The exact-head workflow `36100705048` completed successful raw capture and
comparison on Linux x64, Linux ARM64, Windows x64, and Windows ARM64. Both
macOS jobs were cancelled when the PR advanced. The retained artifacts are
bound to GitHub's synthetic merge tree `a20c61fd338fb929dd9afcd7a0857bd14b1c98e5`
(the PR head was `e233f8634ba4643f99cf38719251cd0d39b9751f`). Capture artifact
IDs: Linux x64 `10849541860`, Linux ARM64 `10849730611`, Windows x64
`10849382796`, Windows ARM64 `10848699234`; shared audit `10849700355`. All
four completed targets reported `all_candidate_profiles_equal: false` and a
distinct honest default negative control. Their per-profile equality pattern
is identical:

| Go profile | JA3 from captured fields | Ordered H2 SETTINGS | Ordered header names |
|---|---|---|---|
| Chrome | mismatch | match | mismatch |
| Edge | mismatch | match | mismatch |
| Firefox | normalized JA3 match; raw ClientHello differs | match | mismatch |
| iOS | mismatch | match | mismatch |
| Opera | mismatch | match | mismatch |
| Safari | mismatch | mismatch | mismatch |

All 24 target/profile raw ClientHello records differ byte-for-byte. All
requests negotiate `h2`. Chrome, Edge, and Opera Go settings are
`1=65536, 2=0, 4=6291456, 6=262144`; Firefox settings are
`1=65536, 2=0, 4=131072, 5=16384`; iOS settings are
`2=0, 3=100, 4=2097152, 9=1`. Safari Go settings additionally contain
`8=1`, which primp omits (`2=0, 3=100, 4=2097152, 9=1`).

The Go Chrome/Edge/Opera header order is
`:method, :authority, :scheme, :path, accept-encoding, user-agent`; Go Firefox
uses `:method, :path, :authority, :scheme, accept-encoding, user-agent`; Go
iOS/Safari use `:method, :scheme, :authority, :path, accept-encoding,
user-agent`. Each primp candidate adds browser-like fields absent from the Go
capture, and none has the same ordered names. The raw HPACK block is retained;
the comparison uses the names decoded from that exact block by the spike's
Rust HPACK decoder because the shared independent validator cannot decode all
Huffman strings. Thus header comparison is disclosed but is not an independent
second decode.

JA3 here is recomputed from the parsed no-GREASE ClientHello fields, not emitted
by primp. For Linux x64, Go → primp hashes are Chrome
`bc70db521676294ec86a843895554e29` → `cdd7aa4602d3f2688478edcee7fc8efc`, Edge
`a354302df79f386dc4731fa7e1e1eb98` → `dc3e0c3d04f4d6092dc0397600b16f60`,
Firefox `3ec5d3c9a10d43b0576e31e639c83cd0` → the same hash, iOS
`5a527c775ff4ae29b4f0c77b113f9625` → `f0d84abacad964331252aaa90e9971f7`,
Opera `88ad1be7e1bf9f76e05c44006bfc050e` →
`01bdace6cd483cceb0a59f798c7ebe4a`, and Safari the same Go hash as iOS →
`0e9b6ab137db93a09105301b04fc44e4`. The Go Chrome/Edge/Opera hashes vary by
native OS/architecture; the four-target equality pattern does not. No JA4 is
computed or claimed by this spike.

The shared audit job passed cargo-deny license/source checks. cargo-geiger
completed and uploaded its report, but logged that it could not parse
`aws-lc-rs-1.18.1/util/process-criterion-csv.rs`; treat the unsafe inventory as
incomplete for that source file, not as a clean/exhaustive audit. The Go capture
validator also explicitly reports `current_source_bound_oracle_established: false`
and `historical_oracle_status: INVALIDATED`; its raw fields reparse internally,
but these receipts do not establish an accepted source-bound oracle.

These four native targets are enough to reject primp 2.0.1 as a production
FETCH-002 replacement: the profile mismatches are material and repeat across
OS/architectures. The two macOS receipts remain absent at this SHA. No
production fetch code was changed, and this spike does not establish the
separate redirect, SSRF/policy, timeout, or error-contract requirements.
