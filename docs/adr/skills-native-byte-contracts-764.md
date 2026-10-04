# Skills byte, outer MCP, and control-document owners (#764)

Status: source correction prepared for independent review; no compiler, product,
SDK, SQLite, target, endpoint, or native-platform execution in this increment.
The complete original six-P2 review of publication
`128e1bfc81733e488e25f5b4935300a33009e265` remains immutable. Its source was
`625e746a9ab9e31f631458244c7f82d6e1754457`. The pre-edit preservation commit
`33aac435` contains the full review, receipts, raw requests/documents, original
319-member archive, eight SDK payloads, complete frozen Go files and source
metadata ledger. Evidence is under
`migration/evidence/skills-native-764/byte-corrections`.

## Decisions and reasons

1. Keep native paths as `PathBuf` until an output boundary. `GoText` owns bytes
   and a separate decoded text view; filesystem access never uses that view.
   Human doctor/target/status writers emit the native path bytes directly.
   CLI JSON emits one escaped `\ufffd` for each invalid byte; a literal U+FFFD
   stays literal. Reports, issues, target evidence/hints, lifecycle destinations
   and install markers use this boundary. Sentinel characters or blanket lossy
   conversion would merge legitimately different inputs. Metadata reads use
   the original native library/destination paths, not their presentation.
   Marker/event JSON readers retain their existing Go string-repair/null/order
   semantics; a decoded historical JSON path is not asserted to recover raw
   bytes that its previous JSON writer discarded.
2. Escape U+2028/U+2029 at the shared CLI Go JSON encoder, alongside its existing
   HTML escapes. Input strings and human output are unchanged. Direct typed
   JSON serialization retains the byte/literal distinction; embedded payload
   `Value` construction performs Go per-byte text repair first. Existing MCP
   comparisons parse every successful payload field semantically and retain
   raw transport/content separately. They do **not** assert nested JSON lexical
   equality. No comparator/output projection was added.
3. Normalize CRLF in `SKILL.md` as bytes, parse only its closed YAML header as
   text, and retain the Markdown body bytes through inspect/render/materialize.
   Byte lengths continue to enforce the existing limits. Valid UTF8 variant
   bodies follow the unchanged variant parser; invalid bytes in a body that
   requests variant/term processing remain a strict textual error. Plain
   Markdown and resource bytes are not silently rewritten. Overlay/header,
   capability, resource, input-budget and generated-artifact checks remain.
4. A known Skills tools/call must pass its typed **outer** owner before its
   handler can run. An ordered field reader retains the first name/metadata
   type error, duplicate ordering and Go null behavior. `_meta` is
   `map[string]any`, so nested overflowing float64 numbers are outer errors,
   although ignored numbers inside Arguments remain an unparsed RawMessage.
   The legitimate `_meta:null` and ordinary-map cases are explicit controls.
   This prevents malformed metadata from authorizing a write.
5. Admit Go string bytes at the actual known-Skills transport entry, for both
   line and Content-Length framing. Repair only JSON string tokens per byte;
   repair envelope/params string keys where their typed owner decodes them.
   Preserve argument surrogate escapes and duplicate/numeric structure until
   the existing handler decoder. Invalid non-string bytes and malformed JSON
   still fail; the 1MiB/64KiB/100-line wire bounds are unchanged. Eleven catalog
   names scope this admission; other RPC owners keep their previous parser.
   `RawValue` necessarily stores UTF8: invalid string bytes are repaired before
   storage, with the original request bytes retained in the differential input
   ledger. This is not a claim that the internal audit retains pre-decoder raw
   invalid bytes, nor a general Go JSON-RPC ID codec cutover.
6. Final Unix `SKILL.md`/`symskills.toml` symlinks are refused using nofollow.
   For an ordinary target verified inside the held directory capability,
   retain the original failed Go-shaped nofollow PathError. Verification is
   metadata-only: pinned cap-primitives uses Linux O_PATH/fstat or Unix
   component resolution/statat, not a body open/read. Outside, missing,
   looping, special or otherwise unverifiable targets keep the stronger
   existing native refusal. A later metadata observation cannot make the
   failed control read successful; alias changes cannot publish its body.
   The root decision was: "preserve stronger outside/unverified refusal and
   exact originalGo nofollow PathError ONLY when ordinary in-root target is
   verified through retained root capabilities." This avoids both an
   ambient ancestry check and opening a FIFO/device to classify a diagnostic.
   The renamed-root test deliberately replaces the old ambient spelling.
   Windows Root.Open/reparse behavior remains separately platform gated.

Discovery has a separate non-loading identity owner. Frozen Go hashes the
resolved path, a NUL, and raw `os.ReadFile(SKILL.md)` bytes even for an invalid
bundle. Native now reads a permitted confined target through a retained,
bounded regular-file capability for this hash, while its loader still rejects
the final Unix control link. It never reads outside bytes just to reproduce a
hash. Outside/unverified refusal and bounded reads are explicit approved
corrective contracts, rather than exact Go ambient-read behavior. Ordinary
confined resource directory links keep their existing owner; this change does
not expand #490 or normalize resource identities.

The Go1.26.7 Windows `UTF16ToString`/`decodeWTF16` sources preserve unpaired UTF16
as WTF8. Native encoded path bytes are retained until presentation. Prepared
Windows high/low/separated-surrogate environment cases and per-byte JSON rules
need actual native execution; SDK source inspection proves no Windows API
execution, permissions, filesystem admission or universal raw-path parity.
Typed TOML diagnostics, conditional discovery stack forms, executable lookup
and remaining whole-#764 cutover requirements are not waived here.

## Prepared verification and required execution

The original five generators, fixture helper, comparator, sink/denial suites
and all three real-child negative control plans remain unchanged. Source-only
projections preserve **638 Unix / 614 Windows** original pairs and add
**188 / 170**, giving **826 / 784** prepared pairs. They include all ten original
outer request inputs, all eight original byte transport inputs, four exact
original LF documents, CRLF/body/render/install cases across all six targets,
raw/valid path presentation, and complete confined-link discovery rows.
Two additional actual-child input controls target metadata admission and raw
body handler decoding; they are prepared, not executed.

Thirteen additive Rust test declarations cover byte/JSON boundaries, body
load/render/materialization, ordered outer admission, both transports, unchanged
other RPC admission, nofollow identity, outside/dangling/FIFO/socket and
renamed-root capability ownership. Original assertions remain; two existing
event fixtures change only their destination constructor from String to
PathBuf. No test success is inferred from declarations or formatting.

Executed checks are limited to lossless source/archive/metadata roundtrips,
Git Go/dependency identity, Python AST/case/raw-input projections, standalone
Rust formatting/parser checks, workflow static lint and diff checks. The first
source verifier tried to read a sparse, absent Browse manifest from disk; its
complete failure log is preserved and the verifier now reads that immutable
Git blob without materializing another checkout. This was a source-check
script error, not a product observation. The supplementary owner retention
initially expected an absent extracted `.cargo-checksum.json`; its complete
failure is retained too. It now verifies actual immutable crate tar SHA against
Cargo.lock and checks each inspected cache file against that package member,
retaining the complete original package archives. Neither failure is hidden or
attributed to candidate product execution.

Before publication as an executed port: normal current-main integration, a
clean immutable source checkpoint, different-author complete review, locked
compiled strict checks and affected/proper-fixture workspace tests, every old
and additive differential pair/control, and real Linux/macOS/Windows gates
are required. Prior render/preflight/raw-argv/drift evidence and explicit
#476, #490 and #457 corrective contracts remain unchanged. Source preparation does
not complete #764 or authorize a merge by itself.
