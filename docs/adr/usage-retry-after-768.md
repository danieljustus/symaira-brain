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
Decimal conversion uses Rust's correctly rounded binary64 parser after the Go
grammar and underscore positions have been checked. Hexadecimal scanning,
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
