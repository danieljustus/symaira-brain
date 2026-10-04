# Skills sync raw argv process gate

From a clean committed repository with pinned Go 1.26.7 and the candidate CLI
already built, create an owned scratch directory and run:

```sh
python3 scripts/skills-argv-oracle/build_go.py --out "$OWNED/symbrain-go.exe" --receipt "$OWNED/go-build.json" --go-cache "$(go env GOCACHE)" --module-cache "$(go env GOMODCACHE)"
python3 scripts/skills-argv-oracle/compare.py --go "$OWNED/symbrain-go.exe" --rust target/debug/symbrain.exe --out "$OWNED/process.json"
```

The command above requires a complete preexisting frozen-module cache. Add
`--prepare-module-cache` when preparing a fresh runner: that separate recorded
phase uses the public Go proxy and checksum database to fetch the frozen
dependencies. It verifies the complete frozen Git source again afterwards.
Module verification and compilation then run with `GOPROXY=off` and
`GOSUMDB=off`; a failed download or changed source stops the gate.

Use the actual native Windows runner for Windows acceptance. Python Popen
passes wide arguments to CreateProcessW; the report retains UTF-16LE argument
code units, including lone surrogates, exact stdout/stderr bytes, exit codes,
complete owned-home metadata/content before and after, immutable source hashes
and copied binary identities. No fallback binary, operator credentials or
network endpoint is available to the candidate. Optional `--parent` records an
actual archived prior CLI's output without treating it as the oracle.

The Skills-specific builder requires installed Go exactly1.26.7 and explicit
preexisting compiler/module cache directories. Dependencies must already be
cached or fetched by the explicit preparation phase above: the verification
and build disable network downloads, toolchain downloads and user Git/Go config.
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

The separate immutable supplemental plan is enforced by `novel.py`. It selects
53 cases on Unix (51 Go/native pairs and two actual-parent pairs), or 112 on
Windows (109 Go/native pairs and three actual-parent pairs). The runner requires
an actual parent binary and archive/build proof; it never invents inherited
output or converts it to Go parity. All three process results and complete
seeded filesystem snapshots are retained. Run its `flag-input` and
`parent-input` controls with separate outputs and verify each using
`verify_novel_control.py`.

On Unix, pass the original FAA native CLI and its round-trip archive receipt as
`--parent` and `--parent-proof`. On native Windows, `build_parent.py` saves the
current CLI, builds immutable 01f with the same existing target/cache, and
restores and verifies the current bytes even if the build fails. CI then runs
`novel.py` using the saved current CLI and both actual references, and preserves
all three executables and JSON evidence. This is a native CI build; portable
tests and source review cannot establish the Windows runtime result. See
`docs/adr/skills-native-supplemental-plan-794.md` for the exact ownership contract.
