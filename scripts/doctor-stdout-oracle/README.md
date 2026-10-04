# Doctor stdout completion oracle

Run from the repository root with Go 1.26.7 and the existing owned Cargo target:

```sh
scripts/doctor-stdout-oracle/run.sh /tmp/doctor-stdout-proof
```

The driver resolves the explicit `CARGO_TARGET_DIR` (or default `target`) before
building, then passes absolute executable paths to all four probes. Their
children run from disposable owned projects; a relative build path must not be
resolved against those projects. Individual probes also resolve explicit Go,
Rust and optional baseline executable inputs before changing child directories.

The Linux job owns real `/dev/full` and 4 KiB bounded-reader probes. Nine non-fix JSON/human/help cases and four repair cases compare actual frozen-Go/current-native processes, complete stdout/stderr/exit and full owned filesystem/log state. Closed-reader repair must stop at its first header; ordinary human write errors still permit diagnostics and repairs.

Twenty-seven additional actual-Go/public-library cases cover immediate errors, a cumulative seven-byte prefix, and recovery after the first error across repair, JSON and human formats. Caller-owned raw EPIPE is not process stdout. Three actual child mutants change only signal exit, concrete JSON errno text, or human error status; they must reject six intended observations and preserve 21 unchanged observations.

For local historical reproduction, set `DOCTOR_STDOUT_BASELINE_PROBE=1` and `DOCTOR_STDOUT_BASELINE` to the SHA-verified preserved a66 CLI. Exactly six of its nine non-fix cases and two of its four repair cases retain the original failures. These are retained rejected baseline evidence, not candidate acceptance. CI does not depend on a local archived executable.

Each new report records the immutable candidate, dirty state, complete scoped source manifests, frozen-Go source manifest, actual binary hashes, SDK and runtime. No frozen production or original fixture is changed. Linux observations do not establish native Windows/macOS execution or completion of issue #765.
