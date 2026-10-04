# Pinned-SDK Retry-After process oracle (#768)

Run `bash scripts/usage-retry-after-oracle/run.sh OUTPUT.json` with Go1.26.7,
Cargo and Python3. Linux execution uses the repository subreaper. Use the
explicitly owned Cargo target; the runner checks that the executed test binary
belongs to it and copies the actual Go/native executables into `.evidence/`.

The1600 scalar inputs call frozen production parseRetryAfter and RateLimitedError
in a fresh Go process. Native public parser output compares exact binary64 bits
and actual SDK int formatting, including nonfinite and overflowing values.
Metadata binds GOOS, GOARCH, int width, SDK source hashes and compiler settings.
No target's conversion result is presented as proof for another target.

The1164 TLS inputs exercise twelve real production constructors, full reports,
read-only private homes and original public hostname verification through an
owned private CA/resolver. Inputs cover negative, fractional, hexadecimal,
underscore, special, overflow and malformed syntax; all21 legal HTTP outer
whitespace code points; and repeated/invalid-byte first headers. All25 Go
whitespace code points also appear in the scalar corpus. HTTP fixtures and Go
production remain frozen. Complete request/response bytes are retained.

Four actual native processes corrupt the Go scalar or constructor evidence and
must fail the intended assertion. Mutated input hashes, first failure logs and
control wire bytes are retained. Missing input or incidental failures cannot
satisfy a control. The scalar and TLS counts are exact, including in controls.

This gate supplements the unchanged six Usage gates. It does not prove
production cancellation, deadline or proxy behavior from fast private peers.
Inherited default-header/OpenCode instance differences stay explicit. Full
independent review and native three-OS CI are required; full #768 remains open.

The uncompiled long-decimal successor adds `run.sh OUTPUT.json --decimal`.
That mode appends 81 public scalars and 192 TLS inputs to the complete unchanged
original corpus:1681/1356 exact records. It includes the confirmed 10010-byte
Go 0.1 input, the withdrawn giant-negative projection's actual Go 0 control,
five-/six-digit exponent transitions,19/800-digit state boundaries, signed
zero, malformed tails, exact normal/subnormal/overflow halfway values and
genuine dotted sticky-rounding transitions. The public-only 100010-byte input
is not sent through an HTTP-header limit.

All original four controls remain, plus intended rejection of a missing long
delay, an incorrect normalized fallback and lost sticky rounding. Only corpus
counts change in owned disposable Go callers; immutable original fixtures and
Go production remain unchanged and source/hash restoration assertions stay
strict. Metadata includes the exact generated caller hashes and all seven SDK
source bindings. Native3 workflows retain the original gate and add a separate
additive gate/artifact. No compiled/native/TLS/control result exists yet for this
successor. The retained81-case probe and unit fixture are actual SDK-only
Linux/amd64 reference evidence, not current native or provider acceptance.
