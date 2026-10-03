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
60 actual CLI observations and 16 recorded raw frames per binary, eight actual
concurrent autostart clients, three actual Go-produced state files, unchanged
retained file bytes and four rejecting controls. Native exact-head receipts on
all six workflow runners remain mandatory. No observed/schema correction
completes the absent runtime state-key bridge or waives earlier Go failures.
