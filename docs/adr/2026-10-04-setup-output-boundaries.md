# Setup output errors at the actual write boundary

Issue #765 keeps the frozen Go installation and source ownership contract until
native cutover is proven. The fourth independent review of source `280914bc`
found two inherited output defects: JSON lost the underlying stdout error, and
actual broken pipes did not stop the native process at the failing write.
Original source, all 50 tested executables, all 91 review artifacts and the 39
author artifacts are retained with hashes and lossless roundtrip verification.

We reuse the approved Memory #758 process boundary from PR #803 through one
focused helper owned by `symbrain-cli`. `run_stdio` admits actual process stdout;
public embedded callers retain ordinary writer errors. Only an actual OS broken
pipe at that boundary invokes Go's default SIGPIPE behavior on Unix. A callback
that merely reports `ErrorKind::BrokenPipe` does not terminate its host. There
is no global startup signal change and no Core dependency on installers or Guard.
Memory's formatter and signal checks are mechanically shared, preserving its
already committed store writes and existing tests.

Setup wraps each real write rather than postponing handling until completion.
The first unsupported `symcockpit` row precedes later core installs: a closed
reader must terminate there, before those later binaries and sidecars exist.
Source setup and JSON finish retain already completed installations. No output
error rolls back committed files. For ordinary embedded human writers, the
frozen Go command ignores formatting errors and continues its existing work;
real SDK/public-library comparisons preserve that behavior explicitly.

Checked JSON completion reports command context, `encode JSON`, operation,
stdout path and the original I/O cause. The wrapper retains the underlying
error as its source. Raw byte-valued report fields, Go HTML/JavaScript escaping,
managed-owner lexical rules, provenance and installer policies are unchanged.
The normal PR #803 integration keeps its shared flag-normalization module;
the obsolete uncalled Guard fallback-flag helper is removed after the separately
approved standalone Guard cutover. No command routing is broadened.

The permanent Linux process gate keeps the 18 original complete-output/state
pairs, 24 embedded SDK writer pairs and executable negative controls. Actual
`/dev/full`, closed readers and a 4 KiB pipe with 128 bytes consumed are Linux
inputs, not evidence of Windows or macOS execution. Existing native three-OS
Setup, Doctor, Source and Memory gates remain required before merge. This slice
does not complete typed Brain configuration, historical memory migrations,
Go-independent source builds, or issue #765 as a whole.
