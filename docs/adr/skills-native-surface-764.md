# Skills native CLI and embedded tools (#764)

Status: source preparation only; no candidate build or product execution.

The accepted product boundary keeps the skill SSOT in process. The remaining
Go dispatch was keyed to library contents, configuration and arguments, so a
populated library could change the implementation selected for the same command.
The prepared change makes all six real CLI verbs and all eleven embedded tools
use the shared Rust loader, registry, renderer, install/status/sync and metadata
functions. No external symskills service or generic tool framework is introduced.

## Routing and configuration

`cmd/symbrain/cmd_skills.go` exposes list, status, targets, log, sync and doctor.
Its flag parser normalizes the complete argument vector, then stops at the first
positional argument. Each verb accepts target/scope even where the handler
ignores them. Unknown and malformed forms still return native parser errors;
they cannot request Go fallback. Populated metadata includes full timestamp
precision, per-target installations, latest event attribution and last-used
access-time rules. The original raw sync codecs and fixtures are unchanged.

CLI configuration follows the actual global/project/environment load and resets
the entire partial result to defaults on any load error. The embedded gateway
registers tools with Version/Wrap only, so its Skills options backfill Defaults,
without CLI config.Load, event logging, ProjectDir or marker metadata fallback.
No caller registers custom targets or capabilities from configuration; accepting
the untagged Targets field would invent behavior rather than port this route.

The gateway preserves the raw Skills argument object before Value conversion.
Its private decoder retains duplicate-field error order, null pointer defaults,
Go string repair and profile-selector-before-bundle argument checks. Memory and
other embedded adapters continue using their existing Value paths. Outer gateway
framing and JSON admission still apply; this seam does not bypass them.

## Read observations and write authority

Read-only installed-marker observation is separate from the existing strict
writer guard. Future schema versions can be observed without granting authority
to rewrite them. Syntax failure yields no decoded fields; a field type failure
retains the other fields and the first Go-shaped diagnostic. Cache drift remains
the independently reviewed SKL-008 extension: compare freshly rendered bytes,
never copy cache into the SSOT, and present linked mode only after verified safe
cache identity. Unsafe or unreadable cache/marker paths remain fail closed.

MCP's omitted ProjectDir has a specific legacy install-base contract. The new
InstallOptions flag is false for every existing caller and true only for this
adapter's project scope. It preserves the legacy base identity while retaining
the actual current-directory target destination. It does not change general
project SDK behavior. Restore uses the existing per-skill owner lock, snapshots
dirty data before a forward commit, validates extracted bundles before replacing
them, never resets history, and syncs only that skill's eligible selected rows.

Bounded readers, resource inventory, capability-rooted links and process locks
remain explicit corrections for #476/#490/#457. Profiles, events, tar snapshots
and discovery acquire bounds rather than allowing an added route to bypass
those contracts. The lifecycle log admits up to 16 MiB: the existing 1 MiB
rotation threshold can legitimately be exceeded by one appended record, so it
is not a safe read limit. Final symlink/special-file refusal remains explicit.
Private JSON syntax/string helpers reuse existing reviewed source; their source
hashes are retained without adding a Skills dependency on Guard policy.

## Executable ownership and dependencies

Target evidence and per-skill Git use a private SDK-shaped owner lookup before
changing the Git working directory. It preserves native path units, lexical
filepath.Join/Clean, search order, effective execution access and Go ErrDot.
The existing exact tar 0.4.46 and Core/tempfile edges are reused. Windows alone
adds the already pinned same-file 1.0.6 for Lstat-style file identity: path spelling
is insufficient for Go's implicit-cwd/absolute-PATH owner comparison. Its existing
lock package/checksum is retained; no new dependency version was selected.

This source checkpoint is not full compatibility approval. Conditional Go
GODEBUG bisect stack/reporting forms currently have no native stack-identity
implementation; universal predicates are prepared, and conditional/reporting
forms are explicit blockers rather than guessed owner permission. Exact Windows
raw argv0/PATHEXT/BAT-CMD/Lstat sharing, typed TOML diagnostics and raw-path JSON
serialization require native execution and independent review. A later gate must
resolve these differences; no error projection or compatibility exception is
authorized by this ADR.

## Proof and pending acceptance

All original parent sources, frozen Go maps and existing #621/#794 reports stay
intact. The prepared native three-OS gate contains 567 real process cases plus
three actual changed-input controls, isolated HOME/XDG/project roots, real owned
Git history and six target installations. CLI stdout/stderr/exit compare exactly;
MCP compares complete Skill catalog, initialization, error metadata and payload
fields, while retaining raw transport separately. Files/directories/modes/links
are compared. Only validated current-run generated timestamps and regular empty
SKL-004 lock files are treated separately; raw file hashes remain retained.
Timeout cleanup retains the failing child output and owned PID rather than
discarding an unfinished case. No process or SDK proof has run at this head.

Required next steps are Cargo locked graph/type checks, strict lint, all affected
and proper-fixture workspace tests, fresh original preflight/raw-argv/render-drift
gates, all prepared CLI/MCP controls on Linux/macOS/Windows, and independent full
review. Issue #764 and the overall Rust release remain open until those pass.
