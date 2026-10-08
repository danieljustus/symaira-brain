# Managed version timeout: retain the actual failure before choosing a fix

## Decision

Coverage job `111389667920` fails the existing Unix managed timeout test at
`error.to_string().contains("timed out")`. Add failure-only diagnostics to that
test, including the returned error's display/debug forms, elapsed time, owned
script path/size/permission mode and descendant PID marker result. Keep the
three-second production timeout, five-second test bound, original fixture,
descendant-liveness assertion and production implementation unchanged.

Assertion messages evaluate these filesystem reads only after their unchanged
predicate fails. There is no new file handle across process spawning, startup
barrier, retry, sleep or altered admission condition. The missing-marker panic
also identifies its IO error and originating probe. These observations should
make a future actual coverage failure actionable without masking it.

## Original observation and source ownership

The complete original job log has SHA256
`c10b0163fa1a1ba2b8a44ca8e87c8d4a123b1f51d8209e9f020bee75ed0bf981`.
It identifies publication `a30bba8bbd52323fa5e000207f1c391d576ac86c`, main
`5e232700bb9031fc34d4995465a4037837540abb`, and actual CI merge
`3c4a497cc4efb73c5074f9619b0e6d6e61ea5572`. The managed integration executable
is `install_tests-a2623068a0289a0a`: six tests pass and the timeout test fails;
the seven-test binary completes in 0.04 seconds. The log prints no returned
probe error. Missing coverage JSON is a subsequent consequence of the failed
test command, not evidence of a second installation defect.

This successor starts at `729184d8addbe94db7d1d172186afa4616565889`.
All managed sources, Cargo.lock and the Rust toolchain pin match publication
a30 exactly. The CI merge object is not available locally; its identity and
parents are retained from the full original log. No current CI executable
bytes are claimed from a filename alone.

## Historical executable controls

The existing coverage682 archive retains an actual managed executable from
source `5931cce3e4fe3e090b361fb3f9aca7e07b7ad49b`, with original SHA256
`6be4d378a39a4773c26ef6c8ef59a5e46eb146b21f59460b76f80d5ce292046f`.
Its gzip, ELF content, managed Cargo fingerprint and original archive receipt
were verified before execution. It contains the managed timeout test and
managed test source path, without the Skills install-test source path.

The original timeout fixture and all applicable Unix probe/spawn/cleanup
function bodies are byte-identical to 729. The move to `version_probe.rs`
adds a Windows-only resolver. Managed Cargo.toml and the Rust 1.98.0 pin are
unchanged; lock changes add hostname and its Usage consumer, outside Managed's
dependency closure. The historical installer predates the HTTP/1.0 retry
policy and seventh Unix test, so its full-suite result is explicitly six
historical tests, not acceptance of the current seven-test suite.

Exactly two authorized historical runs use strace, a subreaper, private
HOME/XDG/TMPDIR/profile paths and no compiler or Cargo target. The focused test
passes in 3.071 seconds and the full six-test suite passes in 3.071 seconds.
Both traces retain successful owned shell/sleep exec, group termination,
parent reaping and descendant `kill(pid, 0) = ESRCH`. These local controls do
not reproduce or explain the current CI failure. ETXTBSY, missing executables,
interpreter failures and early child exit remain hypotheses until an actual
failing run exposes its returned error.

Two earlier disk-floor checks stopped before any executable or strace start:
`/tmp` is a separate filesystem below 700 MiB, while `/workspace` had more than
700 MiB available. Only the owned restored executable and private fixtures
were moved outside all targets to the workspace oracle area. Both stops and
the path transfer are retained; neither is a CI-cause claim.

## Evidence and remaining acceptance

Original source/log/archive metadata and complete historical stdout, stderr,
strace files, profiles, source bindings and floor-stop records are retained
losslessly under `migration/evidence/managed-timeout-803/`. Historical ELF bytes
remain in their original roundtrip-verified oracle archive and the explicitly
restored owned copy. Original receipts are not rewritten.

Source-only checks verify unchanged assertion predicates, fixture setup and
production/dependency sources. Formatting and diff checks run without Cargo.
The new diagnostic text has not been compiled or executed. Root must arrange
an independent review and actual current-source coverage execution; the
diagnostic checkpoint neither fixes an inferred cause nor grants runtime or
native Windows acceptance.
