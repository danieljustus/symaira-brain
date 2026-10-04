# Remaining Usage credential gates and next bounded scope (#768)

## Inventory at the reviewed checkpoint

This inventory is bound to unchanged source
`8803847278546f3161eaaa84d2e65bc2eec594a2`, evidence head
`eea73b7478e62f1bb174d456bbbecccc1de94034`. It is not a new native acceptance
claim. The source and review-owned target are untouched; all native executions
use retained immutable copies. No Rust/Cargo build was performed.

The following nine groups are every reachable `needs_go_fallback()` return path.
Checks happen in this order; the first matching group routes the entire report
through Go, even if another provider could run natively.

| Group | Exact remaining boundary | Next step and why |
|---|---|---|
| Windows home | HOME path differs from USERPROFILE, including empty/unset choices. Linux has no runtime branch. | Native Windows filesystem/environment receipts and Go-owned root selection; do not infer acceptance from pure path algorithms. |
| Three custom bases | Any nonempty KIMI_CODE_BASE_URL, HERMES_PORTAL_BASE_URL or OPENROUTER_API_URL fails supported_custom_base, even without that provider's credential. | Complete URL families separately: explicit public ports/case then percent paths; preserve trusted HTTPS/private-origin policy and exact constructed/wire URL. |
| Copilot files | With no nonempty UTF8 env token: apps/hosts unsafe read, distinct eligible tokens in the first usable prefix pass, or selected token containing ASCII controls. | Keep nondeterministic map selection and unsafe reads gated. Env preemption already works. A separate header-wire family may prove permitted control forms. |
| Claude map | More than one distinct usable nondefault token and no usable default. The file ambiguity check is currently unconditional, even when an OAuth env source preempts the file. | Preserve ambiguous legacy selection. A later explicit-env preemption family can skip a file that Go never reads; no invented account priority. |
| Nous JWT | Typed Hermes selected JWT expiry outside signed64-bit float-to-int range. The file check also runs when a Nous env source preempts it. | Env-preemption family first; overflow needs actual target/architecture conversion receipts before any native range change. |
| Kimi token source | Credential file unsafe/unreadable, or selected literal token has ASCII controls. File/CLI strategy remains relevant beside API/web sources. | Preserve file ownership/read bounds and fallback-chain semantics; API presence alone cannot erase an available CLI strategy. |
| Kimi device | Unsupported device file access, invalid UTF8, or trimmed device bytes outside ASCII space/graphic. This runs even when no CLI token exists. | Next positive family: regular bounded valid UTF8 without internal ASCII controls; preserve exact Unicode bytes and Go TrimSpace. Other forms retain their current boundary. |
| OpenCode workspace | Nonempty cookie env source plus workspace outside exact wrk_ followed by nonempty ASCII alphanumeric suffix. Without cookie, workspace spelling cannot cause a fetch and is already native. | Prove full normalization and GET/POST/body/query/Referer behavior for complete URL/direct-id families; do not assume parser-only coverage proves transport. |
| Automatic Claude Keychain | Neither OAuth env nor selected file token, and macOS attribute listing finds a valid service. NonmacOS returns false. | Actual native macOS ownership/ACL/expiry/process evidence; synthetic private empty adapters prove ordering only. No operator inventory/read in this lane. |

`needs_go_fallback_for` also declares other_provider_env and
other_credential_source, but the only production caller passes false for both.
Those are not extra reachable gates. Its copilot_file/kimi_cli fields do not gate
anything at this checkpoint. Claude/Codex malformed or missing regular files,
Hermes malformed/expired files, allowed environmental references and constrained
HTTPS/workspace spellings are already native. The capability reader's rejection
of a Claude/Codex unsafe file does not itself create a distinct Go gate here.
Inventorying explicit predicates is not proof that every ungated OS/string or
transport form has complete parity; inherited limitations remain documented.

## Actual read-only evidence

