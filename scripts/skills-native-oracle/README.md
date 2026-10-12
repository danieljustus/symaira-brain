# Skills764 native process gate

The initial preparation record below predates native execution; the current
corrected-reference admission checkpoint is at the end of this document.
The immutable case plan contains 276 CLI, 66 platform raw CLI, 42
CLI configuration, 162 MCP and 21 MCP configuration cases: 567 total. The three
negative controls change actual native requests and require a matching actual
baseline, correct exit and a genuine parser/tool error. They never mutate stored
reports or comparator code.

The runner requires absolute existing Go/Rust/Git executables, a successful
fixed dcddcef0 Go1.26.7 build receipt, a clean candidate and fresh external report.
It restores an owned HOME/XDG/project fixture before each flavor, retains all
process argv/stdin/stdout/stderr/exit/PID plus filesystem hashes, and verifies
source and executable hashes after all cases. Twenty-second process deadlines
and owned cleanup are retained in partial reports. There are no live credentials,
server ports or report-field erasure. MCP semantic payload comparison is not a
claim of raw frame byte equality; raw frames remain in each receipt.

Only current-run timestamp fields are normalized after validating their actual
process window. Native empty regular .symskills-lock-<64hex> files implement the
existing SKL-004 concurrency correction and receive a separate validated map.
Directory/file modes, links, every other field and raw file SHA maps remain.
Read-only render observation extends only present-cache status: these fixtures
start without that cache, and the unchanged dedicated #621 gate remains required.

The workflow prepares Linux/macOS/Windows native tests and this gate. Cargo
locked/type checks, strict lint, proper MCP workspace fixtures, the existing
Skills gates and independent review are additional acceptance requirements.
Copied SDK source and case generation do not satisfy any native gate.

The F7/F8/F9 successor appends `json_cases.py` inputs and the complete 26 original
independent review wires in `json-review-inputs/`. Prepared totals become 926
Unix / 884 Windows; five existing actual-input controls remain unchanged and
two new controls require accepted baselines before actual depth/overflow input
mutations. `compare.py` is unchanged. `json_compare.py` admits only the explicit
syntax-error IDs and requires `[1,2,null]`, code -32700 and complete messages,
exits, stderr and filesystem state; valid inputs use the original comparator.
These are prepared plans, with zero current product/compiler/native executions.
See `docs/adr/skills-native-json-depth-float-764.md` for exact owner boundaries.

The F10 EOF successor appends `eof_cases.py`: all eighteen exact independent
line/framed wires, every required-token EOF state, ordinary incomplete/valid
contrasts and syntax-before-type positions. It retains the complete926/884
prefix and appends186 inputs for prepared1112Unix/1070Windows. The original
seven actual-input controls remain unchanged; two new genuine EOF-input plans
require matched valid baselines, exact nil-ID errors, stderr and complete
file/raw-hash/validated-lock state. All nine remain unexecuted. Exact parse-case
IDs use the existing strict comparator, valid cases use the old comparator;
no old response/input is normalized. See `docs/adr/skills-native-json-eof-764.md`.

## Approved side-effect and parser boundaries

CLI `skills status` may persist only its derived comparison cache at
`<CacheDir>/status-render`. The same comparison stage in CLI `skills sync`,
including dry-run, may use this cache; dry-run still must not change installed
skills, retained renders, bases, the library or event logs. The library `status`
and `sync` entrypoints and MCP restore/status callers remain uncached by default.
An explicitly empty cache path also selects temporary staging. Cache hints do
not authorize reuse: actual trees and markers must pass full bounded validation.

The derived tree is swapped atomically; its hint is a separate publication, as
in the frozen Go implementation. A hint failure is reported but does not roll
back an already committed valid tree. A later call validates that tree again
and can repair the hint. The synchronized Unix regression exercises the shared
lock, file and hint operations after root/parent capability capture and ancestor
replacement; it is not a complete CLI or Windows race-acceptance claim.

