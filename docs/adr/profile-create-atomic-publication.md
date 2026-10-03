# Atomic profile publication under concurrent creators

## Observed failure

Native Windows CI run37153314306, job111291463063, for docs PR802 failed
`profile::create::tests::concurrent_creators_never_clobber_each_other` at
`create.rs:223`: a losing creator returned PermissionDenied instead of
AlreadyExists. The same test passed in the later ordinary suite in that job.
The original complete log is retained under
`migration/evidence/profile-create-race/original-windows-ci.log`; a passing rerun
must not erase this intermittent failure. Profile production was unchanged from
main31de722: docs head06ff95b4 and main have identical create.rs SHA256
13c3de6a10eaabe50585ac1f12f177a3962aec353bf4caabd76f85647b7c7386.
The original log names actual checkout merge202de89a53f653c43a062f594f4ac8c6fe130be6
(head06ff95b4c48a8aeef82820bc913d734c2a6f5f11 into
base31de72294521701fc4b1a0bce39f03cc34d72e7e). This Linux workspace supplies no native Windows
acceptance result.

## Decision and reason

Replace the deterministic shared temporary namespace `.race.toml.0.tmp` through
`.99.tmp` with a uniquely allocated owned `tempfile::NamedTempFile` in the same
parent directory, then publish with `persist_noclobber`. Windows can return access
denied while a previously deleted filename remains delete-pending; the observed
log establishes the error kind but does not instrument the precise failing
filesystem call. Avoiding shared name reuse addresses that hazard structurally,
without treating permission errors as AlreadyExists, retrying profile creation,
serializing creators, or weakening the test.

Promote the already workspace-pinned tempfile3.27.0 from this crate's test-only
dependency to production. No dependency version, lockfile package or Go source
changes. This uses the established portable ownership/publication implementation
instead of maintaining another random-name generator and platform FFI layer.
The pinned implementation uses Windows MoveFileExW without REPLACE_EXISTING;
Unix uses no-replace rename where supported, otherwise atomic hard-link
publication. All reject an existing destination rather than replacing it.

Set the same Unix0600 mode on the owned temporary handle, write all template
bytes, and sync before publication. The winning profile therefore contains the
complete selected template. The file owner cleans up on mode/write/sync or
publication failure. Directory creation retains the existing Unix0700 behavior.
Permission and other I/O failures retain their original error kind. Existing
profiles keep the existing AlreadyExists diagnostic and CLI exit behavior.
Hard-link fallback cleanup still depends on filesystem deletion succeeding;
this change does not promise cleanup after external ACL/filesystem interference.

## Required verification

Keep the original eight-creator assertions and add a simultaneous start barrier;
run32 independent eight-thread races. Every round requires one winner, seven
AlreadyExists losers, the exact winner's template bytes, Unix0600 and one final
file with no owned temporaries. Add an actual eight-process start barrier with
eight separately observed results and the same winner/loser/byte/cleanup checks.
All calls happen once; coordination polling is only an IPC start/completion wait,
not a retry of profile creation. Process guards kill/wait children on failure.

Additional probes preserve100 preexisting legacy temporary files untouched,
verify occupied-file and directory publication failures remove the newly owned
temporary, and retain existing-profile/validation behavior. The native init CI
step on all three targets explicitly runs the process suite as well as the
original focused unit suite; the complete workspace gate also discovers it.
Native focused runs print observed creator results and full winning template
bytes; always-uploaded logs retain available successes/failures for14 days.
Affected policy and CLI init/profile contracts, strict Clippy, formatting and
CI validation must pass. Native Windows/macOS exact-candidate CI and independent
review remain required before acceptance. This change does not itself approve
or merge its author's implementation.
