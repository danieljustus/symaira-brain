`run.sh OUTPUT_JSON` builds the complete production Go CLI from immutable
`dcddcef0df5789123c7c9a7ebe6e01f10e941f2c` and the candidate Rust CLI, then
replays 151 local release-repair cases. The original differential corpus and
Go source stay unchanged. Existing release fixtures supply archives, publisher
failures, checksums, platform skips and fake version probes; no production
release endpoint or user credentials are used by the replay.

Additional cases cover source preservation, explicit replacement, duplicate,
case-folded and null provenance fields, malformed typed fields, invalid UTF-8,
unpaired UTF-16 escapes, Go time.Time parsing, optional module configuration and
all accepted/rejected boolean flag spellings. Every Rust process has an absent
Go fallback path. Output, exit and the complete fixture filesystem/modes must
match. Only fixture roots, download scratch names and newly created release
timestamps differ by construction. New timestamps must be UTC and within the
replay interval; existing source sidecars are compared without normalization.

The report retains process output, fixture manifests, binary digests, current
candidate state and immutable Go/candidate source hashes. Its positive replay
must return zero; failed comparisons cannot become a green receipt. Native
cargo tests separately prove source preservation with an empty PATH and no
release server, and preserve Go's config-load failure boundary without writing
binaries. `--from-source`, `--modules`, `doctor --fix` and full native config
diagnostics remain open in #765. Three current-head native CI reports are
required before this repair increment merges.

Windows version probes first require the original managed filename and then
resolve executable candidates using Go's PATHEXT rules; extensionless PE files
are not automatically accepted as current installs. Native Windows process tests
verify absence, refusal, candidate selection and timeout without Go. Release
downloads avoid pooled connections so an HTTP/1.0 missing-primary response
cannot poison the alternate-asset request. The HTTP regression repeatedly
verifies and installs the fallback archive. Failed Windows CI and the original
Linux closed-connection replay remain in `migration/evidence/setup-repair-765/`.
