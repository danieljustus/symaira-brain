# Skills MCP profile, discovery and versioning tools (#764, slice b)

Status: implemented and locally verified (macOS; the first 141 cases also on Linux
`rust:1.98-bookworm` as uid 1000, the review-extended 197-case corpus not yet
rerun there) on top of slice (a). Native Windows CI is pending. Salvaged from
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
4. **Go path semantics for names.** `filepath.Join(lib, name)` is join-then-clean
   and an absolute `name` is *not* a new root (`Join("/lib", "/abs")` is
   `/lib/abs`). `Path::join` replaces the prefix instead, which would have let an
   absolute `name` aim `skills_restore`/`skills_history` at any repository the
   caller can write (extraction runs before the dry-run return). Every
   name-derived path therefore goes through `gotext::join`; there is no
   `Path::join` on a caller-controlled element. **#918 supersedes restore's
   deliberate reproduction of relative `..` traversal:** restore now calls the
   existing skill-name validator before any filesystem access, including dry-run.
   This requires a lowercase alphanumeric-dash slug (at most 64 bytes, no leading,
   trailing or consecutive hyphens), rejecting empty, dot, parent, slash,
   backslash, NUL and absolute names without normalizing them. Absolute restore
   names are rejected, not re-rooted or allowed to select an outside repository.
   History retains the original Go join semantics.
5. **Git exactly as Go.** All repository access goes through `git` on `PATH`
   with `GIT_TERMINAL_PROMPT=0`, `LC_ALL=C`, combined stdout+stderr, the
   `symskills <symskills@localhost>` identity and `commit.gpgsign=false`.
   A missing binary means "not versioned". Restore extracts through `git
   archive` into a capability-confined directory (traversal, absolute and
   escaping links rejected, size/entry bounds), validates the restored tree
   before any write (symlinks must satisfy Go's `Clean(Join(Dir(name), link))` is
   local and not `.`; a failed removal before link creation is an error), copies it into a sibling staging directory, swaps entries
   while keeping `.git`, and records a forward commit.
6. **Restore sync.** `sync=true` reuses the shared install layer; the added
   `install::sync_selected` is `sync` over an already filtered status list,
   which is what Go's `install.Sync(stale, ...)` receives.
7. **Catalog.** The five registrations (description, schema, annotations) are
   Go's; the eleven Skills tools are exposed in Go's relative order. Skills vs
   Usage ordering in the whole catalog stays the deferred reorder.

## Consequences and residuals

* TOML parse/type error text (BurntSushi vs `toml_edit`) and YAML frontmatter
  parser text differ; class, code and the Skills-owned prefix are asserted.
* A restored revision containing a dangling symlink fails validation in both
  implementations, but the shared bundle loader words the I/O error differently
  (`statat x: no such file or directory` vs strerror text); not in the corpus.
* Lock files are masked only below `home/`, `rendered/` and the install `base/`
  store; the Go and Rust install layers name them differently.
* The shared install layer writes `output_digest`/`output_manifest` into
  `.symskills.json` and names its lock files differently from Go; the replay
  compares every other marker field and ignores lock files.
* **Frozen Go defect superseded by #918.** The original port deliberately
  reproduced relative-name traversal during restore, even with `dry_run:true`.
  Go and Rust now reject invalid names together before repository access or
  extraction. Rust uses Go `%q` quoting in the shared validator so control-byte
  (including NUL) errors match exactly. The existing required-arguments error
  retains precedence for an empty name or revision.
  The 197-case oracle stays pinned at
  `3d9a89e7a2ee201d93c74501a10eb2af3f87e620`; its recorded bytes are unchanged.
  Following the migration's explicit security-contract exception pattern (not a
  silent repin or a skipped case), `tests/common/skills_restore_contract.rs`
  enumerates 13 affected cases: one relative traversal and twelve absolute
  restore names. Each now requires the exact validation reply and an unchanged
  pre-call filesystem/git snapshot. One subsequent history reply omits only the
  refused restore's forward commit; the remaining recorded history bytes stay
  exact. Negative controls reject writes and error-class changes. Other cases
  keep the original byte/side-effect comparison. `skills_history` can still
  read a repository selected by relative parent components; changing its
  read-only name contract is outside #918's restore fix.
* Rust adds input bounds Go lacks (16 MiB profile, entry and git-output
  limits). Invalid UTF-8 in git output is decoded lossily where Go escapes it.
* Windows path cleaning is unix-style lexical; the replay is `cfg(unix)`.
* The `sync=true` outputs are produced by the existing YAML encoder; the
  #835 encoder fork is not included, so frontmatter edge cases that depend on
  it are not asserted here.
