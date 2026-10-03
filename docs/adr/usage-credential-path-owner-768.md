# Lexical credential paths and owner selection (#768)

## Finding and decision

The independent full review of immutable candidate7e0b6335/sourcef75993eb found
one P2 group after all four existing process gates passed. Owned HOME values
`<root>/link/../home`, with link pointing to `owner/nested`, selected different
credential owners: frozen Go's `filepath.Join` removed `link/..` lexically, while
Rust's retained parent components selected `owner/home` through the filesystem.
Copilot, Kimi current home and explicit KIMI_CODE_HOME were affected. Seven actual
ordinary-token constructor pairs gave three owner mismatches/four controls;
five literal-token pairs gave three mismatches/two controls. Every file remained
unchanged. Actual parent routing proved the ordinary-token flaw was inherited;
literal reference-shaped file tokens were newly admitted by this increment.
These observations are preserved exactly, including complete raw reports,
headers, CLI exits, probe sources and full review receipts, under
`migration/evidence/usage-copilot-kimi-768/original-root-review-7e0b/`.

Apply a small Usage-local pure lexical Clean/Join utility at both Copilot and
Kimi path construction, existence probes, file reads and eligibility decisions.
The reader and constructor must select the same owner. The existing capability
reader, byte/depth limits and nofollow/nonblocking file behavior remain in use.
Do not canonicalize: resolving the symlink would retain the wrong physical owner.
Do not depend on the CLI or a complete managed-file subsystem for pure path work.
No new dependency or credential store is introduced.

The utility follows Go1.26.7 filepath algorithms: Unix separators and raw bytes;
Windows slash conversion, drive-relative paths, UNC/device volumes and postClean
rules that prevent lexical rewriting from creating a drive/device path. Windows
paths preserve native UTF16 units without lossy conversion. A raw KIMI_CODE_HOME
is preserved as Go returns it, then joined/cleaned before each probe and read.
This is a bounded implementation with actual platform tests, not a claim about
every possible filesystem path or Windows ACL behavior. Go BSD license/source
attribution is retained in `migration/licenses/go-filepath-bsd.txt`.

## Retention, integration and evidence requirements

Before target reuse, all seven actually executed candidate test/CLI binaries
were byte/SHA archived in
`/workspace/oracles/symaira-usage768-local-files-7e0b-binaries/`, including CLI
4d0417cf0c33f6c664e5988402493c835a8fd1520b78bb76e4aa5e00d3a2b60c.
The original owner Go/native probes, exact linked production rlib and archived
parent CLI are also retained there. The archive receipt is tracked with the
original review. Original Go source, original97 baseline, historical failures,
original published Files binaries and immutable7e0b history remain unchanged.
After review release, main e3dbda6c was normally merged; its approved policy
publication fix is unrelated to this credential correction.

The same actual source-bound gate now additionally runs16 owned constructor
cases: ordinary and literal tokens across Copilot plain/dotdot/symlink paths,
Kimi plain/dotdot/symlink paths and explicit-home dotdot/symlink paths. Both owners
have different token bytes and, for Kimi, different device IDs. Exact full reports,
method/URL/body/header/raw-header bytes, routing and both owner files plus symlink
state are compared read-only. Sixteen actual CLI invalid-flag pairs verify route
admission without provider HTTP. A wrong-owner request mutation must fail the
real native request comparison with exit101. Owned symlink creation is required;
a platform inability fails and retains evidence rather than silently skipping.

Actual Go native-platform Clean/Join outputs are also compared for22 Unix or21
Windows path pairs: rooted/relative/leading-parent, drive-relative/drive-rooted,
UNC/device/namespace, colon and rooted-child edge cases, Unicode and a Unix raw
non-UTF8 path. Linux observations cannot prove Windows volume or macOS behavior.
Native exact-candidate three-OS CI and independent full review remain required.
The unchanged89-input file corpus, exact original97 accounting/replay, all three
complete parent gates,355 ordinary tests and strict checks must pass on one clean
source. Existing unsafe access, map ambiguity, control/device headers, numeric
architecture, unsupported base/workspace, Windows HOME mismatch and automatic
host-Keychain gates remain. Full #768 remains open.
