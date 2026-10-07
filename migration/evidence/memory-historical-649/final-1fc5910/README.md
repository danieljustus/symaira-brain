# Author historical Store runtime validation

Tested source: `1fc5910ebc8ae04b772200ac7cb69afda8fd8716`.
The full driver, original allocation environment and exact commands are in `validation.json` and `run.sh`. Use only an explicitly allocated target and fresh output directory. The owned standalone frozen Go source has a real `.git` directory; its 1,215 files/two manifests are bound in `original-frozen-source.json` and freshly checked in `fresh-lineage.json`.

`retention.json` maps every one of the 329 original output files to exact raw SHA/size and retained lossless gzip SHA. All original Go/native stdout/stderr, full SQLite schema/table/shadow snapshots and before/after database files remain available. The compressed child paths are data, not new process executions. `binaries.json` binds all 519 current ELF paths/487 unique payloads, 38 actually executed Cargo roles and actual CLI to roundtrip-verified archives; unrelated compiler artifacts are not claimed as executed. It also retains/freshly verifies the three failed source-run binary receipts. All prior tracked historical proof files are hashed; no original failure is overwritten.

This is author Linux validation, with separate immutable Go observations for corrective repair controls. Full different-author review, native three-OS/protected CI and shipped release remain pending. No operator data/credentials or paid endpoints were used.
