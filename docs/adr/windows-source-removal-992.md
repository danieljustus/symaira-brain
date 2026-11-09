# Windows-only source removal (#992)

Status: accepted 2026-11-09. Supersedes the Windows runtime, test and fixture
requirements in earlier ADRs (for example `0008-windows-probe-pipe-deadlines.md`,
`2026-10-03-doctor-windows-compat.md`, `guard-windows-*-805.md`); those keep
their original meaning as historical records.

Scope follows AGENTS.md "Supported Platform Scope" (#990): macOS on Apple
Silicon, iOS and Web are product targets; Linux stays for CI/server/CLI. Native
Windows is unsupported.

## Removed

- Every `cfg(windows)`, `cfg(not(unix))` stub, `cfg!(windows)` and
  `target_os = "windows"` branch in `rust/` and `browse/crates/`, together with
  the Windows-only modules (named-pipe daemon server, job-object process
  wrapper, Windows startup discovery, Windows metadata/raw-append, Windows path
  models) and tests that exercised only those branches.
- Windows-only dependency declarations: `windows-sys`, `interprocess`,
  `widestring`, `same-file` (cli/core), `process-wrap`, `hostname`. The lock
  files lost the matching entries (deletions only).
- `scripts/guard-standalone-oracle/windows_path_reference.py`.

## Retained on purpose

- Sealed fixtures and evidence, including
  `rust/symbrain-harness/tests/fixtures/install_oracle_windows.json` (registered
  in the oracle-freeze manifest). Only the loader branch for it was removed.
- `scripts/guard-standalone-oracle/windows-sdk-{source.json,LICENSE.txt}` as
  historical provenance.
- `browse/port/harness/windows_pipe.py` and `test_windows_pipe.py`, until the
  `browse-daemon-native` workflow stops invoking the latter.
- Third-party crates' own Windows-target entries in the lock files.
