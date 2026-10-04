# Windows daemon probes: explicit pipe errors and bounded I/O

Decision: replace the probe harness CRT file stream with a small standard-library Win32 byte-pipe transport. Use pointer-width handles, explicit function signatures, overlapped reads/writes and one request deadline. Only ERROR_PIPE_BUSY is retried before sending bytes. Other connection errors retain their precise Win32 code. Once connected, transfer failures are not readiness retries, preventing mutation replay.

Reason: native AMD64 logs show errno22 from Python open(), which hides the precise Windows error. Native ARM64 passed35 actual daemon requests but its registry step later reached the job deadline. The current stream reader checks a deadline around a blocking read, so it cannot enforce that deadline. The log does not prove this caused ARM64 cancellation. Increasing the job timeout would not repair the reader.

Ownership: one client handle and per-operation event; on pending-operation failure cancel that operation and drain completion before releasing its buffer/OVERLAPPED. Close both handles deterministically. No daemon production logic, Go source, wire protocol, request corpus or acceptance threshold changes.

Evidence: preserve both full original CI logs, downloaded native artifact and independent original-evidence receipt under migration/evidence/browse-registry-772/windows-pipe-7f. The reported Go status after kill_tree is not a demonstrated original Go exit. Native unit41pass+3owned child ignores, lifecycle1x1 and race50x50 passed. Portable transport controls exercise busy-only retry, partial writes, ownership, frame bounds and no replay after transfer failure; they do not establish Win32 runtime success. Require independent review plus genuine six-platform exact-head acceptance before merge.
