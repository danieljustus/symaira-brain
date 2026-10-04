# Own the Skills frozen Go checkout explicitly

Status: source-only successor to immutable522ce5d/source51e42aa; light actual
SDK validation follows a clean source checkpoint. Refs #793/#794.

The original complete Linux author evidence remains unchanged:551 primary Rust
tests,14 separate nested passes,315 exact process pairs,3 genuine controls and
53 additional owned cases. Its failed unstamped Go executable and exact output
were retained before the successful real-Git-directory build. The pinned
Go1.26.7 SDK recognizes a Git directory, while the central worktree helper
creates a .git file. The unchanged Skills comparator correctly rejects that
missing revision/modified stamp. The same preparation problem is reachable in
the existing Windows Skills CI step.

Choose one dedicated Skills builder rather than changing every central Go
reference route. It fixes the immutable revision and command package, creates
an owned local shared clone with a real .git directory, disables CRLF
conversion, and verifies all committed blobs, index modes, native file types
and supported native permission bits before and after building. Windows does
not expose the POSIX executable bit; retain its exact index mode and native
permission observation instead of claiming a Unix-mode check ran there.

Require installed Go exactly1.26.7, buildvcs=true, mod=readonly and a clean
frozen checkout. Caller-selected preexisting Go caches are explicit inputs;
private HOME/config/telemetry paths, disabled user Git/Go configuration,
offline module/toolchain resolution and local-only clone protocols prevent
operator configuration or network downloads from participating. Verify cached
module integrity, bound build concurrency to2 and keep the700MiB stage floor.
The output and receipt resolve outside the source checkout and fixture;
existing evidence is never overwritten. Full command outputs are retained as
bytes in the checked build receipt, including failures.

The new helper is solely SDK-fixture preparation. It does not change product
stdio/signal policies, Rust bodies or Cargo dependencies. Its subprocesses
have dedicated groups, bounded waits and scoped interruption cleanup. Native
Windows timeout/cleanup behavior still requires native execution; source
inspection does not establish it. The Windows CI step changes only its Skills
Go build invocation and preserves the312 wide cases and3 existing controls.
The central run-go-oracle.sh, comparator, corpus and control verifier retain
their original bytes.

Meaningful source admission tests use an actual frozen Git checkout. Changed
Go payload bytes and changed index modes must fail before compilation;
existing outputs/source destinations must be rejected without mutation.
Actual pinned-SDK positive and linked-worktree metadata-failure observations
will be retained separately. No Cargo target, operator service or11434
endpoint is allocated to this source-only successor. Root owns the existing
Skills target and performs different-author full current-leaf review and
native315/312 acceptance before publication.
