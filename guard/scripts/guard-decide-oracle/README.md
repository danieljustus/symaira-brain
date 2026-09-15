# External guard-decide Go oracle

`oracle.py` is a provenance-bound, black-box harness for the real
`symbrain guard decide` command. It never implements policy. The corpus in
`cases.json` supplies inputs and independently stated expected decisions; the
command supplies the actual outputs.

Run from the repository root:

```bash
python3 guard/scripts/guard-decide-oracle/oracle.py
python3 guard/scripts/guard-decide-oracle/oracle.py --check
python3 guard/scripts/guard-decide-oracle/oracle.py --validate \
  target/migration-run/guard-decide-provenancefix/manifest.json \
  --expected-manifest-digest "$(shasum -a 256 target/migration-run/guard-decide-provenancefix/manifest.json | cut -d' ' -f1)"
python3 -m unittest discover -s guard/scripts/guard-decide-oracle -p 'test_*.py'
```

The harness treats the fixed acceptance revision
`0b585d52915a824664e1377d0a995dff3f5405cd` as a trust anchor: it independently
resolves that commit and archives its tree, rather than trusting the checkout's
current HEAD. This permits a historical oracle run while still rejecting a
missing, malformed, or different commit object. The current checkout HEAD is recorded
for context only and is not required to equal this immutable historical trust
anchor. It verifies the complete historical tree before building, and builds there with the selected Go
1.26.7 toolchain and `CGO_ENABLED=0`; the manifest binds the binary's Go
build-info toolchain to the selected executable hash and records the Go 1.26.6
launcher separately. The manifest records every tracked build
input (including nested module manifests and toolchain files), source hashes
before/after, harness and case digests, toolchain identity, binary size/hash,
full argv/cwd, environment allowlist, and raw stdout/stderr hashes.

Each case runs with unique HOME, XDG roots, and private TMPDIR. Subprocesses
run in their own process group with a bounded timeout; timeout state is captured
at the timeout exception and remains distinct from the final return code, while
cleanup signals and reaps descendants. Output is captured before cleanup. Audit JSONL is read
back before runtime cleanup, checking its schema, decision, redaction, and
0700 directory/0600 file permissions, then copied into evidence.

`--check` executes and validates the real corpus again; it does not rewrite
cases or treat handwritten expectations as generated output. `--validate`
validates evidence hashes and provenance without rerunning the oracle. An
independently supplied manifest digest can bind the evidence file itself; the
validator rechecks mutated output/source/toolchain metadata through its real
validation path.

The deadline-equality case is intentionally counted as one diagnostic probe.
It is excluded from deterministic acceptance because production has no clock
injection seam; the source equality rule remains documented in `cases.json`.
The generated evidence lives under the ignored
`target/migration-run/guard-decide-provenancefix/` directory. Evidence paths are
relative to that root; absolute, traversal, and symlink-escaping paths are
rejected, and declared byte counts are checked against the bytes read.