59 owned copied-executable cases exercise the Linux-reachable families:44 require
Go,15 stay native. The immutable public probe reads current production
configured/source/status/Authorization and needs_go using canned401. Actual
current CLI and fresh frozen-Go invalid-flag processes show the existing
admission-before-parsing boundary; all owned regular files, links, directories
and FIFOs remain unchanged. Native FIFO rejection is bounded; no Go credential
FIFO read is attempted. This is not positive native200 or header-stack proof.

Examples demonstrate needless gates without changing them: an explicit Claude
OAuth env token wins over an ambiguous file, and an explicit Nous env token wins
over an overflow JWT file, but both current reports still require Go. A Kimi API
source with no CLI token still requires Go because an unused device_id contains
Unicode. Copilot's explicit env token already preempts its ambiguous file.
These are later coherent families, not permissions to widen the current slice.
Windows-home and actual macOS-Keychain paths remain static inventory only.

A fresh pinned Go1.26.7 private worktree at frozen
`dcddcef0df5789123c7c9a7ebe6e01f10e941f2c` runs39 complete Kimi constructor,
strategy-chain, report, request/raw-header-byte and read-only state cases. The
production Go sources are unchanged.20 device value/trim controls cover Latin,
CJK, emoji, combining marks, RTL digits, NBSP/NEL/line-separator/zero-width/BOM;
eight response/strategy cases cover API short-circuit, API->CLI, CLI->web,
all401,403,429,500 and invalid successful JSON; seven raw/control boundaries,
65536/65537 file limits, missing device and irrelevant-device API-only are kept.
Selected bytes, full reports/meters/source/error, exact methods/URLs/headers/body
and before/after source hashes remain retained. All transport responses are
owned canned values; this baseline bypasses real HTTP header validation and is
not a native/wire acceptance claim. Raw ff/internal-tab canned success therefore
does not justify relaxing those current gates.

All exact inputs, outputs, runners/logs, fourteen immutable predicate/transport
source digests and frozen-Go source/SDK hashes are retained under
`migration/evidence/usage-remaining-baseline-768/`. The original103/36 and older
failed observations remain untouched.

## Authorized next increments and long-term rationale

First, classify Usage argv before credential admission. FlagSet errors, help and
unexpected positionals are independent of credentials and the presence of a Go
executor. Root authorized this as a separate successor scope after the full
review's64 actual triples/32 Go-forward observations classified the current
limitation as inherited. A single parsed Usage invocation must carry Report,
Help or the precise diagnostic outcome through Usage dispatch. Normalize once,
retain first-positional/terminator/help precedence, write raw error bytes and use
the accepted Go quote primitive. Only Report consults needs_go_fallback. This
changes the Usage dispatch arm without rewriting the global argument parser.
Valid gated reports must still forward every original byte to Go. New real
unsupported-source argv cases and mutated classification controls join all
existing unchanged parent gates. No credential reads are needed for diagnostics.

Second, admit the complete regular bounded UTF8 Kimi device family: Go TrimSpace,
exact remaining bytes, strategy-specific identity-header presence, Unicode
values and outer Unicode whitespace. Retain invalid UTF8, internal ASCII-control
values and unsafe source shapes unless a separate complete family proves them.
Do not change token resolution, refresh behavior, request URL trust, owner paths,
API/CLI/web order or source labels. Unicode must not be transliterated, normalized
or replaced. Before admission, compare full native constructor/requests/reports
against all39 frozen-Go values and actual owned HTTP/TLS peer header bytes for
Unicode plus rejecting controls; custom canned capture alone is insufficient.
Native3 exact-head CI, source/binary provenance and independent full review are
required. No new target build starts while the previous reviewer owns its target.

Do not pick an arbitrary token from a distinct Claude/Copilot map to make a test
pass. Preserve those compatibility gates until a separately designed explicit
account-selection policy is authorized and evidenced. URL/workspace families,
actual platform credential ownership, numeric conversion and host Keychain have
separate acceptance criteria. Neither next increment closes full #768.

## Implemented successor decision and exact limits

