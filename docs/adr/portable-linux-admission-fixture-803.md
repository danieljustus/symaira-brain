# A complete portable Linux loader simulation for the owned checker (#803)

Status: source correction prepared; different-author source review and genuine
native three-platform/official CSDK acceptance remain pending. No production
loader, runtime flags, source-ID/address/version guard, workflow or SQL changes.

Full independent review of immutable
`a5c65c706ac4cb271fde26d9063b0f4afff1b420` requested exactly one P2 in
`AdmissionFixture.run`: it spoofed Linux while retaining host `os.RTLD_NOW`.
Windows has no such Unix-only attribute, so evaluating the production flag
expression failed before the fake loader. The original owned Python control
removed that constant and obtained ten AttributeErrors from the unchanged six
admission methods. This is simulated host-API absence, not a native Windows run.
Original full report SHA
`f75f0b9b73f426611a2dbbd36a68b34276e587ed924351fd05f5e31f29d5f2f0`
and receipt SHA
`ea69d2662b52180555a2379d470dc33ed986fe54c81649335160b14d858f5960`
remain byte-exact. All58 own proofs, the receipt and422 prior input bindings were
verified and retained before editing; physical native metadata is preserved.
Existing original distribution archives stay at their original paths, with
recorded bytes/modes/uid/gid/mtime, while immutable Git blobs retain prior trees.

The scoped fixture now supplies **both** Linux loader constant inputs with
`patch.object(..., create=True)`, restoring host state on exit. Distinct simulated
GLOBAL/NOW bits let the fake loader require exactly their union for the verified
library. Extension lookup must receive no explicit mode, retaining its existing
production default. These test bits model the Linux call boundary; no native
loader or host ABI acceptance is inferred. There is no production flag change,
platform waiver or environment-loader rewrite.

The original14 test-method ASTs and eight preparation tests remain unchanged.
Three additive test methods are prepared: run all original six admission methods
with both host constants absent and verify restoration; require the full flags
and default extension call while rejecting missing-bit/zero/explicit-extension
modes; reject the actual production Windows platform guard before the mock loader.
The test module imports the old TestCase through a module reference, so unittest
discovery does not accidentally count imported test classes a second time.

The unconditional Linux/macOS/Windows test glob, original37 historical SQL cases
and nine controls are unchanged. This isolated sparse successor retains the
complete parent index and stays below15MiB materialized. Only Python AST/full-Git
source/archive/hygiene checks are performed for this checkpoint: all new mock
checks are **prepared, not executed**. No C/SQLite/SDK/Go/Rust/product/compiler,
target, port or GitHub operation is claimed. Genuine CSDK build/preload/extension
binding, original REALDEFAULT/FTS5/realNULL refusals, wrong-image/library controls,
all historical/reopen/governed phases and native-three-OS CI remain required.
The original c310 admission finding remains source-closed by its independent
mock evidence, separately from actual native-loader acceptance.
