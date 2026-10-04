# Brain Memory startup: four final source corrections

Status: SOURCE PREPARED only, successor of immutable
`8afb9a9bf3c266478adea2f154bee9d5e0e1ebe1`. No compiler, Cargo, SDK
program, product, SQLite operation, network endpoint, target or port was run.
The full different-author review requested four corrections. Its report,
receipt, original projections, failures, all65 bound prior proof paths and
original seven-member Darwin context ZIP were SHA-verified and losslessly
retained with observed native metadata before successor edits. The immutable
8afb and older bc0/9804 sources and previous six corrections remain available.

## Decisions and reasons

1. Memory keeps its own `symmemory` namespace, fields, one-shot configuration
   and whole-default reset on admission failure. It now reads original bytes
   through the exact accepting TOML parser already used by Brain. The additive
   Core `resolved::parse_document` API shares syntax admission without applying
   Brain fields/defaults. BurntSushi1.6.0 strips exactly one leading FF FE,
   FE FF or UTF8 BOM; the remaining document is still UTF8, not transcoded UTF16.
   Memory must not silently discard a valid configured database/key just because
   its separate loader decoded UTF8 before this admission boundary. Nulls,
   invalid later UTF8, repeated markers and wrong field types still reject.
2. Rotation time remains a private owner. Go1.26.7 `getnum` parses the hour,
   then the minute only if hour conversion succeeds, saving conversion errors.
   Its range checks precede the saved conversion/sign error; minute range wins
   when both ranges fail. The native owner now follows that order. Thus `+25:xx`,
   `+25:x1` and `?25:xx` report hour range, while `+24:xx` and `+x5:61` remain
   conversion errors. Parsing versus marshaling, raw JSON quote bytes, offsets
   24/60 and all previous strict/fallback boundaries stay intact. This adds no
   global time API or relaxed syntax policy.
3. Typed rotation decoding uses direct-child links constructed in one reverse
   pass over the scanner's preorder nodes. It visits each root entry and its
   direct fields once. Two integer arrays cost linear space and preserve sibling
   order without recursion or per-object rescanning. Syntax is still completely
   checked first; duplicate/unknown fields, raw spans, saved ordinary type errors
   and immediately fatal Time errors retain their original order. The previous
   100000-object input required 10000100000 parent predicates; the replacement
   has one linear link pass and 100000 direct root visits. This is a source
   complexity argument and Python projection, not measured compiled performance.
4. Fixture ownership includes kernel admission. Ordinary ASCII and valid-Unicode
   blocking parents remain mandatory on every native platform. The exact raw
   component `626c6f636b65642dffe282` is actually created before invoking either
   CLI. Only a Darwin failure with errno92 for that exact component may produce
   an UNEXECUTED row. The report retains original case/input bytes, raw path,
   operation, actual error, full partial fixture state, zero CLI children and
   verified cleanup. `total`, `executed`, `equal` and `unavailable` are distinct;
   unavailable is never counted as equality or as a successful fault control.
   All other creation errors fail the gate. Linux retains every raw case and
   Windows keeps its separate UTF16 contract. The earlier actual Darwin errno92
   concerned another owner's different raw filename; it is contextual evidence,
   not proof that this new compound fixture is unavailable. Exact new Darwin
   admission is still unexecuted here.

The reviewed extra blank EOF line in `command_capture.rs` is removed with no
capture/lifecycle behavior change. Its original raw whitespace failure remains
in the retained review. A historical source hash is never overwritten: the
runtime preflight has an additive parent/current SHA overlay checked against
immutable8afb Git objects and actual current source bytes, preserving both old
checkpoint assertions and all source/SDK/retention checks.

## Prepared acceptance and limits

The new family has94 Unix /82 Windows plans, each with its complete81/69 old
input prefix byte-identical. Thirteen additive cases cover six global/project
Memory markers, five mixed timezone failures, a 100000-entry authenticated array
and a valid-Unicode blocking parent. The marker cases observe the configured DB
and key through complete state and readonly authenticated fallback envelopes.
Two additional real input controls change only the native database selector or
key and must demonstrate the intended complete-file-set or warning mismatch.
These controls, the new Rust unit tests and all process cases remain PREPARED,
with zero actual compiled/SDK/product executions.

Executed source checks comprise Python AST, Bash syntax, Rust source formatting,
portable admission predicates, linear/order projections and the unchanged five
schema-program comparator controls. Python predicates distinguish exact Darwin
errno92 from different platform/name/errno combinations; they are not kernel
observations. Source plans are generated without creating fixtures or using
SQLite. Full raw intermediate preparation attempts are retained.

The prepared runtime driver retains the original41/36 startup family,81/69
six-finding family, all eight previous state controls, original102/152/182/146
families and their controls, full workspace/all-target/all-feature tests,
strict Clippy, format/actionlint, inherited Setup/Doctor/Guard/Memory gates,
actual frozen Go constructors and doc tests. It additionally runs the94/82
family and two selector/key controls. Failed observations and actual binaries
must be retained before fixes. A clean source manifest alone proves no runtime
success. Explicit resource allocation and700MiB floors remain required.

A different-author full review and fresh native Linux/Windows/macOS validation
remain mandatory. Broad TOML wording, unsupported consumer/delegation families,
DB engine diagnostics, provider lifecycle boundaries and all previously open
syntax/type/lookup/owner gates are not waived by these four corrections.
Issues765/759 remain open; there is no whole-Brain or whole-Store cutover claim.
