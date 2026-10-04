# Admit equivalent Guard config forms and Go anchor Unicode decoding

Implementation decision; independent review and native acceptance are required.

The approved raw-path parent kept several conservative doctor gates. Actual
Go1.26.7 process probes found fourteen healthy states that unnecessarily used
Brain's Go fallback or failed closed in standalone: nine typed inline-TOML,
empty-array and dotted forms, and five known/unknown anchor Unicode forms.
The baseline38 input/output/state observations are retained unchanged under
`migration/evidence/guard-doctor-boundaries-770/baseline-8e3`.

Use toml_edit's common TableLike interface for regular and inline tables.
Accept arrays of typed inline tables as the equivalent of array-of-table
headers, including empty arrays. This applies consistently to defaults,
rules/match, proxy, audit, remote, sequence and spawn/allowlist. Every known
field still receives its typed check before semantic validation. Scalars or
nested arrays in a struct array remain decoder boundaries, unknown-key
warnings remain Go-owned, and multiple invalid defaults remain gated.
Actual Go probes also accept multiline/commented/trailing-comma inline syntax
and the pinned parser's newer escapes; their original bytes and outputs are
retained rather than assuming a different TOML grammar from version labels.

Anchor strings use the existing Guard capability/request Go JSON replacement
primitive through the Go JSON utility interface. Validate original syntax
first, then replace each malformed UTF-8 byte or unpaired UTF-16 surrogate as
encoding/json does. Repaired keys and values are decoded in original field
order. Numeric tokens stay RawValue bytes: no float64 conversion, no overflow
introduced by ignored unknown fields, no lost duplicate or first-error order.
This admits healthy replacement-valued anchors and preserves actual typed
errors following or preceding replacement. It does not grant a capability,
change policy decisions, modify token verification or weaken the audit sink.
GoText still preserves Unix path bytes in human output and bytewise JSON output.

The three original selected TOML gates remain explicit. Fifty real identical
multiple-invalid-default inputs produced two distinct Go reports. Preserve
these observations and keep the gate; this change adds no nondeterminism
exception and does not repurpose any existing exception such as E-002.
Ordered TOML type errors, unknown-key warnings, malformed discovery and config/
anchor/filesystem read errors have separate source and actual-process inventory.
Warning-only unknown keys are healthy Go states, not decoder-error cases;
standalone currently fails closed on them. They remain visible unfinished work.

The additive78-case actual Go/native gate includes65 full comparisons and13
explicit conservative TOML gates. Actual mutants remove an inline allowlist,
reject a correctly repaired healthy anchor, and hide a later typed anchor
error; each must be rejected for the intended output change. Existing124/80/94,
raw63, original3/raw2 controls, ordered31, Unicode/raw/unsafe-audit and Root35
proofs remain mandatory. Native Linux evidence does not replace native macOS
or Windows. CLI and Brain share the Guard handler directly; this slice does
not complete #770/#769, remove the Go executor or claim full Brain acceptance.
