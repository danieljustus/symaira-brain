# Diagnose source-setup timeouts before changing their budget (#806)

Status: bounded independent Linux acceptance at source7ea3cfb; native Windows cause and the prepared JobObject experiment remain unverified.

The first independent journal execution at `9faee8b` failed before fixture
construction, compiler or CLI launch: Python exposes `NamedTemporaryFile`, while
the draft used the Rust-style `NamedTempFile` spelling. Preserve that actual
AttributeError, the original source/hash and Root's full invocation record in
`original-9fa-api-failure`. The saved shell driver is explicitly a retrospective
exact command record; the original command ran directly before that file existed.
Correct only this API spelling. Six direct owned-file tests now exercise real
atomic replacement, replacement failure, full binary streams, failed/finished
states and capture hashes. These Linux Python observations establish journal
I/O; they do not establish Source CLI or Windows process behavior. The complete
unchanged Source gate and controls must be rerun independently before acceptance.

Windows CI job111355025851 proves that a native `symbrain setup --from-source`
invocation exceeded the outer replay timeout of25 seconds. Its command line
does not identify the case: many cases use identical arguments. The report was
written only after every pair and cleanup finished, so the exception discarded
all earlier observations. The upload then reported no JSON files. Preserve the
original log and classify the concrete failing phase as unknown.

Do not infer a cold Go build from that command line. Most Source cases use an
owned compiled fixture executable as git/go/swift. Only `real-go-build` copies
the actual SDK tool and creates a dependency-free worker. Its explicit GOCACHE
is a newly created `setup-source-tools-*/real-go-cache`, outside each disposable
fixture. Go runs first and Rust runs second against that shared cache. A real
native build would normally consume the cache just warmed by Go. Neither a
cache timing measurement nor the failing case was retained in this CI job.

The outer Python25-second watchdog is separate from native30-second identity
commands and30-minute builds. Frozen Go uses the setup background context and
has no equivalent per-build deadline. Installed version probes use three
seconds. Native tool output is captured in private files, while frozen Go uses
Output/CombinedOutput pipes. The JSON report is emitted after the build and
publication; empty observed stdout does not establish which earlier phase hung.

Add a bounded journal outside the compared fixture roots. Record configuration
entry, actual child launch, return, full raw stdout/stderr, each exact completed
observation, pair completion and cleanup boundaries. Before deleting a failed
fixture, retain diagnostic metadata, hashes, tool invocation logs and any live
`.symbrain-source-capture-*` bytes. Retain the actual GOCACHE path and state for
the real worker. Missing/partial TimeoutExpired streams stay nullable. A live
snapshot may race changing files; truncation, instability and read failures are
explicit and cannot satisfy a comparison or a cleanup assertion. Atomic JSON
checkpoints are already covered by the existing `setup-source-parity*.json`
upload. The existing cases, exact contracts and25-second timeout stay intact.

There is also a concrete Windows cleanup hypothesis in pinned process-wrap
10.0.1. JobObjectChild.try_wait consumes a completion-port notification and
returns the inner status without caching that status in the job wrapper.
ChildWrapper.kill calls start_kill then wait. JobObjectChild.wait first caches
the inner exit status, then waits for a notification with INFINITE. Existing
Owned.drop calls kill then wait after polling. If polling consumed the final
notification and termination of an empty job adds none, cleanup can block beyond
the polling budget. This source path is not yet the proven cause of the CI
failure. Exact API sources and Cargo-lock identity are retained with the review.

Prepare an owned native Windows experiment: let a child exit, repeatedly poll
its cached status to consume notifications, then enter the historical drop.
An external12-second watchdog retains every phase, stream, PID and forced
cleanup result. Separately compare start_kill plus inner_mut().wait, including
a successful parent with a live owned descendant. JobPort closes handles on
drop, but this std wrapper creates the job with kill-on-close disabled. Therefore
merely dropping the wrapper would abandon the required descendant termination.
The diagnostic comparison is not a production fix; actual Windows evidence and
the Source journal must guide that decision. No timeout increase, dependency
upgrade, Go production mutation or test projection is justified by this review.


Root independently executes all111 original Source pairs at clean7ea3cfb. All111 exact observations match; the journal retains893 events and111 complete pairs, including successful actual native descendant cleanup. Actual wrong-exit and wrong-source children are rejected on exactly their intended fields. A separate real controlled Python child reaches the unchanged25-second watchdog (25.07s): NUL/FF stdout/stderr and all256 marker bytes survive in a failed journal before owned fixture teardown. This establishes diagnostic losslessness, not a new production timeout or a Windows cause.

All288 current-source/four frozen-Go bindings and300 production files identical to fully compiled58fe50 are checked. The exact independently reviewed Rust executable is restored from its archive, while Go CLI9337009 was freshly compiled earlier; no fresh Rust compilation is claimed for this diagnostic-only successor. The Windows ignored test and external watchdog remain source-only; actual current-head Windows/macOS execution is required. Complete Root raw proofs are retained in migration/evidence/doctor-source-progress-806/independent-7ea. The original9fa API failure, complete original CI timeout and original c672 process-proof lineage remain immutable.


At current8359, Linux job111372955009 reaches all four release setup output
pairs, then fails before source output comparison with `ModuleNotFoundError:
progress`. Direct replay starts with its own script directory on sys.path;
source-output, pipe, human-pipe and embedded probes import it from another
directory. Bind the sibling progress module by its resolved file path, with
a specific module name, so invocation directory and unrelated Python modules
cannot choose the logger. Keep every replay function AST, observation, case,
writer contract, timeout and control unchanged.

Root reproduces the original import failure with isolated Python from /tmp and
loads the repaired module successfully. The whole actual Linux output suite
passes with the archived exact58 native CLI/embedded writer and frozen Go CLI:
all original output, source, pipe, human-pipe and embedded comparisons, plus
all four actual control groups rejecting15 intended differences and preserving
three partial-human controls. No fresh Rust build or native Windows pass is
claimed. Full original provider log, ZIP/member hashes, raw reports and source
are retained in import-boundary-8359. The actual Windows journal now narrows
the first explicit-browse-json timeout to after captured Go-version output and
before the build invocation; polling versus cleanup still needs native phase
evidence. Keep the25-second budget and execute the owned diagnostic watchdog.
