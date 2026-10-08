# Remaining credential sources: Go-only baseline

`linux-go-baseline.json` contains 97 actual frozen-Go constructor/report results
and 97 successful read-only checks on clean source `a491f22`. It records the
actual Go runtime/architecture and four baseline-script hashes. Only synthetic
credential values and owned temporary paths occur in the receipt. The transport
returns canned HTTP 401 responses and opens no actual provider connection.

Reproduce with `bash scripts/usage-local-files-baseline/run.sh OUTPUT_JSON`, an
explicit Go 1.26.7 SDK in PATH, and the usual subprocess subreaper on Linux.

This receipt has no native constructor/CLI comparison and authorizes no routing
cutover. Distinct-token Go map selection reflects one run, not a deterministic
contract. Windows/macOS behavior, numeric overflow portability, host Keychain
and native bounded-reader behavior remain unproven by this baseline. See
`docs/adr/usage-local-file-contracts-768.md` for decisions and sequencing.
