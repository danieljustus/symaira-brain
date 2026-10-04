# Reachable positive port slices for Brain #769 and standalone Guard #770

Read-only recommendation, 2026-10-04. Inspected immutable Guard source
`9603d24e6749ee67e5eac1b7ddda6900af022cd5`, final evidence-only HEAD
`549e1c859e9ff8897d447e054ec3fa51b9ac151e`, in
`/workspace/symaira-guard770-doctor-boundaries`. Approved parent is
`8e3b21b4fa649847f5b9a26a80f0c24fa172e1a0`; normal main ancestor is
`e3dbda6cbb95429237d15a9b176209b7d107792c`. Candidate files and targets
were not edited, read for binaries, built or executed. No GitHub writes.

## Concrete recommendation

The next Guard positive port is **ordered unknown-key warnings in Doctor's
configuration loader**. Fourteen additional actual whole-Go invocations exit0
with warning-only configurations that current `known_keys` still delegates.
They cover root keys, unknown tables and arrays of tables, quoted dotted and
Unicode keys, proxy/audit/sequence/spawn/rules/match/remote/allowlist fields.
Ten repeated runs of a multi-warning input produce identical stderr bytes.
This is reproducible positive functionality, not a parser-error waiver.

For Brain, the next substantial positive slice is a **shared resolved global
configuration loader consumed by install and MCP**. Actual frozen Go accepts
the project-local default_profile and chooses it over the global default.
The preserved native executable rejects the former or uses the wrong latter.
The relevant four Brain handler files are byte-identical between its archived
source e867c2bb and inspected HEAD549, so these are source-backed reachability
observations; they are not fresh executable acceptance for HEAD549.

A small companion Brain slice is audit's Go integer flags: populated private
JSONL plus -n 0x2, 0b10 and 2_0 gives Go exit0 and native exit2. Reuse the
existing byte-based Go integer parser through an appropriate shared module.
Do not expand shared Core's dependency graph to CLI/Activity merely to call it.

## Actual routing inventory

| Command | Native dispatcher and remaining delegation | What positive work remains |
|---|---|---|
| `symbrain guard` | Brain adapter only re-exports `symguard_cli::{run,run_at_path}`. version, decide, scan, grants, help and unknown verbs always return Some. Doctor alone can propagate None. | Doctor warning-only configurations below; consumer presentation and full native acceptance. No distinct Brain risk/approval engine. |
| `symguard` | Same handlers; Doctor None becomes explicit unsupported-state exit1 with no Go executor. | Same Doctor states; no separately implemented interpretation. |
| `symbrain audit` | Dispatcher wraps run in Some. Parsing None is converted to native usage error; log errors are native errors. | Go base0 integer semantics, complete byte JSON quoting and raw errors/flags; not a new command port. |
| `symbrain config` | path/get/set and missing/unknown subcommands already native. Leading flags and bare '-' return None; -- terminator is native. | Leading --help/-h/unknown and '-' are actual Go exit2, so this is finite error/help completion, not a positive port. Stored-file get/set remain separate from runtime resolved configuration. |
| `symbrain install/uninstall` | Unconditional Some. Internal parser/resolver None returns native usage; missing optional config is ordinary state. Six supported harness installers are native. | Shared config resolution: project default, global→project→environment precedence, known typed fields and malformed-file refusal before writes. Keep AtomicFile/symlink/private modes and dry-run behavior. |
| `symbrain mcp/serve` | Unconditional Some. Native gateway, embedded stores and broker; optional child discovery returns omission diagnostics. Optional audit None does not invoke Go. | Consume the same resolved global config: reject malformed global/project config, honor module/path/identity/audit/pattern settings; then full transport/profile/lifecycle coverage. No fictional proxy command port. |