Disabled profile memory, audit and history must not be initialized merely to
match the frozen Go gateway's unconditional startup side effects. An explicitly
enabled profile audit remains permitted; global audit defaults do not override
a profile opt-out. The real CLI regression
`mcp_profile_startup::disabled_memory_and_history_do_not_create_startup_state`
checks both opt-out and explicit audit opt-in without a Go fallback.

`startup_controls.py`, invoked by this runner, adds five real Go/Rust process
pairs for absent/false/true history configuration, audit profile/global
precedence, and disabled/enabled memory. Each pair retains raw process and full
filesystem observations. Only exact startup-owned memory DB/key, audit, and
recipe-history paths can be classified as legacy-versus-target effects; all
other response, file, mode, link, hash, and Skills render/cache differences
remain strict mismatches. The comparator never edits captured observations or
uses global path exclusions. The parent-owned native run must still execute and
review these controls before claiming acceptance.

Malformed transport input without a safely determined known Skills owner keeps
the inherited JSON parser and diagnosis in both line and Content-Length modes.
No prefix search or permissive partial parser establishes ownership. Valid
owned Skills input still uses its existing bounded, ordered outer decoder.

## Full stdout prerequisite

The gate checks the real stdout sink before its expensive case matrix. Linux
uses `/dev/full`. macOS uses one owned 10 MiB HFS+ image, mounted at the platform
default `/Volumes` location, with a measured `ENOSPC` write. It never fills the
host filesystem and detaches only the image whose exact identity it created.
Windows retains its existing read-only sink cases and has no full-device case.
The partial report retains prerequisite commands, bytes, errno and cleanup.
Passing this prerequisite does not establish Go/Rust process parity.

These are explicit corrective contracts, not permission to rewrite historical
captures or erase files/errors from comparisons. The frozen process gate stays
strict for the full protocol and unowned state. The ordinary MCP/JSON/EOF and
permission-denied pairs now route through the same exact startup assertions as
the five dedicated controls, rather than failing before those controls execute.
`ordinary_startup.py` binds this exception to the exact disabled-memory/audit
fixture, absent global config and overrides, and the fixed Go oracle. It validates
the historical startup effects before constructing separate comparison views;
no captured row, cache, marker, hash, input or global comparator is rewritten.
Actual-input controls also require unchanged initialization and full catalogs.

Windows startup privacy requires native owner/DACL observations from
`startup_privacy.py`, not Unix-style mode bits. Missing/incomplete ACL probes,
null/empty DACLs, non-owner ownership, broad allow grants and unreviewed ACE types
fail closed; Unix keeps its strict POSIX checks. The policy's synthetic tests do
not prove native Windows execution or target permission creation.

Run the focused structural checks with
`python -m unittest discover -s scripts/skills-native-oracle -p test_startup_controls.py`.
Rust config-validation process regressions are in
`mcp_profile_startup::audit_environment_override_does_not_bypass_config_validation`
and `mcp_profile_startup::audit_environment_override_applies_after_valid_or_missing_config`.
Compiler/CLI-MCP replay, real Windows ACL observations, all native targets and
independent review remain required. A focused test or Go-only diagnostic pass is
not Linux/macOS/Windows Rust acceptance.

## Checkpoint publication

Every existing checkpoint retains the same complete raw JSON payload, but is
written to an owned temporary file beside the report, flushed and synced before
atomic replacement. Cancellation during publication leaves the last complete
report readable; it does not turn the unfinished case into a completed case.
`python scripts/skills-native-oracle/test_checkpoint.py` verifies a failed
replacement, a real killed writer at the publication boundary, and a successful
retry. The native workflow runs this check before its process matrix.

The workflow has a bounded 90-minute job budget because the previous 45-minute
budget cancelled Linux and Windows collection. Twenty-second individual process
deadlines, every declared case, negative controls, and mismatch failures remain
unchanged. The native inventories remain 1,112 Unix and 1,070 Windows cases.

