# Preserve daemon session profile and worktree path identity

Status: accepted implementation decision for #772 / PR #801.

The independent review of `c7ad0b5f98365d64139f0f139f936a1ecd08cdef`
found two actual process differences: relative XDG cache paths selected a
relative browser profile instead of Go's temporary-root fallback, and an
incomplete POSIX UTF-8 sequence in the worktree path became one replacement
character instead of two. The original review and observations are preserved.

Decision: the session-profile resolver follows Go's OS cache selection,
including empty HOME/LOCALAPPDATA errors and rejection of relative nonempty
XDG_CACHE_HOME on Unix. On a cache-resolution error the registry selects
TempDir/symbrowse/sessions. Darwin and Windows retain their existing platform
rules; do not apply Unix XDG validation to other platform inputs. Browser
profiles remain independent of explicitly configured state/output caches.

Worktree metadata preserves Go JSON replacement of each malformed UTF-8 byte
both in the current-directory fallback and successful git stdout. Git probing
remains bounded to one second and the child is reaped. A literal U+FFFD remains
distinct from a truncated multibyte sequence. Keep this path compatibility in
the daemon's focused metadata module rather than changing all display strings.

Rationale: profile placement and origin identity are persistent session
contracts. Accidentally selecting the working directory or conflating path
bytes changes observable behavior even when ordinary configurations pass.
Use actual Go/native process comparisons for supported inputs and isolated
platform constructors for environmental states rejected earlier by Go's CLI;
do not claim those constructor tests are complete CLI parity.

Fresh independent correction review and all six native acceptance runners
remain required. The original Windows-ARM Go shutdown failure stays visible.
Registry/autostart contracts and the full #772 cutover remain separate.

The first hosted macOS ARM acceptance reached the unchanged Go daemon but
failed with `AF_UNIX path too long`: the runner's nested default temporary
directory plus Go's HOME-based Darwin socket path exceeded the native socket
limit. Hosted Darwin process/MCP runners therefore use short, private owned
HOME directories under `/tmp`, with unchanged production endpoint rules.
Direct macOS runs retain the required external-volume policy. This correction
does not claim the external APFS/NVMe acceptance required by #790.