The first 15 routing probes use a preserved historical Brain executable with
a private executable fallback sentinel returning93. Only config --help, -h,
-unknown and '-' invoked it. install/uninstall dry-run, config path/get,
audit errors, MCP missing-profile and Guard scan/grants/help did not. These
observations support the static dispatch inventory, not a current binary claim.
The CLI-tree ledger's green rows cover bounded cases; they do not prove every
configuration or flag state. Its prose inventory also has stale generic exit
codes; the actual frozen Go results here use 0,1,2.

## Full remaining Doctor None inventory

| Boundary | Reachability and Go result | Next disposition |
|---|---|---|
| Unknown root/nested keys | Proven warning-only exit0; also warnings before later semantic exit1 | First positive slice: reproduce ordered warning metadata and quote/raw-path bytes. |
| Inline/defaults/struct arrays | Former healthy None; source9603 now admits typed valid and semantic-error forms | Preserve current30 typed forms and old9 closure; do not recount as new remaining states. |
| Known scalar/list/table type mismatch | Reachable TOML decoding errors before validation | Exact typed decoder wording/order later; never reinterpret as empty/default healthy. |
| Multiple invalid defaults | Reachable Go error with two observed first-invalid fields across original50 runs | Preserve original50 proof and delegation. No new exception, no E-002/map-order claim. |
| TOML syntax diagnostics other than selected missing '=' | Reachable Go syntax errors | Separate exact parser diagnostic work; no blanket substitution. |
| Config stat/read/invalid UTF-8 | Reachable error, with stat vs read stages and platform/raw-path differences | Add actual stage-bound proofs; use GoText; no positive state classification. |
| Audit metadata nonmissing error | Reachable stat error (e.g. file ancestor) | Exact platform errno/raw-path port; pending diagnostics. |
| Anchor read nonmissing error | Reachable read error (e.g. directory) | Exact stage/raw-path port; pending diagnostics. |
| Invalid UTF-8/unpaired UTF-16 JSON strings/keys | Former healthy or typed-error None; source9603 now repairs after original syntax validation | Preserve current35 plus original5 Unicode closure, first-error order, raw numeric spelling. |
| Anchor no first token / serde parse failure after repair | Defensive or unreachable for proven valid syntax; no positive witness identified | Keep conservative checks. No unwrap or assertion to manufacture completion. |
| Discovery source parsing failures | Malformed JSON/JSONC/YAML/known entry types; actual Go error | Exact shared discovery decoder/diagnostics after bounded valid-state inventory. |
| Discovery missing command and URL | Actual Go discovery error; multi-invalid entries may choose different map member | Single deterministic case port first; repeated proof required for multiple invalid entries. |
| Discovery file I/O | Native admitted error subset; wider errors/platforms remain | Preserve original error reports and explicit filesystem hardening contracts. |
| Earlier config/discovery failure propagated from report builder | Same reachable boundaries, not new states | Entire report and warnings must stay buffered until admission is certain. |
| Missing config/audit/anchor | Already-native healthy defaults/not initialized/pending states | No new port required. |

Unknown defaults *keys* are dynamic capability names and do not warn: the
actual defaults-dynamic probe exits0 with empty stderr. A type-error input
containing an unknown key exits1 without warnings, while a decoded config
with an unknown key plus sequence validation error emits its warning first.
BurntSushi metadata includes unknown table parents and repeated child keys
for arrays; root-array produced repeated owned.nested lines. Quoted dots and
Unicode require TOML-key representation followed by Go %q representation.
Sorting keys, joining segments with '.', deduplicating warnings, or warning
before all known fields decode would change the observed contract.

## Dependency order and acceptance for implementation

1. Guard warning collector: retain typed decode-before-warning semantics;
   collect exact source-ordered undecoded key metadata using TOML source spans,
   including parents, inline tables, AoT duplicates and quoted key segments.
   Return buffered GoText warnings with decoded configuration. Shared Doctor
   runner needs stderr plumbing without changing Guard policy/kernel APIs.
   Flush once only after whole-report admission; a fallback must emit neither
   a partial native report nor duplicate warnings. Both entrypoints consume
   identical reports. Preserve all unknown and multiple-default gates until
   their specific expanded corpus passes. Mutants must drop a warning,
   reorder it, emit it for a type failure and emit twice on fallback.
