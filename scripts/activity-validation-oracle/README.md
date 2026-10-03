`run.sh OUTPUT_JSON` executes the immutable production Go CLI at
`dcddcef0df5789123c7c9a7ebe6e01f10e941f2c` and the candidate native CLI in
isolated HOME/XDG roots with an absent Go fallback. No credentials or live
provider endpoints are used; databases contain only the original synthetic
activity fixture or remain empty.

The five unchanged ACT-CLI-001..003 report cases are generated from the original
Go harness and compared through the native executable. An additional 173
portable validation cases cover Go integer bases/overflow, all flag boundaries,
profile precedence, date/time diagnostics and bounded query validation. Twelve
Unix cases preserve raw invalid UTF-8 and literal RuneError bytes separately.
Every stdout/stderr byte and exit code must match. No output is normalized.

The runner writes actual observations, source/binary hashes and a Go-only
fixture, then runs the native process fixture test against that fresh file.
Ordinary Rust tests use the retained immutable Go fixture, including all raw
Unix cases. Missing, changed or truncated fixtures fail actual native tests.
Native Linux/macOS/Windows CI retains both fresh reports for fourteen days;
Windows excludes only the twelve Unix raw-argv cases and executes all 173
portable cases. Imports and other activity-store work remain tracked in #761.
