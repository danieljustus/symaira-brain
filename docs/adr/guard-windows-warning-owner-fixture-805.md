# Exact Windows Guard warning owner in the Brain adapter fixture

Both complete native Windows logs at jobs `111383721071` and `111383721116`
fail the healthy child of `generated_owner_and_delegation_are_shared_by_brain`
at its configuration-path substring assertion. The latter log includes both
whole-workspace and focused Doctor failures. The previous status and warning
prefix assertions passed; the actual discovery errno-2/errno-3 control passed
too. The old assertion did not print the captured warning path. Full original
logs, the failing fixture bytes and the 617-file source map are retained with
SHA-256 and lossless gzip round trips in
`migration/evidence/guard-doctor-config-paths-770/windows-warning-owner-a83-original/`.

The source-bound discrepancy is the fixture's
`root.path().join("config/symguard/config.toml")`. A slash inside that supplied
component remains part of its Windows spelling. The generated configuration
owner instead follows frozen Go `DefaultPath`: `filepath.Join(xdg, "symguard",
"config.toml")`. Pinned Go 1.26.7 Windows Join calls Clean; Clean returns
FromSlash, using backslashes in the generated spelling. Native Guard preserves
that generated-owner contract. Reading the same file successfully does not
make the two diagnostic byte strings equal.

Construct the expected fixture path from separate native components. Do not
normalize actual warnings or reuse the production lexical helper to derive an
expected output. Strengthen the assertion to the complete exact warning line,
including prefix, owner bytes and newline. Any extra warning, wrong owner or
separator change now fails with the full expected/actual byte vectors.

Two additional native Windows child roles preserve this distinction. A
slash-form XDG value with `discard/../config` must emit the native cleaned
owner. A slash-form explicit `SYMGUARD_CONFIG` override must retain the caller's
slash spelling exactly. Both remain healthy, use the same owned configuration,
and check its original bytes and modification time. The original healthy,
semantic, typed and discovery roles and raw-wide early refusal remain. There
is no global slash-equivalence comparison or change to production, frozen Go,
decoder admission, discovery error classification or warning order.

This successor is source-only. Formatting, Actionlint and source-bound controls
cannot establish Windows runtime acceptance. A different-author full review
and fresh exact-head native Windows whole-workspace and focused Doctor gates
must execute before regular merge. Unix expectations remain exact and require
their existing runtime gates too.