2. Shared Brain resolved Config in the appropriate Core/config layer:
   model typed defaults and presence separately from stored TOML editing.
   Apply actual configkit global file→project file→environment semantics,
   including explicit false/zero, empty environment values and loader-stage
   failures. First use install default_profile; next use MCP module/path and
   identity injection settings. Existing Gateway::with_identity_injection
   already supplies the native consumer seam, but CLI never calls it.
   Audit enabled/verbose and patterns enabled/threshold require separate
   consumer integration tests; native patterns currently promote with3.
   Test the actual executable with empty PATH/private roots and observed
   foreign child request bytes. Mutants remove project merge, choose global
   over project, ignore explicit false, and start a child on invalid config.
3. Audit flag/parser completion and finite config leading-flag completion:
   full original stdout/stderr/exit/state comparisons plus seeded JSONL.
   Include Go unsigned-accumulation/error precedence from the Activity fix,
   all base0 regimes, signed boundaries, underscores and raw invalid bytes.
4. Remaining deterministic Doctor filesystem, single-invalid discovery and
   typed diagnostic ports, with exact original failing input closure. Follow
   by broader valid discovery Unicode/null/duplicate/source-format inventory;
   don't infer missing positive states merely from .ok()? in a parser.
5. Current native3 acceptance for shared Guard and complete Brain consumers,
   on exact source-bound binaries. Native Rust functionality does not remove
   optional vault/worker subprocesses or authorize installed Go removal.

Guard must depend only on Core/Guard domain/audit primitives, not Managed or
Brain gateway. Brain owns exposure shaping; Guard owns per-call conduct.
The common config slice does not add risk classes, approval prompts or
schema pinning to Brain. Raw errors use shared GoText/Go quoting helpers;
filesystem nofollow/private-mode/audit failure-deny behavior remains intact.
Keep files below400lines and use already-pinned parsers; no new parser needed
before a concrete source-supported gap proves it necessary.

## Preserved evidence and practical limits

Raw outputs in all receipts are base64 with actual exit and exact inputs.

- `/tmp/symaira-brain769-positive-slice-probe.json`: 32 cases plus10 repeated
  multi-warning runs;14 healthy warning configurations, two error controls,
  one dynamic-default healthy control and15 routing observations.
- `/tmp/symaira-brain769-config-reachability.json`: nine common config
  observations; two positive project-default differences and four malformed
  config refusal differences; three matching controls.
- `/tmp/symaira-brain769-audit-positive.json`: four populated-log integer
  probes; three positive Go/native differences and one matching octal control.
- `/tmp/symaira-brain769-go-buildinfo.txt`: actual Go1.26.7 executable module
  metadata, pinned BurntSushi1.6.0/CoreKit0.17.0; executable digest in receipts.
- `/tmp/symaira-brain769-slice-probe.py` and config-probe.py: exact private
  helpers. All subprocesses ran under subreaper/umask022 with cleared env,
  empty PATH and synthetic owned configs; no host Keychain/live service use.

This totals45 distinct whole-Go observations plus10 repeated Go runs;28
archived-native observations. The historical native source is explicitly
e867c2bb88b9931f83f93ad7e880b84c9a0260f8, binary SHA
0bfd2a5f84a70c3b4d81da2b9621b95540367fe1a8d2c5f58bf3fb20bcd62682.
Those four inspected Brain handlers have an empty diff to HEAD549. This is
Linux-only slicing evidence, not approval of candidate549, broad #769/#770
closure, native Windows/macOS proof or installed cutover. Original50 invalid
defaults and all previous124/80/94/raw/kernel proofs remain unchanged.
