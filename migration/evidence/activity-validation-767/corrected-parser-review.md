# Correction review: activity integer error precedence

Correction source: `89083ea31f3ab0fd849ac94712fb0ee6e24ada50`.
Previous independent full-layer review: `36ccc6ef60815806d38bfbec4ddbf9e96679d648`
against main `e8c7f7990ab61967db0a3cbadaf6e485b4a33d99`.

The correcting author and this reviewer are separate agents. The original
independent full-layer review is preserved, including its three actual failures.
This corrective review examined the complete byte-parser diff, actual Go
ParseInt/ParseUint ordering and original diagnostics, expanded oracle cases,
negative controls, unchanged profile precedence and all retained limits.

The parser scans digits and unsigned magnitude before checking separators,
then signed bounds. It retains ASCII-only base-zero syntax, signed-minimum
handling, prefix separator allowance and invalid-byte diagnostics. The root
reviewer separately selected 930 actual process inputs spanning decimal,
binary, explicit/legacy octal and hex, signs, separator errors, invalid bytes,
valid RuneError, and signed/unsigned boundaries. Complete stdout/stderr bytes
and exits matched the immutable Go binary in disposable HOME/XDG roots with
an absent fallback. The numeric source bytes match the inspected source SHA.
Literal observations and binary identities are retained in
`corrected-numeric-review-linux.json`. No additional defect was found.

Disposition: original numeric finding corrected; normal merge remains subject
to fresh native three-OS acceptance and protected GitHub checks. The author's
265-case full harness, original five report cases, three rejection controls
and 299 primary tests are retained separately and not relabeled as executions
of this reviewer. This is scoped validation, not acceptance of #758/#761.
