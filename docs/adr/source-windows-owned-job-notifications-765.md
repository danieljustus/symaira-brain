# Source Windows polling preserves whole-job cleanup notifications

The complete original Windows run37184966738/job111384880074 remains frozen.
Publication44354141 and actual clean CI mergefb39903 are separate identities;
all recorded Source/diagnostic input hashes match the publication bytes. The
original Source gate selected95Windows cases, completed one JSON pair and then
timed out at the unchanged25second budget for native explicit-browse-human.
Its staged payload and invocation log are retained, but do not establish the
original CLI's blocking frame or CPU use. Source negative controls were not
reached. Do not call that acceptance successful or retrospectively supply a
missing original phase.

The same actual native Windows run separately proves a reachable cleanup
defect. Its isolated historical mode observed parent exit and16additional
wrapper polls, then entered historical kill without returning before its
unchanged12second watchdog. Parent-only-wait and live-descendant comparison
modes returned0 without forced cleanup; the recorded descendant was inactive.
The diagnostic explicitly disclaims production acceptance and proof of the
original CLI timeout's exact cause. All63original files, complete decoded job
log/API, original raw ZIP/member bytes, source maps, locked dependency sources
and phase report/receipt are SHA-bound and gzip-roundtrip retained under
`migration/evidence/setup-source-765/windows-job-443-original/` before edits.

Pinned process-wrap10.0.1 std JobObject `try_wait` consumes completion-port
notifications before polling the inner Child. It does not populate the wrapper
wait cache. Default kill invokes start_kill then wrapper wait; after the final
ACTIVE_PROCESS_ZERO notification has already been consumed, that wait may block
indefinitely. The original diagnosis reproduces exactly that API sequence.

Choose top-level polling while preserving whole-tree waiting. The Source-owned
Windows helper polls the inner std Child only, leaving job notifications intact.
On cleanup it terminates the complete owned JobObject and waits through the
wrapper, including after a termination error as the old Drop did. Thus cleanup
still awaits the whole job, rather than permanently switching to parent-only
waiting. Parent cached status and descendant termination are separate duties.
No taskkill, global wait disable, budget increase, unsafe code or new dependency
enters production. Unix group kill/wait, cancellation, capture ordering, argv,
source provenance, install state and error output remain unchanged.

Broker at this source uses std::process::Child and its cached status, not
process-wrap JobObject or Tokio. No current Rust product route uses the Tokio
JobObject implementation. Its pinned source has the analogous notification
wait concern; this successor introduces no such route and does not rewrite
Broker ownership policies or claim a new Broker process-tree contract.

The original three diagnostic modes remain. Two additive native Windows modes
use the same production polling/cleanup helper for an already-exited quick
parent and for a successful parent with a still-live owned descendant. Each
repeats16parent polls, requires exact successful phase/exit completion without
forced cleanup, and confirms any recorded descendant inactive. Their events
explicitly distinguish parent polling from completion-port polling. The
historical hang remains a genuine retained negative lifecycle observation.

Original artifacts omitted native executable bytes. Their recorded CLI/test
SHAs and source/Cargo bindings are retained with that explicit gap. Additive
Windows collection copies the actual Go/current Source CLI and executed
diagnostic test PE files before temporary cleanup, records their SHA/size/PE
machine/source checkout, checks byte stability, and uploads bytes plus manifests.
This does not reconstruct absent old executables or alter comparison values.

This is an uncompiled source-only successor. Pinned rustfmt, Actionlint, Python
preparation and source controls cannot establish native runtime acceptance.
Require different-author full review followed by actual native Windows five-mode
diagnosis and full95Source cases/original controls, current111Unix Source and
all parent Setup/Doctor/output/lifecycle gates, plus native-three-OS CI before
regular merge. Only fresh complete native Source results can determine whether
the original externally observed failure has also been eliminated.
