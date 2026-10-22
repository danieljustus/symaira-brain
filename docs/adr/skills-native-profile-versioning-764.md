# Skills MCP profile, discovery and versioning tools (#764, slice b)

Status: implemented and locally verified (macOS and Linux `rust:1.98-bookworm`
as uid 1000) on top of slice (a). Native Windows CI is pending. Salvaged from
PR #813 (head `578d201f5`) where the design carried over; the code was rewritten
against current main because PR #813 depends on its YAML fork, `binary`/`text`
byte-presentation layers and a reordered catalog that are owner decisions.

## Decision

`skills_profile_list`, `skills_profile_resolve`, `skills_discover_sources`,
`skills_history` and `skills_restore` are served natively by
`symbrain-gateway::embedded::skills_tools`, with the library work in
`symbrain-skills` (`context_profile`, `discover`, `vcs`, `gotext`). No Go child
process, no `symskills` binary.

1. **Go handler contract.** Each handler reproduces `internal/skills/mcptools`:
   typed `json.Unmarshal` into the anonymous argument struct (case-folded keys,
   input-order first error, `null` semantics, `*bool` clearing, the
   `struct { Name string "json:\"name\"" }` type text), `exitcodes` kind in
   `_meta["symaira.dev/tool_error"]` (`validation`, `internal`, `conflict`),
   compact HTML-escaped JSON, result bodies as structs so field order equals
   Go's sorted map-key order or struct order.
2. **Raw arguments.** The five tools receive the raw `arguments` member and use
   `symbrain_mcp::raw_skills_repaired` (the only slice (a) file touched, one
   additive export) so absent vs `null` vs duplicates keep Go's behavior.
3. **Defaults only.** The Go gateway registers the tools with default options:
   no config file or env override, `HomeDir` empty (so `$HOME`), no project
   directory, no event log, `vcs.enabled` always on. The native tools do the
   same; project/parent profile contexts and the disabled-versioning branch are
   therefore unreachable from this surface.
4. **Git exactly as Go.** All repository access goes through `git` on `PATH`
   with `GIT_TERMINAL_PROMPT=0`, `LC_ALL=C`, combined stdout+stderr, the
   `symskills <symskills@localhost>` identity and `commit.gpgsign=false`.
   A missing binary means "not versioned". Restore extracts through `git
   archive` into a capability-confined directory (traversal, absolute and
   escaping links rejected, size/entry bounds), validates the restored tree
   before any write, copies it into a sibling staging directory, swaps entries
   while keeping `.git`, and records a forward commit.
5. **Restore sync.** `sync=true` reuses the shared install layer; the added
   `install::sync_selected` is `sync` over an already filtered status list,
   which is what Go's `install.Sync(stale, ...)` receives.
6. **Catalog.** The five registrations (description, schema, annotations) are
   Go's; the eleven Skills tools are exposed in Go's relative order. Skills vs
   Usage ordering in the whole catalog stays the deferred reorder.

## Consequences and residuals

* TOML parse/type error text (BurntSushi vs `toml_edit`) and YAML frontmatter
  parser text differ; class, code and the Skills-owned prefix are asserted.
* The shared install layer writes `output_digest`/`output_manifest` into
  `.symskills.json` and names its lock files differently from Go; the replay
  compares every other marker field and ignores lock files.
* `skills_restore` with a traversing name (`../library/alpha`) reproduces Go's
  extraction/staging path (`filepath.Join(tmp, name)`), which writes outside
  the temporary directory in both implementations. This is Go behavior kept for
  parity and is a candidate Go fix, not a native feature.
* Rust adds input bounds Go lacks (16 MiB profile, entry and git-output
  limits). Invalid UTF-8 in git output is decoded lossily where Go escapes it.
* Windows path cleaning is unix-style lexical; the replay is `cfg(unix)`.
* The `sync=true` outputs are produced by the existing YAML encoder; the
  #835 encoder fork is not included, so frontmatter edge cases that depend on
  it are not asserted here.
