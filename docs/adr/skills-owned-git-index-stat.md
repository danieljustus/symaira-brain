# Owned Skills restore: volatile Git-index stat fields

Accepted owner decision, 2026-10-11: “Ja, nur Git-Index-Statmetadaten”.
The decision is retained in Kanban t_11cd8f23 comment132. It does not waive
native acceptance, change product Git settings, or permit an index exclusion.

## Boundary

The original Go oracle remains dcddcef0df5789123c7c9a7ebe6e01f10e941f2c.
Actual repeated frozen-Go restore writes produced different raw index bytes
with identical protocol responses, paths, blobs, modes, stages, commits and
trees. The source-bound prior capture is
`pr813-integration-t_11cd8f23/applying-sync-after-lock-snapshot/receipt.json`;
its `go-write-a.index.bin`, `go-write-b.index.bin`, and `rust-write.index.bin`
were independently checked against recorded SHA-256 and internal SHA-1.
Only ctime, mtime and inode differed. Device, UID, GID and size remain strict.
Index layout follows https://git-scm.com/docs/index-format.

`git_index.py` supports only the owned demo fixture's SHA-1 version-2 index,
its two ordered stage-zero entries (`SKILL.md`, `notes.md`), ordinary flags,
and optional flat-root TREE extension. Other versions, entries, extensions,
invalid lengths, padding and checksums fail closed. TREE bytes are compared,
not discarded. The parser is deliberately not a general Git-index reader.

The only projected fields are ctime seconds/nanoseconds, mtime
seconds/nanoseconds and inode. Full raw bytes, SHA-256, internal checksum,
file mode and decoded stat observations remain in each process capture.
Separate non-mutating comparison views preserve every other byte/field.
No raw report or historical failure is rewritten.

Projection requires exact write-request bytes and process-input digest,
known fixture/case ownership, and the existing frozen-Go startup binding.
It applies to the ordinary applying restore, applying restore with sync,
and the existing normal/warning archive wrapper variants only. Other paths,
inputs and cases retain strict comparison. The complete startup, catalog,
protocol, content, marker, Git object, commit and tree assertions still run.

## Executable gate

`python3 -B -m unittest discover -s scripts/skills-native-oracle -p 'test_*.py' -v`
executes real Git-generated index bytes inside synthetic MCP envelopes. It
accepts only approved stat mutations and rejects coordinated raw/hash changes
to mode, blob, path, stage, flags, device, UID, GID, size, TREE data and malformed
structures. It also rejects missing raw bytes, false lengths and wrong ownership.
These are structural controls, not native Go/Rust acceptance.

The unchanged `scripts/skills-native-oracle/run.py` command-line interface runs
the actual process matrix. Retained native replay, fresh startup controls,
actual Git mutation controls and candidate-bound independent review remain
mandatory. Linux and Windows cannot be inferred from local macOS success.