## Deterministic owned Git stat fixtures

`copytree` changes file inode/ctime, which Git normally refreshes into its raw
index. Two unchanged frozen-Go restore dry-runs consequently produce different
index hashes even though the complete tool result agrees. These owned fixtures
set Git's supported `core.trustctime=false` and `core.checkstat=minimal` through
their isolated environment and prime the real index after fixed timestamps,
before retaining the seed. Ordinary Git diff refresh remains enabled: disabling
`diff.autoRefreshIndex` falsely adds unchanged files to `changed_files`.

The environment is identical for both actual binaries, and only belongs to
owned fixtures. No production Git settings, frozen Go source, historical raw
observations, input case inventory, filesystem/hash comparator or assertions
are changed. `test_git_fixture.py` executes real Git over two freshly cloned
fixtures, requires exact unchanged index bytes, and verifies genuine source
changes are still detected. Full native and independent review remain required.

Applying restores necessarily replace files and refresh volatile stat words.
The separately approved [stat-only contract](../../docs/adr/skills-owned-git-index-stat.md)
now retains full raw index bytes in `raw_git_indexes` and validates the SHA-1
v2 structure before comparing non-mutating views. Only ctime, mtime and inode
may differ in the exact owned applying-restore fixtures. Device, UID, GID,
size, modes, blobs, flags, paths, TREE bytes and every other observation remain
strict. Unsupported structure fails; this is not a `.git/index` exclusion.
Run `test_git_index.py` through unittest discovery for the cumulative controls.
Native receipts include all tracked Rust workspace inputs, Cargo manifests,
lockfile and toolchain, not only the directly affected crates.

## Corrected reference admission checkpoint: skills-control-nofollow-v1

The default invocation remains `--reference historical`: it admits only the
original dcddcef0 receipt/executable and uses the original startup binding.
Historical results (903 matches, 209 failures, zero original controls) are NOT
corrected-reference evidence. The earlier source-preparation ADR remains an
immutable record; this section documents the subsequent producer wiring.

Both the existing `scripts/skills-argv-oracle/build_go.py` and this `run.py`
accept only the additional named `--reference skills-control-nofollow-v1`.
There is no source-pin, receipt-hash, arbitrary-binary or environment override.
`skills_reference.py` records the exact local source commit
`ce909711d6e7125d281840d14744e7aa987f626f` under
`refs/heads/oracle/skills-control-nofollow-v1`. Independent review132 approved
the actual retained Go1.26.7 darwin/arm64 build receipt for this source, with
SPEC/parity and QUALITY/security both APPROVE. The registry now admits only
that exact receipt on `("darwin", "arm64")`. All other native targets remain
unregistered and fail closed. This admission delta still needs its own
independent same-card review; it is not full native parity or publication.

The exact corrected oracle tree is the ENTIRE dcddcef0 Git tree with only these
four reviewed Go files replaced/added from
`ee185858c2468e7644c6410d3676cf389cf6e518`:

- `internal/skills/skill/load.go`
- `internal/skills/skill/root_unix.go`
- `internal/skills/skill/hardening_test.go`
- `internal/skills/skill/root_control_unix_test.go`

Its 2439 path/mode/blob rows have SHA256
`8161fed7fef8b3e48b318966c0693ebc76c585b942b1e04efece858d3bf6436f`,
over UTF-8 `json.dumps(tree, sort_keys=True, separators=(",", ":"))`, where
`tree[path] = {"mode": git_mode, "blob": git_blob}`. The existing isolated
builder verifies every on-disk byte/index/mode against the pinned commit, then
this full-tree identity. Current branch HEAD is NOT an eligible oracle: its
other Go startup, frontmatter and dependency changes are not this correction.

The complete local source tree, index, bytes and native modes were compared
before and after the source-only commit. A local branch is NOT fresh-remote
reachability evidence. The integration owner must separately authorize ordinary
publication of this exact source branch,
verify that a fresh remote fetch resolves the pinned commit, and retain the branch
through integration (no squash-only replacement or branch deletion). No tag,
release or publication is authorized by this admission checkpoint.

