# Standalone native Guard process proof (#770)

Run from the repository root with pinned Go 1.26.7, Rust 1.98 and Python 3:

```sh
scripts/guard-standalone-oracle/run.sh /tmp/guard-standalone.json
```

The runner archives the complete immutable Go tree `dcddcef0df5789123c7c9a7ebe6e01f10e941f2c`, verifies every original Go source and module input byte, then builds only the supplemental `main.go.txt` entrypoint over the unchanged public Guard command packages. The frozen repository has no standalone Go main. The new Rust binary has no Brain CLI dependency or Go fallback. Tests use disposable HOME/XDG roots and empty PATH, with no operator configuration, credentials, provider calls or real MCP server launch.

All124 selected process cases execute on each native platform. The corrected warning slice requires122 full stdout/stderr/exit and file modes/content matches; owned root paths and validated truthful SDK lines are the explicit runtime differences. Real audit ID/time fields must fall inside the execution window and agree with each other before only those fields are replaced for comparison. Complete raw bytes are retained. Two selected TOML diagnostic states remain unported and must explicitly fail closed with exit1 and no stdout. The known Unix directory audit-open diagnostic now matches; Windows retains the deny-only audit wording contract pending native proof. These are selected-case counts, not every unsupported diagnostic shape. Typed anchor cases preserve duplicate/null/folded fields, numeric spellings and first-error order; unknown values use raw JSON and do not introduce float64 overflow errors. Three actual-process output mutants hide a config error, hide an anchor error and change audit-failure deny to allow; each must be rejected. Both full reports and controls are native CI artifacts. The platform-specific frozen broken-output case is excluded from this selected portable corpus and is not counted as passing.

