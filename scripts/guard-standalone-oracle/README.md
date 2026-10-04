# Standalone native Guard process proof (#770)

Run from the repository root with pinned Go 1.26.7, Rust 1.98 and Python 3:

```sh
scripts/guard-standalone-oracle/run.sh /tmp/guard-standalone.json
```

The runner archives the complete immutable Go tree `dcddcef0df5789123c7c9a7ebe6e01f10e941f2c`, verifies every original Go source and module input byte, then builds only the supplemental `main.go.txt` entrypoint over the unchanged public Guard command packages. The frozen repository has no standalone Go main. The new Rust binary has no Brain CLI dependency or Go fallback. Tests use disposable HOME/XDG roots and empty PATH, with no operator configuration, credentials, provider calls or real MCP server launch.

All124 selected process cases execute on each native platform. The corrected warning slice requires122 full stdout/stderr/exit and file modes/content matches; owned root paths and validated truthful SDK lines are the explicit runtime differences. Real audit ID/time fields must fall inside the execution window and agree with each other before only those fields are replaced for comparison. Complete raw bytes are retained. Two selected TOML diagnostic states remain unported and must explicitly fail closed with exit1 and no stdout. The known Unix directory audit-open diagnostic now matches; Windows retains the deny-only audit wording contract pending native proof. These are selected-case counts, not every unsupported diagnostic shape. Typed anchor cases preserve duplicate/null/folded fields, numeric spellings and first-error order; unknown values use raw JSON and do not introduce float64 overflow errors. Three actual-process output mutants hide a config error, hide an anchor error and change audit-failure deny to allow; each must be rejected. Both full reports and controls are native CI artifacts. The platform-specific frozen broken-output case is excluded from this selected portable corpus and is not counted as passing.

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
It executes78 actual process cases:67 complete comparisons and11 explicitly
retained TOML decoder/map-order gates. Three actual mutants remove a
configured inline allowlist, gate a repaired healthy anchor and hide a later
typed anchor error. All original124 input bytes are retained; the three specific unknown-key inputs
across these two corpora now require complete warning/report equality. All
remaining decoder/map-order gates keep their original refusal assertions.
The [remaining boundary inventory](../../migration/guard-doctor-boundary-inventory-770.md)
distinguishes healthy Go-only states from decoder errors and warning-only states.

The `-config-warnings.json` receipt ports ordered unknown-key stderr for
successfully typed configurations. Linux executes54 cases:46 full comparisons
and8 retained typed/alias/discovery/map-order gates, plus ten actual Go repeats
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