The retained native131 private HOME/Git clone/offline-module build recorded
`go list -deps -json ./cmd/symbrain` inputs including embeds,
module graph/manifests, the whole selected SDK, native Go environment, VCS build
metadata and executable identity. Review132 verified before/after equality,
2439 source rows, 1940 compiler input files, 11536 SDK files and 58 module entries.
The admission change reads these retained bytes; it does not rerun the compiler,
SDK metadata commands, product processes or mounts and holds no build slot.

The independent anchor files are retained as private evidence, identified by
these portable artifact names and exact SHA256 digests:

| Artifact | SHA256 |
|---|---|
| `corrected-reference-native-review132.json` | `9af483f69b1f2b70ad3a043532eda33a607bbc7511c28c4b55a589204a212377` |
| `corrected-reference-review132-checks.json` | `b3bc6782f1f7f4a0d5e868a73c46e81b74ca4e1dbc7391ac29684608b149e5d9` |
| `native131-go-build.receipt.json` | `7aaacf61391f4edc0d7ad2f0084f9296d14527f500d8a3923bd569a2f585fb74` |
| Retained `corrected-reference-native131/symbrain` | `7047ca4db34406afedc309e86e0eeb6f6d77d0eaeb50584966ec1f598b4dec86` |

Receipt builder inputs stay bound to the original builder candidate
`4261a489c538cf1b1937b2e305fac357ec16829e`, including the then-empty registry.
Neither those historical input hashes nor the receipt are rewritten to match
the later registry addition. Review132 is the independent approval anchor;
the new admission review covers only the registry/tests/docs and affected callers.

An independent reviewer must verify complete successful build receipts on
each additional actual native target before its SHA256 is pinned by
`(sys.platform, platform.machine())` in `CORRECTED_RECEIPTS`. Anchors cannot come from the
receipt being admitted. Recording source/binary hashes is not that approval.
The receipt describes its builder input hashes; adding an independent receipt
anchor is a separately reviewed registry change, not a rewrite of that receipt.
Only after admission review and separate source/publication/resource gates may
the existing native producer run with the same named reference.
It binds ordinary startup to that exact receipt/source identity for every
caller and restores the historical binding after the run. Missing, invalidated,
wrong-reference and equal-historical records fail, rather than relaxing #834.

The complete original 1112 Unix / 1070 Windows cases, all nine original
real-process controls, strict byte/state comparisons and startup ownership
remain unchanged. No captured output is rewritten. Full Linux/macOS/Windows
execution, affected package/caller checks, lint and independent review remain
pending. Run the bounded noncompiling admission check with:

`python3 -B -m unittest discover -s scripts/skills-native-oracle -p test_reference_admission.py -v`

Its mocked SDK/receipt records and reused synthetic startup pairs remain
test-only harness controls, never compiler or native parity evidence. Explicit
retained-file admission mode (no silent skip when evidence is absent) is:

```sh
python3 -B scripts/skills-native-oracle/test_reference_admission.py \
  --reviewed-receipt /absolute/path/to/native131-go-build.receipt.json \
  --reviewed-binary /absolute/path/to/corrected-reference-native131/symbrain \
  --historical-receipt /absolute/path/to/frozen-native-go-build.json
```

Use the stable external HOME/XDG/TMP fixture. This calls `admit_receipt`
directly for the positive case, then rejects test-only mutations of the real
receipt/binary, the retained historical receipt, a missing receipt and an
unregistered key before any product invocation. The optional test-only
`--routing-key linux/x86_64` or `--routing-key win32/AMD64` explicitly simulates
routing on the current host, without replacing any trust pin or claiming foreign
native execution. Python `-O` and `-OO` must exit 1. Full native
producer execution, the original nine actual controls and strict #834 startup,
raw-byte and state contracts remain required, as do broader Brain/Browse/Guard
and release/cutover gates. PR813/source-branch publication remains scheduler-owned.

