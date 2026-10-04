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
