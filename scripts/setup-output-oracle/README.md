# Actual Setup output boundaries

`run.sh REPORT_DIRECTORY` builds the frozen Go oracle and native CLI plus its
public embedded-caller example. Run from the repository root. Reports preserve
raw bytes, exits/signals, and complete owned file/mode manifests. Only private
fixture roots, verified new UTC provenance timestamps and documented Source
staging names are normalized; comparison failures retain their full reports.

The four original process groups contain 18 Linux pairs: six `/dev/full` JSON
writes, six closed/partial-reader JSON writes, and six human writes. Partial
readers consume exactly 128 bytes from a 4 KiB pipe. Human closed-reader writes
include the unsupported symcockpit row before subsequent release installs.

`embedded.py` adds a test-only caller to an owned archive of unchanged Go source
and compares its custom writers with the real Rust public `run` API. The 24
pairs cover install, fix and source, human/JSON, raw EPIPE, raw ENOSPC,
kind-only broken pipe and callback errors. No production Go source is changed.

`controls.py` executes the real native child and changes only the emitted
stdout-error diagnostic or terminal signal status. All 15 intended differences
must be rejected and three partial-human controls must remain matching. Full
control observations, wrapper bytes/hash and failed replay output are recorded.

This gate has an explicit Linux CI owner. `/dev/full` and pipe sizing are not
portable runtime proofs. Existing Windows/macOS parent gates are still required.