## Corrected Linux/Windows build-only preparation

The existing `skills-native.yml` now has an explicit `workflow_dispatch` route
for ONE selected `build_runner`: `ubuntu-latest` or `windows-latest`. It has no
new matrix and no Darwin build option. Reuse the independently reviewed native131
Darwin reference; a new build would not be its admitted receipt. The earlier
checkpoint paragraphs above describe their historical phase, not a new grant.

Automatic pull-request/main-push runs retain their complete existing three-OS
job, historical default reference, package/lint/fixture/process checks and raw
artifact upload. They do NOT switch to a self-generated corrected receipt or
skip missing admission to turn green. The original 903/209 historical outcome
is not repaired or relabelled by this build-only preparation. Manual build-only
runs use a separate concurrency group and job name; their success does not
establish the ordinary process job's acceptance.

The manual job fetches the published source branch into its exact local anchor
and calls the EXISTING `build_go.main()` with the named corrected reference and
explicit cache preparation. Its existing checks still require the exact commit,
complete 2439-row source tree, clean candidate, pinned native Go1.26.7, verified
modules, offline build, before/after source and compiler-selected input/whole-SDK
closure, and clean VCS build metadata. No source overlay or trust pin is added.
The program rejects missing/unknown/mismatched runner selections and Darwin.

Each run/attempt owns a fresh external directory named
`skills-control-nofollow-v1-build-only-<run_id>-<run_attempt>`. Creation and output
checks refuse overwrite; there is no cleanup or fallback to prior artifacts.
It uploads the binary, `go-build.json` (including exact command output bytes,
source maps and compiler/module/SDK closure), and raw build stdout/stderr logs,
even on failure. Compiler caches are disposable job-local inputs, not uploaded
as acceptance evidence. A failed/partial build stays failed. The build program
ends at the builder's exit, before `run.py`, startup classification or product
comparison. No receipt is automatically admitted.

Later, ONLY after independent source review and scheduler-owned publication,
the scheduler can run (the selected ref must contain this reviewed workflow):

```sh
gh workflow run skills-native.yml -R danieljustus/symaira-brain \
  --ref "$REVIEWED_PRODUCER_REF" -f build_runner=ubuntu-latest
gh workflow run skills-native.yml -R danieljustus/symaira-brain \
  --ref "$REVIEWED_PRODUCER_REF" -f build_runner=windows-latest
```

GitHub must also have `workflow_dispatch` registered on the default branch;
source preparation alone does not establish dispatch availability. If that
publication prerequisite is missing, it remains a scheduler-owned gate, not a
reason to launch a different workflow or weaken PR checks. The equivalent
existing builder CLI, on an explicitly authorized native runner with the exact
local source anchor and fresh external outputs, is:

```sh
python -B scripts/skills-argv-oracle/build_go.py \
  --reference skills-control-nofollow-v1 --prepare-module-cache \
  --out "$OWNED/symbrain$EXE_SUFFIX" --receipt "$OWNED/go-build.json" \
  --go-cache "$OWNED/go-cache" --module-cache "$OWNED/module-cache"
```

The parent directory and both cache directories must already exist. On this
Mac, a healthy `dev-external --status` and fresh exclusive A/jobs2 grant remain
mandatory before any compiler invocation; this source-only task grants neither.
Actual Linux/Windows build artifacts must next be independently reviewed, then
anchored by a separately reviewed registry change. Only then may a later
scheduler-owned step wire their exact receipts into corrected process runs.
Linux/Windows native comparisons, broader Brain/Browse/Guard checks and
publication/release/cutover remain open.

Noncompiling source controls (not native/compile acceptance):

```sh
python -B -m unittest discover -s scripts/skills-argv-oracle -p test_build_go.py -v
python -B -m unittest discover -s scripts/skills-native-oracle -p test_reference_admission.py -v
```

