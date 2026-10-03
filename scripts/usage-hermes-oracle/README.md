# Read-only Hermes/Nous credentials (#768)

`run.sh OUTPUT_JSON` executes the unchanged Go `dcddcef0` Hermes constructor,
actual authenticated request strategies with canned 401 responses, and actual
CLI subprocesses in disposable HOME/XDG roots. It adds only untracked supplemental
Go tests in an owned checkout; frozen fixtures and production Go remain unchanged.

The gate requires 80 full report/request/no-write cases (all 27 historical file
cases plus typed JSON, UTF-8, size-bound and JWT extensions), 90 actual CLI
stdout/stderr/exit comparisons across table and JSON output, and five actual
rejection controls. The ordinary workspace suite ignores the fresh integration
test; this gate explicitly executes it with `--ignored`, requires its real Go
fixture and complete distinct input IDs, and cannot pass with zero/missing cases.

Unix filesystem subprocess probes additionally verify final symlink/directory
rejection and retain immutable Go's actual owned FIFO timeout. Native FIFO
rejection is an explicit safety contract, recorded separately from Go parity.
Windows does not receive credit for these Unix-only probes. All native CI jobs
retain available partial reports and logs on failure for 14 days.

No operator credentials, system Keychain items or live endpoints are read. The
parent source-reference gate still handles synthetic `symvault`/`security`
fixtures separately. Out-of-range numeric JWT expiry and unrelated routing
families remain on Go; #768 stays open.
