# Guard diagnostics retain operating-system path bytes

Accepted implementation decision; independent review and native acceptance are still required.

On Unix, a filename is a byte sequence. A path containing `e2 82` is a real,
distinct path even though it is not UTF-8. The newly admitted Guard config,
anchor and directory-open diagnostics opened this path correctly but then used
`Path::display()`. Human output collapsed two invalid bytes into one replacement
character. JSON likewise returned one literal replacement instead of Go
encoding/json's two `\ufffd` escapes. This changed the diagnostic's path identity.
The original independent failed inputs, binaries and review remain under
`migration/evidence/guard-raw-paths-770/original-e876`.

Keep diagnostics as `symbrain_core::GoText` until their final destination.
Human output writes its bytes directly. JSON uses valid UTF-8 normally, encodes
each invalid byte as `\ufffd`, and preserves Go's HTML and U+2028/U+2029 escapes.
A genuine U+FFFD remains literal UTF-8. Core already provides Guard's OS-byte
and Go-quoting primitives; this adds no Guard dependency on Brain or Managed.
Core explicitly requests serde_json's existing `raw_value` feature, including
standalone builds. The API follows the independently authored Doctor primitive
in source `0b56d4edc7f9e6728fdbf955df89c06e2a1689d2`; its candidate remains unchanged.
After both changes pass independent review, Managed should re-export the Core
representation so Setup, Source and Guard share one encoding contract.

The policy kernel keeps its existing String response and audit-sink interfaces.
The CLI retains an additional byte-valued reason only when the actual audit
append fails for an originally allowed or confirmation-required decision.
The kernel still performs the fail-closed transition. Existing denied decisions
retain their original reason. Audit capability checks, no-follow opening,
private modes, single writes and conservative unsupported-TOML gates are
unchanged. The whole doctor report remains buffered before any output.

Permanent actual Go/native probes use private HOME/XDG roots and empty PATH.
They exercise the original malformed paths, valid Unicode, quote/backslash,
HTML and JavaScript separator paths, and already-denied requests. Actual output
mutants must fail the comparison. The shared Brain compatibility adapter is
compiled and invoked by a production-boundary regression test. This validates
the consumer adapter without claiming that a full Brain executable was built.

Unix byte paths cannot be manufactured through Windows' native UTF-16 path
API. Native Windows runs the valid Unicode cases and records that raw Unix
cases and their mutants are inapplicable. Existing Windows audit-open wording
remains an explicit fail-closed deviation, pending native proof. Linux evidence
never substitutes for macOS or Windows acceptance. Three conservative TOML
states, broken-output and wider malformed discovery boundaries remain open;
this correction does not complete #770 or assert full CLI cutover.
