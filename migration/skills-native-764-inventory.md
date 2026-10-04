# Remaining Skills route inventory

This is an inspected-source inventory, not an executed parity report. Parent
integration is ordinary merge of main 5e232700, published preflight cec81cfc and
reviewed render drift 4f3c0032, followed by original-source retention 86118925.
The issue snapshot and Go source map are in evidence/skills-native-764.

| Real Go route | Native owner | Branches retained in prepared cases |
| --- | --- | --- |
| list | skills_cli_list + metadata | empty/populated/broken entries; timestamps; every install; last event/atime; JSON/table; target/scope ignored |
| status | skills_cli_status + install/status | all or explicit six targets; user/project; dynamic/reset config; markers; read-only cache extension preserved |
| targets | skills_cli_misc + targets_status | scope validation; target ignored; evidence/counts/states/runtime capabilities; no custom registration |
| log | skills_cli_misc + install/event_decode | current/rotated log; raw filters; trimming; ParseInt base/sign/overflow; read error; exact flag stop order |
| sync | skills_cli_status + install/sync | all/selected target; project/user; dry-run booleans; dynamic config; preflight and owner locks |
| doctor | skills_cli_misc + config | exact config/path/root shape; load-error defaults; target/scope accepted and ignored |
| root parser | skills_cli + skills_cli_flags | missing verb/help/unknown; missing values; undefined flags; positional and terminator stop |
| skills_list | embedded/skills_library | Defaults only; full bundle metadata; empty issues null |
| skills_inspect / skills_validate | skills_library + wire | name/path; full frontmatter/manifest/body/resources; warnings versus errors; null issues |
| skills_profile_list / skills_profile_resolve | context_profile | global only in gateway; specificity/merge; inheritance/cycle/missing links; sorted links |
| skills_render_plan | skills_render | six default targets; writes default false dry_run; selected profile before bundle; partial target success; profile finish-all then first error |
| skills_install | skills_render + install | opencode and dry_run true defaults; alias; actual render/install; exact project legacy base identity; unsupported target |
| skills_discover_sources | discover | harness roots and explicit missing/invalid paths; raw-content ID even invalid bundle; managed state; sorted deduplicated rows |
| skills_history | skills_versioning + vcs | per-skill repository; default/nonpositive limit; actual fixed locale Git rows; missing repository |
| skills_restore | skills_versioning + vcs_restore | revision resolution; dry-run default; dirty snapshot/conflict; forward commit; validation; selected post-restore sync |
| skills_targets_status | skills_library + targets_status | Defaults and fallback user scope without ProjectDir; real target catalog and capabilities |
| all MCP arguments | skills_args/raw seam | typed struct validation before I/O; duplicate/type error order; null strings/pointers/arrays; branch-specific ignored fields |

The original CLI has no render/install/profile/history/restore verb: those are
MCP tools in this route. No invented CLI verbs were added. The gateway does not
register a standalone symskills process or load CLI configuration. Frozen Go
library APIs with custom-target options remain reference SDK behavior; absent
route registration is not turned into an implicit extension.

Prepared tests are not evidence of execution. See prepared-case-plan.json for
all 567 unique case IDs and the three planned actual input controls. Existing
315 raw-argv, preflight and twelve live render-drift cases remain separate
required gates. Conditional GODEBUG stack/report compatibility, raw Windows
process arguments and exact typed TOML diagnostics remain explicit pending
boundaries in docs/adr/skills-native-surface-764.md.
