# Preserve the Go Retry-After contract (#768)

Status: source prepared; fresh process validation, independent review and native
CI remain required.

The independent review of `5e77418` / source `c0ae71c` found one shared
Retry-After defect. Once real HTTP error statuses reached the provider parsers,
negative delays appeared in native error messages and Go hexadecimal and
underscore float syntax disappeared. Actual private TLS constructors reproduced
the difference. The original review, full request/response bytes, first failed
assertions and actual executable identities are retained in
`migration/evidence/usage-retry-after-768/original-independent-review-5e774`.
The lossless binary archive records all569 executable paths/499 unique hashes;
the246 retained proof bindings include the linked parser caller and its library.

The long-term contract is the frozen production `parseRetryAfter` and
`RateLimitedError`, executed with the pinned Go1.26.7 SDK. An HTTP standards-only
replacement would silently change existing diagnostic behavior. A finite-only
filter would also change behavior: Go accepts unsigned NaN and explicit positive
infinity, rejects signed NaN, negative infinity and negative finite values, and
rejects numeric overflow reported by ParseFloat. Negative zero remains valid.

Use a small Usage-local grammar adapter rather than a new numeric dependency.
Decimal conversion preserves the pinned SDK's exact/Eisel-Lemire fast decisions
and its bounded multiprecision fallback after Go grammar/underscore admission.
Hexadecimal scanning,
sticky bits, subnormal handling and ties-to-even rounding are adapted from the
pinned Go SDK. The BSD notice and exact SDK source hashes accompany the port.
The public parser returns the float value; provider error formatting performs
the separate Go integer conversion.

Out-of-range float-to-int conversion depends on the SDK target. The pinned
default AMD64 compiler uses CVTTSD2SQ, whose invalid conversion returns the
signed minimum; 386 uses the corresponding32-bit operation. ARM64 uses FCVTZSD.
The implementation reflects those instructions and the gate compares actual Go
integer output with the current native target, including NaN, positive infinity
and finite overflow. Linux observations do not establish macOS or Windows
runtime behavior. Native CI must execute the oracle on each release target;
unsupported targets require their own evidence before acceptance.

Preserve Go Header.Get ownership at the transport boundary. Read the first
duplicate value, even if a later value is parseable. Decode valid UTF-8 response
header bytes rather than using the ASCII-only HeaderValue projection, so Go's
Unicode outer-whitespace grammar remains reachable. An invalid UTF-8 first
value contributes no parsed delay and does not promote a later duplicate.
Actual TLS cases cover these ownership and byte boundaries through the same
production builder/executor and all twelve provider strategies.

The new oracle prepares1600 public parser inputs, including all25 Go whitespace
code points and deterministic rounding probes, and1164 real TLS constructor
cases. It compares binary64 bits, SDK integer output, full provider reports,
request/auth bytes and read-only filesystem metadata. Four actual negative
processes must reject missing scalar evidence, altered hex rounding, an
incorrect negative-delay diagnostic and a finite-only waiver. All original six
Usage gates remain required unchanged. Raw transport differences and the
existing private-peer cancellation/deadline/proxy limitations remain explicit.

The first archive attempt encountered a cross-device hardlink error and is
preserved alongside the verified copy fallback. Original Go production and
fixtures remain unchanged. Full #768, protected CI and exact-head native
Linux/macOS/Windows acceptance remain open; source preparation is not approval.

## Long-decimal source correction after independent 780 review

The original 780 source, complete independent source review and projections are
preserved in `independent-780/retention.json`. Root's actual Go1.26.7 scalar
probe confirmed `0.` plus 10000 zeros plus `1e100000` yields 0.1 without an error;
the prior Rust mathematical parser would overflow and drop the retry suffix.
Root also falsified an initial negative-mantissa projection: `1` plus 10000
zeros plus `e-100000` yields 0, not 1. Both original and withdrawn interpretations,
raw inputs and receipts are retained. No native780 executable was built.

A general exponent rewrite is insufficient. Go's first scan keeps 19 mantissa
digits while continuing to count significant input digits. Its fast conversion
may return that state directly, or compare Eisel-Lemire lower/upper bounds.
Fallback reparses the original input with 800 stored decimal digits, a sticky
nonzero-discard flag and different decimal-point accounting. For example,
appending redundant zeros to a coefficient/exponent halfway representation can
change the fallback result, while the equivalent dotted representation keeps
the point and exercises actual sticky-bit rounding. The complete pinned
fast/fallback control flow, native-word-width binary shift chunks, point/zero
handling and ties-to-even rounding now live in focused safe-Rust modules.
All 696 power mantissas and 61 left-shift cutoffs are copied unchanged from the
pinned SDK with its BSD attribution. No numeric dependency or version changes.

This preserves the existing SDK contract rather than inventing a standards-only
normalization or a maximum-header/input waiver. The input is scanned linearly;
decimal working memory is the SDK's fixed 800-byte state, and conversion shifts
are bounded after the same explicit decimal-point checks. There is no full-size
normalized string allocation, and zero/underflow/overflow behavior follows the
same chosen SDK branch. Public long inputs remain admitted independently of the
transport's own header-size bound.

Actual supplemental observations are limited to a small owned SDK-only
Linux/amd64 probe: first 62 records, then 71 and 81 records retaining the originals.
The latter includes real halfway/sticky transitions. SDK source, probe source,
ELF, commands, complete inputs/outputs and archive roundtrips are retained in
`decimal-sdk-linux-amd64*/retention.json`. This is public SDK ParseFloat and
direct int output, not frozen Usage constructors, HTTP or current-native parity.
The new Rust reference test is prepared but uncompiled/unexecuted.

The original 1600 scalar/1164 TLS definitions and four intended mutants are
unchanged. A separate additive mode reruns all of them plus 81 scalar/192 TLS
cases, totalling1681/1356, and adds three real-process control definitions for
dropping the confirmed delay, wrong fallback normalization and lost sticky
rounding. In disposable Go caller copies only the two exact corpus-count
assertions/diagnostics change; frozen production and original caller fixtures
remain byte-identical. Native3 CI gets a separate additive evidence artifact.
All seven controls, original 355/six-oracle baselines, fresh strict/package/full
process gates and different-author review remain required. No Cargo target,
current Rust product or TLS process was allocated/executed for this source-only
checkpoint; no cross-platform/native acceptance, performance or full #768 closure
is claimed. Main 5e was normally integrated without Go/Rust production changes.
