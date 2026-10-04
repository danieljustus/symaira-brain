# Skills sync raw argv process gate

From a clean committed repository with pinned Go 1.26.7 and the candidate CLI
already built, create an owned scratch directory and run:

```sh
python3 scripts/skills-argv-oracle/build_go.py --out "$OWNED/symbrain-go.exe" --receipt "$OWNED/go-build.json" --go-cache "$(go env GOCACHE)" --module-cache "$(go env GOMODCACHE)"
python3 scripts/skills-argv-oracle/compare.py --go "$OWNED/symbrain-go.exe" --rust target/debug/symbrain.exe --out "$OWNED/process.json"
```

Use the actual native Windows runner for Windows acceptance. Python Popen
passes wide arguments to CreateProcessW; the report retains UTF-16LE argument
code units, including lone surrogates, exact stdout/stderr bytes, exit codes,
complete owned-home metadata/content before and after, immutable source hashes
and copied binary identities. No fallback binary, operator credentials or
network endpoint is available to the candidate. Optional `--parent` records an
actual archived prior CLI's output without treating it as the oracle.

The Skills-specific builder requires installed Go exactly1.26.7 and explicit
preexisting compiler/module caches. Dependencies must already be cached: the
build disables network downloads, toolchain downloads and user Git/Go config.
It uses private HOME/config/telemetry paths and a local shared clone with a real
`.git` directory, verifies every frozen blob/index/native mode before and after
building, and requires the pinned revision with `vcs.modified=false` in the
result. Outputs must be new paths outside the source checkout and fixture.
The JSON build receipt retains exact commands, byte-valued outputs and source
maps, including failures. Existing comparison and control assertions remain
unchanged. The central `run-go-oracle.sh` remains available for other oracles.

On Unix, use the corresponding non-`.exe` binaries. All original 315 argument
byte vectors are replayed unchanged. Windows adds native wide spellings,
inline/separated targets and scopes, whitespace boundaries, Boolean values,
normalization, help and stop controls. Portable helper tests and copied SDK
source do not replace this Windows runtime gate.

Run each of `--negative-control flag-input`, `quoted-value`, `normalization`
with a separate output path. Each changes an actual candidate process input
and must return 1; the original Go input stays unchanged. Preserve their full
reports. Build/oracle failures and mismatches remain evidence, never a skip or
an approval. Native three-OS CI and independent full review remain required.
