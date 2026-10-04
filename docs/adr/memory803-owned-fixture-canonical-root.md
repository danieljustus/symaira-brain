# Memory803 owned provider fixture and failed-setup diagnostics

Status: source correction; current native acceptance remains pending. The user
has delegated implementation decisions. Provider/admission production is unchanged.

Actual Windows job111470788544 and macOS job111470789198 first fail in the portable
provider tests. `verified_manifest` correctly resolves its selected owned prefix;
`AdmissionFixture` instead derives its exact fake loader paths from the lexical
temporary spelling. When these differ (macOS `/var` versus `/private/var`, or a
Windows resolved temporary spelling), its fake library callback falls through to
the extension-path assertion before the intended admission/SQL callbacks. The
complete original logs and JSON are preserved with native metadata. An actual
private Linux symlink alias reproduces the same assertion; this is fixture source
control-flow evidence, not a native macOS/Windows loader claim.

Resolve the fixture's existing owned root once, strictly, before deriving its
fake library, extension and manifest. All exact loader path/flag checks, actual-
extension address checks, SQL identity and admission-before-consumer criteria stay
unchanged. Do not canonicalize arbitrary production inputs or allow any broadly
matching mock path. Fresh original17 tests plus owned baseline, scoped host-path
normalization, unrelated-path rejection and missing-root controls preserve the
boundary. A real owned alias and a reverted-constructor corruption control remain
part of the source proof.

The later macOS always-run diagnostic also fails with127 because the earlier unit
failure prevented setup-go. Only its version record tests executable presence: a
missing command records that pinned Go setup did not complete; an existing Go
command still executes normally and every version failure remains fatal. Setup-go,
all actual build/parity/provider37/9, write/read/controls and native-three-OS gates
remain mandatory and unchanged. This does not turn the earlier failure into an
acceptance receipt or allow a missing pinned SDK during an actual Go gate.

Before edits, full parent Git bodies, native materialized source, complete current
and historical source/raw/failed-runtime proofs and both actual CI logs were bound.
The isolated original a111 and Brain/f312/b6a packets remain untouched. Full
independent source review, coherent-parent propagation and exact-head native CI
are required; no SDK/compiler/SQL/product/Target/cache/port run is claimed here.