The first executes the actual workflow Python and real builder argv parser,
with a hard test boundary before SDK execution and only failed test receipts.
The second exercises the real process entrypoint's missing-platform and
unreviewed-receipt rejection without executing any product. Simulated platform
keys and synthetic negative records are unit controls, never native evidence.

## Reviewed Linux/Windows admission and corrected-process source

Independent review147 approved the actual two native Go1.26.7 BUILD_ONLY
receipts, with SPEC/parity_BUILD_RECEIPT and QUALITY/security_BUILD_RECEIPT
both APPROVE and zero process comparisons. Its portable `verdict.json` digest is
`267ba0917072035f2e4ed846d52dcc776c1adc9c11be7ab47f369bb08000ae7f`;
`artifacts.json` is
`b3113a1b0d6abb5455e1cbafd760e7d1cb86302d7e082518726c7ccebb1db162`.
This source delta requires separate SOURCE_ONLY review and admits exactly these
fixed routing keys:

| Routing key | Run / attempt / job / artifact | Receipt SHA256 |
|---|---|---|
| `linux`, `x86_64` | `38186442269` / `1` / `114609664447` / `11719806790` | `02d809361504618991e8c76beed233767a0f883d42764001e9176524261e9ad6` |
| `win32`, `AMD64` | `38186449761` / `1` / `114609687444` / `11719657439` | `58392844cb3f90d6b9b95961daade3a90c43d72d43f9ad5b147783600fd9f4b8` |

Their GOOS/GOHOSTOS are respectively `linux` and `windows`, with GOARCH and
GOHOSTARCH both `amd64`. No global platform-name normalization is introduced.
Both receipts bind the same complete 2439-row corrected tree above, the original
builder HEAD `578d201f579c45d13af3c076277e87ef8dcf6f74`, and equal before/after
compiler/module/whole-SDK closures independently verified by review147. The
receipt pins authenticate those original inventories, not a fresh self-report.
Darwin's review132 pin, native capture and historical/invalidated data remain
unchanged. The earlier unregistered-platform paragraphs record prior phases.

The EXISTING `skills-native` job now also accepts one manually selected runner
with `mode=corrected-process`. It first calls `fetch_reviewed_reference`, which
uses the runner's existing `gh api` and read-only Actions permission to obtain
the fixed original run, job, artifact metadata and ZIP. It rejects missing,
expired, unsuccessful, wrong-attempt/head/repository/ID/URL/digest inputs before
any product use. ZIP sizes/hashes and binary hashes are fixed in
`skills_reference.py`; no caller URL, environment pin or rebuild fallback exists.
Only the four exact ZIP members are written to a fresh runner-temp directory;
the original receipt and binary bytes are re-admitted by the real `run.py`.
Raw retrieval exits/outputs and partial process observations are uploaded.

This bounded mode builds only the Rust CLI and Git fixture needed by the
unchanged producer. It executes all 1112 Unix / 1070 Windows cases, the original
nine real input negatives and #834 startup controls; no comparator, count or
security predicate changes. Broader affected package/lint/Guard/coverage/native
and integration gates remain required separately. It does not rebuild Go or
duplicate Darwin. BUILD_ONLY remains an explicit separate mode (the default);
automatic PR/main runs remain three-platform HISTORICAL DIAGNOSTICS, not
corrected acceptance. There is no additional matrix job.

After independent SOURCE_ONLY review and scheduler-owned publication of this
exact candidate, the next native invocation is the existing workflow, once per
selected runner, with `-f mode=corrected-process -f build_runner=ubuntu-latest`
or `-f mode=corrected-process -f build_runner=windows-latest`. Use the reviewed
producer ref, retain both complete raw captures, and independently review the
exact run/head/artifacts before promotion. Expired artifacts are an external
evidence gate, never permission to regenerate or substitute an unreviewed oracle.
This source preparation itself executes no native comparisons and grants no
publication, release, cutover or Go deletion authority.
