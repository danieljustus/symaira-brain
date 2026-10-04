# Usage argument diagnostics after literal file admission (#768)

## Decision and reason

The independent full review of `abf20713bacdab562644256e27616a9dd7acb81e`
(source `00bcc6f51a2fbc00581ca7f6b7af0e868d168971`) closed the credential-owner
finding and requested changes for one argument-diagnostic group. Actual frozen
Go, accepted parent CLI9aed and that candidate reproduced twelve mismatches in
22 owned Copilot/Kimi literal-token processes. Four leading dashes were stripped
too far; invalid Unix bytes were replaced before writing unknown-flag errors;
Rust positional quoting used uppercase hex. The accepted parent matched Go on
all22 because these literal file shapes had still used Go.

Use the CLI's existing byte-preserving one-time flag normalization and Core's
existing Go-compatible OS-string quoting. Usage defines no local flags, so its
small parser implements Go FlagSet's first-argument grammar: stop at a positional
argument or `--`, reject malformed dash/name syntax, split an unknown name at
its first `=`, and handle implicit `h`/`help` before any provider operation.
Raw flag errors write original bytes. Positional errors use Go quoting, including
lowercase hex for each invalid byte. Earlier arguments determine the outcome;
a later help flag cannot override an earlier positional or unknown flag.

This reuses established helpers without adding a universal argument parser or
changing credential admission, ownership, HTTP behavior, provider constructors,
Go fallback decisions, rendering or the credential service. In particular,
reference-shaped file tokens remain literal; host Keychain and unsupported
credential sources retain their existing gates. No new dependency is needed.

## Actual comparison and build ownership

Keep all22 original argument/file bytes and every original result unchanged.
The permanent process gate adds60 Unix cases: terminators, precedence, one to
four dashes, malformed names, value splitting, controls, printable Unicode and
multiple invalid UTF8 forms. The immutable abf parent and corrected candidate
execute with absent fallback, owned HOME/USERPROFILE/XDG and empty PATH. All82
Unix cases must match actual frozen Go stdout/stderr/exit bytes, return2 with
empty stdout, and leave every owned credential file/tree unchanged. No positive
provider HTTP request is made. Three real diagnostic mutations must be rejected.

Windows uses56 launchable cases (14 original) and two real mutations. The26 raw
Unix-byte vectors cannot be launched through Python Windows argument strings;
the report retains them as explicit skips. This is a corpus declaration, not
Windows runtime acceptance or a surrogate-conversion claim. Existing helpers'
non-Unix representation limits are not expanded here. Exact-head native Linux,
macOS and Windows CI remains required.

Parent and candidate builds share one explicitly owned target to bound disk use.
The first dirty gate proved that merely rebuilding after changing worktree paths
can retain the parent executable: Cargo returned immediately and the candidate
comparison failed. Preserve that complete failure rather than claiming the
successful direct prototype as reproducible acceptance. Before each variant,
archive every regular file listed by `cargo clean -p symbrain-cli --dry-run`,
decompress and check SHA/length, then clean only that package. Linux also checks
/proc for target users; the runner otherwise requires exclusive target ownership.
Capture the initial candidate and parent CLI copies and require the restored
candidate executable to match the initial candidate byte-for-byte. On clean
sources, manifest bytes are checked against their immutable Git heads. Actual
binaries, build logs, SDK source hashes and all comparison outputs are retained.
No other package target is retired and no second compile target is introduced.

## Retention and acceptance boundary

Before reuse, preserve36 actually executed original CLI/test/rlib files in a
lossless verified archive and all103 original raw review artifacts. Git retains
100 nonbinary artifacts plus the original report/receipt; three executed probes
remain byte-identical in the external archive. Verify the earlier73 retained
artifacts/11 archived entries and accepted112 executable paths/74 unique hashes.
Original failures, parent mismatches, failed build attempt and dirty prototypes
are historical evidence, separately labeled from later clean-source gates.

All four original source-bound Go/native process gates, original97 accounting,
16 owner constructors,16 owner CLI routes,22 Unix path pairs and wrong-owner
control remain mandatory and unchanged. Replay the original12 owner inputs and
32 additional public constructor probes. Ordinary355 tests/four explicit oracle
ignores, strict Clippy/fmt/actionlint and matching source/binary manifests remain
required. This focused fix needs a new full independent review; authorship and
Linux validation do not approve it. Full #768 and native platform acceptance
remain open.