The independently reviewed publication `b8621bc26e8d9c5985e8517968d679c127fa152a`
was normally merged into this isolated successor. Its880 production, original
103/36 and121/83 binary archives, old failed observations and review artifacts
remain unchanged. The accepted target was SHA/gzip-roundtrip verified with zero
`/proc` users before exclusive rename; no duplicate Cargo target exists.

Usage now carries one `Invocation` classification through dispatch. Help,
undefined/bad flags and unexpected positionals use the existing literal-byte
normalizer and Go quoting before calling credential admission. Valid reports
alone evaluate `needs_go_fallback`; every original argument byte still reaches
Go on fallback. The normalization function moved mechanically to its own file
with the same public API and unchanged grammar, keeping touched production
components below400 lines. Existing fixture tests now require native diagnostics
and separately exercise valid report forms for ambiguous and numeric gates;
they no longer infer credential admission from an invalid command.

Regular bounded validUTF8 Kimi device IDs preserve their exact Unicode bytes
following Go-compatible TrimSpace, including CJK/emoji/combining/RTL/BOM/zero-width
and Unicode-whitespace controls. InvalidUTF8, internal ASCII controls/DEL and
unsafe/unreadable source shapes remain Go-owned. Token ownership, lexical joins,
API/CLI/web chain and URL trust are unchanged. The original89-input successor
corpus promotes exactly `kimi-device-unicode-value`: original ID/input bytes and
historical gate string remain, with a separate promotion marker. Its equality
is strengthened to full report/request/raw-header/read-only comparison. Source
totals are now82 full/7 gated on Unix (81/8 Windows conservatively),112 local CLI
observations; historical81/8/111 proof remains byte-identical. The original97
mapping and66 retained inputs are preserved.

Real owned TLS uncovered an inherited shared status-delivery bug: Ureq's default
HTTP-status exception bypassed the existing provider-specific parser. Both
production default/deadline agents now share one internal timeout/redirect/status
builder with status-as-error disabled. All raw response statuses and bodies reach
the existing provider owners; TLS, redirect refusal, body bounds and deadlines
are retained. The private proof uses this exact production builder, adds only an
owned trust root and restricted DNS resolver, and exercises unchanged production
request construction/execution. Actual TLS also exposed Cursor's empty200 body
diagnostic; only that precise empty-body text now follows frozen Go. Original
actual401, empty200, clock and harness failures remain retained, without removing
existing comparisons or changing body limits.

The new complete gate retains the original39 device inputs:32 full native
reports and seven retained raw/control gates, including selected bytes, exact
logical request/raw-header bytes and read-only sources.32 owned TLS device cases
plus84 remote cases across twelve strategy constructors cover401/403/429/500,
success/malformed/empty responses. Four actual TLS200/401 exact/over1MiB bodies
prove the unchanged native bound; Antigravity's shared transport is included,
without a claim about its independent process-observation/report contract.
Three genuine native failures reject missing-case, raw-device-header and remote
status-diagnostic mutations. Early argv preserves all64 original unsupported
admission vectors and expands to160 Unix/128 Windows-launchable diagnostics,
fifteen valid gated report forms and two real late-admission/double-normalization
mutants. Every original four gate, owner and argv regression remains required.

All full actual reports/timestamps and wire bytes remain available. Runtime
fetched_at is invocation-bounded; OpenCode resets compare exact3600/86400-second
offsets from each actual fetched_at. Other report values compare exactly. The
inherited OpenCode generated/fixed X-Server-Instance and transport-default headers,
header case/order remain explicit differences, rather than an exact HTTP-stack
parity claim. No frequency/performance claim follows from these tests. Fast-peer
proof is not a new cancellation/deadline or operator proxy claim. All credentials
are synthetic; pinned Go1.26.7 production/frozen fixtures remain unchanged. Fresh
native3 exact-head CI and different-author full review are required; full #768,
other URL/workspace/Keychain/account/numeric/platform families remain open.
