# Skills complete JSON admission and finite Go-number conversion

The full different-author review of `36c13305b56a71e9f6ffcef53f0a3bf5cba1a029`
requested three further source corrections (F7, F8, F9). This successor is
**source preparation only**: no Rust/Cargo/compiler, SDK executable, CLI,
provider, SQLite, target, port or native comparison has been run. The previous
six corrections and every historical failure remain unchanged in their
immutable checkpoints and lossless archives.

## Complete syntax before typed admission (F7)

Use an iterative, string-aware syntax owner for the complete incoming JSON
before decoding any field. Go 1.26.7 `encoding/json.Unmarshal` calls
`checkValid` first; its scanner allows 10,000 nested objects/arrays and rejects
the next opener. Ignored envelope fields, ignored arguments and `_meta` count
as containers. Brackets inside quoted strings do not. Invalid UTF-8 string
bytes and unpaired surrogate escapes retain the existing Go string-repair
owner; invalid bytes outside strings are syntax errors.

The scanner validates separators, keys, string escapes, literals and JSON
number grammar as well as depth. Errors enter the existing parse-error loop,
which responds with null ID and code `-32700` before typed field errors or
handler execution. Syntax-valid recognized Skills requests continue through
ordered `RawValue` fields, so ignored subtrees at depths 100 through 10,000
never enter a recursive serde `Value` merely to establish syntax. This does
not port the inherited generic/non-Skills `Value` decoder or the arbitrary
JSON-RPC ID codec. Those owners remain distinct pending scope.

The original comparator remains byte-identical: its `[1,2,3]` response-ID
criterion applies to all existing inputs. A new comparator only for explicitly
listed additive syntax-error inputs requires exactly `[1,2,null]`, parse code,
complete error message, exit, stderr and filesystem state. It never rewrites
actual responses or accepts a generic ID substitution.

## Linear ordered metadata traversal (F8)

Replace recursive repair/reparse of each remaining subtree with a single
iterative syntax walk and original number spans. Each string and structural
byte is consumed once per walk. Duplicate keys are retained in encounter
order, and the first Go float conversion error remains the first outer error
unless an earlier typed field error already exists. Later duplicates cannot
clear an error. No recursive tree is constructed for metadata.

The 9,000-array original input remains exactly 18,109 bytes. The former
81,018,000 repeated-array repair-byte projection is retained as source-derived
historical evidence, not a timing or crash measurement. The new walk uses
bounded container state (maximum 10,000 entries), not depth-squared subtree
copies. The handful of envelope/params passes are constant in count. Numeric
conversion consumes each original token with fixed 800-digit fallback state;
no lower depth, number-length or metadata waiver is introduced.

## Narrow Core finite-JSON conversion (F9)

The standard Rust float parser does not establish the pinned Go conversion
contract. Copy the exact six reviewed Retry conversion implementation files
(including both cached-power tables) into private Core modules. A single
public `go_json_float::parse_finite` entrypoint requires one complete JSON
number and rejects overflow. It preserves signed values, negative zero,
underflow, capped exponent accumulation, Eisel-Lemire fallback selection,
800-digit sticky rounding and the original fallback reread. Hexadecimal,
underscores, whitespace, `NaN` and infinity are excluded by the JSON wrapper.
Retry's nonnegative/header admission and architecture-specific integer
formatting are not exported or moved.

This is a **temporary, verified implementation copy**, not a claim that Usage
already shares Core or that generic JSON conversion has compiled acceptance.
All current Usage source/corpora remain byte-identical. A future Usage switch
to Core requires its own complete review and actual provider/SDK/CLI gates;
normal-merging unrelated Usage production into this Skills correction would
unnecessarily widen owners. The only adaptations inside the copied algorithm
are documentation backticks and three function-local lint allowances retaining
literal SDK parity/mask/working-copy expressions. The full historical Retry
CI lint failure and the exact mechanical adaptation map are preserved. There
is no dependency cycle and no new external package: MCP adds its first direct
edge to the existing Core crate.

The token `0.` + 10,000 zeroes + `1e100000` has historical **actual Go SDK-only**
result 0.1, with input SHA256
`cf8db6085218dc12e84d439e4a136d8f36f8cdaa49430e6b2ffe26008841b9e8`.
Its original source, results and executable archive remain bound. This is not
an observed failure or pass of the current Rust candidate. Prepared Core tests
compare all 81 unchanged actual SDK records, including signed/negative-zero,
finite/underflow/overflow boundaries and non-JSON grammar exclusion.

## Preservation and pending acceptance

Before edits, preserve the complete full review and its 66 bound artifacts,
the 271-file affected graph, exact Retry sources/tables/tests/policy/fixture,
Git identities and available native metadata. The existing 1,919-member
original and 27-member SDK/consumer archives are verified in full and reused
without duplicating their payloads.

The prepared process plan appends 100 inputs: all 26 exact independent wires,
additional ignored/envelope/metadata depth boundaries, signed numeric cases,
quoted containers, duplicate first-error order and malformed grammar before
typed errors. All 826 Unix / 784 Windows old input records remain an exact
prefix; current prepared totals are 926 / 884. The original three and later
two actual-child controls remain unchanged; two new real-input controls mutate
accepted depth and finite metadata into rejected depth and overflow. All seven
are **prepared, not executed**. Original 638 / 614 prefixes, 141 raw MCP inputs,
18 original request wires, four full LF documents and confinement/denial/
discovery limits remain governed by the previous acceptance owners.

Pure projections, AST, lossless source/archive verification, direct rustfmt
and actionlint do not constitute compiled/native acceptance. Fresh immutable
Go/native whole reports, both transport modes, writable-owner denial state,
all existing families/controls, affected strict all-target graph tests and
native Linux/macOS/Windows CI are still required. No issue closure, whole #764
cutover, publication or merge approval is claimed.

## Main CI parent acquisition and fixture Git identity

After the focused source checkpoint, normally merge the exact common CI leaf
`e00365333a041118dd0598157137e0ba3586013d` (which includes
`cc3f4e62dcb133b616c6b58c135463ad81058801`). Preserve all current render,
main/Activity and native checks; the workflow is exactly the previous Skills
workflow plus the four-line immutable-parent fetch. Its command retains
`--no-tags` and complete history, with no `--depth`. The original Windows
missing-parent failure and the rejected depth-one attempt remain inherited
as full lossless Git-bound evidence; no remote/native check is inferred here.

The first local source commit `b617558bea545829061c559aa55cf1bbc7ae3a9e`
exposed an author provenance error: repository `text=auto eol=lf` normalized
CRLF in thirteen newly added framed fixture Git blobs, while their physical
original reviewer bytes still matched. Both full original forms and Git/native
identities are retained before correction. Scoped `-text` attributes restore
the exact protocol bytes in Git without modifying any input assertion. Final
source binding explicitly requires physical fixture bytes equal to immutable
Git blobs as well as the original reviewer hashes. This is source-provenance
repair, not an actual product mismatch or executed acceptance.

Two pure checker mistakes are retained with scripts and full tracebacks: a
Python3.12 projection changed global `os.name` and attempted a WindowsPath on
Linux; the corrected projection binds only generator module platform views.
A naive format-template regex selected a later template; the bounded function
projection now checks the actual Rust request-body format. An initial patch
attempt with a redundant out-of-order context hunk was rejected before applying
changes; its tool error is retained as author tooling, not product evidence.
