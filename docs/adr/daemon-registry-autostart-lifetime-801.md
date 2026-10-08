# Registry fixture ownership lasts until the detached process exits

Actual native CI run37211928501 at the immutable 8a95 publication failed on
Windows x64 and ARM64 after all35 preceding daemon comparisons and three
controls passed. The separate registry gate failed removing its owned `br-`
directory with WinError32. Those preceding successes do not make the job or
registry gate accepted. Both complete logs, content JSONs, all9,762 members
of the four native ZIPs, their CRC/mode/date metadata, CLI raw captures and
progress journals remain bound to the original sources and roles.

The partial reports retain the completed Go observation. Their final journal
records identify the Rust observation: autostart.stop.begin, frame.begin,
frame.end, autostart.stop.end, observe.end, then temporary-directory cleanup.
The last two events are less than a millisecond apart. No process-exit wait or
retained autostart handle exists in that source. WinError32 does not identify
which remaining PID holds the cwd; the original journal cannot establish that
PID or prove that the new source fixes the native failure.

Directly started daemon children already use communicate/wait. Autostart is
intentionally exercised through eight real CLI processes, and its detached
server is not one of those Popen objects. Treat its successful IPC stop reply
as a shutdown request, not proof that the server has exited and released its
Windows cwd handle. Retain a Windows kernel process handle immediately after
the private endpoint reports the owner PID. Verify its image against the exact
candidate binary, then confirm the existing status/session.info PID agreement
and that the retained process is still alive. Never force cleanup by reopening
a numeric PID, executable name or global process list.

After the stop response, wait on that same kernel object for the existing
15-second CLI lifetime budget. If stop failed or exit times out, terminate
only the already confirmed handle and require completion within the existing
two-second cleanup budget. Forced cleanup remains a failed case, even if it
releases the directory successfully. Preserve both wait/force failure and
closure in the durable progress journal. A nonzero normal daemon exit also
fails. Close the verified handle before leaving the owned temporary directory.
Unconfirmed identities are closed and refused; they cannot authorize a force
kill. No global signal changes, cleanup-error suppression or directory retry
are added. Ordinary directly owned CLI15s/cleanup2s, daemon product deadlines,
request criteria, registry comparison and all eight existing registry controls
remain unchanged. Non-Windows invocation behavior is unchanged.

Thirteen pure Windows-API/ownership mock tests check ordering, 64-bit retained
handle use, mismatched images/PIDs, already-exited/unconfirmed identities,
timeout/stop/cleanup failures, nonzero exits, PID reuse and API signatures.
They run in the native workflow before product comparisons. Pure source
mutation controls must reject skipped waiting, swallowed timeout, numeric-PID
termination and wrong-image acceptance. Such tests are actual Python machinery
checks, not native Windows API or daemon acceptance. Original five progress
and four capture tests remain separate and unchanged.

This source-only successor has no SDK/product/compiler/Target/port allocation.
Fresh exact-head full Go/native registry reports and controls on all six
native runners must establish cleanup completion and actual process identity.
The original 324 inputs, seven Windows deadline cases, Darwin admission and
all later process/state/MCP families retain their complete criteria and pending
owners. There is no full #801/#772 cutover, Windows acceptance or merge claim.