**Approved deviations (#770).** The doctor states this README describes as
gated, refused or delegated are now rendered natively. Their cases use the
`approved-deviation` contract (identical exit, stderr, state and report rows;
only an error row's detail after Go's frame may differ) or, for
case-insensitive field aliases, `approved-stricter` (Go accepts, native reports
a decoder error). See
[the decision](../../docs/adr/guard-doctor-approved-deviations-770.md).

The receipt records each case, process output, state, Go/source/binary hashes and candidate revision/dirty status. Production Go and existing frozen fixtures remain unchanged. Native Linux/macOS/Windows jobs run the gate and retain artifacts even on failure. Completion of #770 additionally requires the remaining diagnostic/output-boundary ports, complete native acceptance and truthful command/library inventory; this scoped runner alone does not meet zero-unported acceptance.

The additional `-raw-paths.json` receipt exercises actual private diagnostic
paths through the same Go and native processes. Unix includes malformed UTF-8
bytes, while all native platforms exercise valid U+FFFD and U+2028/U+2029.
Doctor text retains the original bytes; JSON replaces each invalid byte
separately while escaping HTML/JavaScript separators. Already-denied decisions
keep their own reason on audit failure. Two actual lossy-output mutants must
be rejected on Unix. Windows records raw-byte cases as inapplicable and retains
the existing fail-closed audit wording deviation; these are not equality passes.
All three CI jobs retain this additional receipt. See
[the raw-path decision](../../docs/adr/guard-raw-path-diagnostics.md).

The additive `-doctor-boundaries.json` receipt admits equivalent typed inline
TOML/array forms and Go-compatible anchor string/key Unicode replacement.
It executes78 actual process cases:68 complete comparisons and10 explicitly
retained TOML decoder/map-order gates. Three actual mutants remove a
configured inline allowlist, gate a repaired healthy anchor and hide a later
typed anchor error. All original124 input bytes are retained; the three specific unknown-key inputs
across these two corpora now require complete warning/report equality. All
remaining decoder/map-order gates keep their original refusal assertions.
The [remaining boundary inventory](../../migration/guard-doctor-boundary-inventory-770.md)
distinguishes healthy Go-only states from decoder errors and warning-only states.

The `-config-warnings.json` receipt ports ordered unknown-key stderr for
successfully typed configurations. The corpus requests54 cases; macOS arm64 executes 53 (raw-path-0 is unexecutable on Darwin):
47 full comparisons and 6 retained typed/alias gates (`warning-discovery-delegation` now matches);
the Linux count is pending CI. Plus ten actual Go repeats
and five real warning mutants. It preserves the exact fourteen original healthy
Go inventory inputs. Windows runs52 launchable cases and records the two raw
Unix path forms as inapplicable; none is a UTF16 runtime claim. A known config
validation failure still emits its warnings; type failures emit none. Warnings
stay buffered across later native delegation. See the
[warning decision](../../docs/adr/guard-doctor-ordered-config-warnings.md).

Before the temporary frozen-Go tree is removed, the runner copies both actual
executables to `-binaries/` and verifies every byte/SHA/length. The clean-head
receipt and all three native CI artifact uploads retain these exact gate binaries.

The `-config-paths.json` successor receipt checks generated XDG/HOME lexical
ownership and exact warning filenames with plain/dot/dotdot/relative/symlink
paths. Explicit SYMGUARD_CONFIG keeps its raw OS behavior. Private directory
junctions provide the Windows symlink-parent control without operator state.
Unix raw byte paths are explicitly inapplicable on Windows. Typed/discovery
refusals retain empty native stdout and the explicit unsupported stderr; the
exact Brain adapter tests additionally require both streams empty before Go
fallback. One actual process mutant canonicalizes a generated base and reads
the wrong physical owner, and must be rejected.

The `-windows-path-reference.json` process gate copies exact SHA-pinned Go
1.26.7 Windows Clean/Join functions and compares the exact production Rust
helper in an owned portable probe, retaining both executable bytes and drivers.
It checks drives/UNC/verbatim/device/postClean and raw UTF16/WTF8 inputs.
The copy/namespace adaptations are declared in the receipt; it is source-
reference evidence and never substitutes for native Windows CLI/file tests.
SDK BSD source licensing is retained in `windows-sdk-LICENSE.txt`.
See [the ownership decision](../../docs/adr/guard-generated-config-path-ownership.md).

The repaired Linux source5bb receipt retains 25 path cases (21 full reports and
four explicit refusals) and the actual wrong-owner mutant. The 386 portable SDK
comparisons include all34 original Windows Clean examples. The immutable
before-fix25 mapping accounts for all15 original failures; the original20 owner
pairs and later10 fresh pairs are replayed separately. All prior CLI/Scan/ordered
corpora stay required. Final source maps, logs, copied probe bytes and exact
executable roles are retained under
`migration/evidence/guard-doctor-config-paths-770/final-5bb`; native three-platform
CI and independent review remain pending.

**Command tree (#770 slice 1).** `command_tree.py GO RUST REPORT GO_SOURCE [--write-fixture]` walks the
Go oracle's help tree, cross-checks it against the frozen source's dispatcher cases and flag
literals, executes every path/flag/positional/`--` form against both binaries in disposable roots and
requires zero unported paths. It verifies (never rewrites) the committed
`migration/evidence/guard-command-tree-770/inventory.json`, and rejects four actual process mutants
(help text, silent success, usage exit code, JSON key) and three fixture mutants. `run.sh` writes
the `-command-tree.json` receipt; the Go-free replay lives in `rust/symguard-cli/tests/command_tree.rs`.

*Host-dependent probes.* The Go `doctor` command reads every client config below `$HOME`. On Windows a
missing parent directory is `ERROR_PATH_NOT_FOUND` ("The system cannot find the path specified."), which
`guard/internal/discovery.containsNotExist` (`no such file` / `file does not exist` / `cannot find the
file`) does not treat as missing; the empty-HOME probe therefore prints an `mcp servers  error:
discovery: ...` row and exits 1, while the Unix capture exits 0. The generator records this as
`HOST_DEPENDENT` (`host_dependent_reason` on the seven `doctor` probes; payload otherwise unchanged, a
fixture mutant drops the reason). `command_tree.rs` compares those probes byte-for-byte on Unix. Off
Unix it requires exit 1 and empty stderr and compares stdout against the recorded Unix bytes after
exactly two rewrites derived from `guard/cmd/symguard/doctor/command.go` (the `mcp servers  none
discovered` row becomes the `error:` row, and the closing `All basic checks passed...` line becomes
`1 issue(s) found. See details above.`), both sides through the runtime-line mask; only the OS wording
of the error row is free (`discovery: hermes (<root>/home/.config/hermes/config.json): [unsupported]
read failed: open ...: ...`). All other 73 probes stay byte-exact (executed counts are asserted). This
missing-parent Windows output is derived from the Go source; it is **not** covered by a recorded
Windows capture of this replay; the `guard-doctor-oracle` case `missing_client_parents` (which skips
pre-creating the client directories) makes the Windows CI leg record Go's real bytes for the shared
`symbrain guard doctor` handler, pending that CI run.

**Scan discovery boundaries (#770 slice 2).** `scan_boundaries.py GO RUST REPORT` compares the actual
`scan --format=json|table` processes for 453 client-config shapes (JSON syntax/type errors with Go's
`encoding/json` wording, field-name folding, duplicate keys and duplicate `args` slice reuse, invalid UTF-8,
lone surrogates, HTML-escaped output, I/O errors, YAML, plus every prefix of two representative documents
and hand-picked token truncations, where EOF inside a token must report Go's `invalid character ' ' <context>`).
Only runs of consecutive map-ranged finding lines (`server "…" is missing …`, `has unknown type …`) are compared
as a sorted multiset because Go ranges over a map; source-level finding lines keep their order.
24 documented yaml.v3 shapes are executed as explicit residuals and never counted as matches. Eight actual
process mutants (server order, dropped finding, unescaped HTML, hidden parse error, case-sensitive
fields, swallowed type error, reordered source-level findings, token EOF reported as generic EOF) must be rejected.
On Windows the `io-directory` case uses a `host-wording` contract: the whole text after `read failed:` is
ignored, which also hides the Go `open` versus `read` word of the `PathError`; exit, stdout and the finding
frame must still match (Windows wording is derived from the Go source, not recorded). `run.sh` writes `-scan-boundaries.json`.
