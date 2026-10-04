# Registry/autostart original evidence for #772

Original Rust source: `a49b25285e8b241f8b0636c8d2b5da164bd63850`;
production code equals `6c3bf1f93d2760b17f4f936076d26f0f4c00fe1d`.
Integrated Go: immutable `dcddcef0df5789123c7c9a7ebe6e01f10e941f2c`,
built by Go 1.26.7. These observations precede the corrections.

- `original-cli-and-registry.json` and probe retain actual session parser
  exit-2 failures, missing unavailable metadata and differing registry errors.
  Exploratory inputs are preserved; this is not a six-platform acceptance gate.
- `original-constructor-failures.log` retains the new focused registry tests
  run against original production: one passed and two failed (empty explicit
  root and lexical cleanup). New test source is in the candidate; a failing
  test added to original production is not a clean original-head CI claim.
- `original-go-state-inspection.json` records binary digests, Go API output and
  an actual raw daemon comparison: Go schema 3/null vs Rust schema 1/[];
  actual Go-produced persisted null-cookie state shows successfully in Go and
  returns `operation_failed` in original Rust. Both retained bytes unchanged.
  The paired probe and stdout remain alongside it. Its absolute locations
  describe the original run; reruns must pass their owned binary/source paths.

The new source-bound `browse/port/harness/daemon_registry.py` separately performs
108 actual CLI observations and 20 recorded raw frames per binary, eight actual
concurrent autostart clients, three actual Go-produced state files, unchanged
retained file bytes and six rejecting controls. Native exact-head receipts on
all six workflow runners remain mandatory. No observed/schema correction
completes the absent runtime state-key bridge or waives earlier Go failures.

`original-dab8-invalid-session-quotes.json` preserves the later confirmed raw
process mismatches on clean `dab8bdac`: NUL, ESC, combining U+0301 and soft hyphen.
`go-print-ranges-probe.go.in` and `go-scalar-quote-probe.go.in` observe only the
pinned standard library in disposable executions, not modified oracle source.
`go-quote-provenance.json` retains their hashes, Unicode version, 712 non-printable
ranges and digest covering every 1,112,064 valid Unicode scalar. The Rust quote
regression executes that full scalar set; the actual process gate separately
verifies all four strings through the production decoder and error path.

The independent rejected `b363762` review is preserved as
`independent-review-b363.md` / `.json`, and its 48 literal mismatching pairs are
`independent-invalid-cli-b363.json`. The original Rust executable is retained at
`/tmp/symaira-registry-b363-rust` with SHA
`91de2c212944e742e509bcc805136e90e98e2bda14c917e2bd553823a868a808`.
The CLI correction adds these 48 actual pairs to the permanent gate: 108 total
CLI observations plus 20 raw frames per implementation, with six receipt mutation
controls. Raw protocol invalid_session remains distinct from CLI internal/exit1.
