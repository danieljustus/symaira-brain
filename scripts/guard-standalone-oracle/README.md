# Standalone native Guard process proof (#770)

Run from the repository root with pinned Go 1.26.7, Rust 1.98 and Python 3:

```sh
scripts/guard-standalone-oracle/run.sh /tmp/guard-standalone.json
```

The runner archives the complete immutable Go tree `dcddcef0df5789123c7c9a7ebe6e01f10e941f2c`, verifies every original Go source and module input byte, then builds only the supplemental `main.go.txt` entrypoint over the unchanged public Guard command packages. The frozen repository has no standalone Go main. The new Rust binary has no Brain CLI dependency or Go fallback. Tests use disposable HOME/XDG roots and empty PATH, with no operator configuration, credentials, provider calls or real MCP server launch.

All 80 selected process cases execute on each native platform. 76 compare full stdout/stderr/exit and file modes/content; owned root paths and validated truthful SDK lines are the explicit runtime differences. Real audit ID/time fields must fall inside the execution window and agree with each other before only those fields are replaced for comparison. Complete raw bytes are retained. Three doctor diagnostic states remain unported and must explicitly fail closed with exit 1 and no stdout. One inherited audit-open error diagnostic differs but both actual handlers must return deny. Those four cases are never labeled byte parity. The platform-specific frozen broken-output case is excluded from this selected portable corpus and is not counted as passing.

The receipt records each case, process output, state, Go/source/binary hashes and candidate revision/dirty status. Production Go and existing frozen fixtures remain unchanged. Native Linux/macOS/Windows jobs run the gate and retain artifacts even on failure. Completion of #770 additionally requires the remaining diagnostic/output-boundary ports, complete native acceptance and truthful command/library inventory; this scoped runner alone does not meet zero-unported acceptance.
