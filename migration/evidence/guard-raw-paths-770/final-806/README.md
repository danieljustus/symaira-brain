# Clean source806ddcf author verification

Full source `806ddcf8bd36c57b5b26d5148cf7015a465d289e` includes the normal
integration of main `e3dbda6cbb95429237d15a9b176209b7d107792c`. Later evidence
commits change no runtime, harness, workflow or ADR bytes. Author checks are
not independent approval; native macOS and Windows remain pending.

168 ordinary Rust tests pass (105 Core +63 Guard, 13 non-doc summaries), with
zero failures/ignored tests. The extra Guard integration test compiles the
exact production Brain compatibility adapter and invokes its shared public
boundary; this does not claim a full Brain executable was built. Strict
all-target/all-feature Clippy, fmt and actionlint pass.

The exact fresh Go1.26.7/native process runner passes124 cases:121 full
stdout/stderr/exit/state comparisons and three conservative TOML gates. The
original80 and94 inputs execute afresh, with byte-identical args/state/payload
verified against their original source. Original80 now80 match; original94
has91 matches and the same three gates. The contract changes only strengthen
newly admitted states to equality; all original receipts remain untouched.

The permanent native raw-path supplement passes63/63 Linux comparisons and
rejects two actual lossy-output mutants. Three original output mutants also
reject with meaningful intended stdout, correct exit and empty stderr. All
four exact original E282 path inputs and four literal-U+FFFD controls now
match. The original31 ordered anchor/TOML probes pass27 equality comparisons
and four explicit decoder gates. Six real audit filesystem probes preserve
private modes, sentinel integrity and deny behavior: legacy Go follows three
unsafe paths that the unchanged native capability correctly rejects. These
three safety differences are recorded, never reported as equality.

Verification binds96 source/harness/workflow/consumer/documentation inputs
and1217 original Go sources/module inputs, both current tests/CLI and the
original53 ELF archive. Raw process outputs, normalization, source/binary SHA,
SDK identities, prototype failures and original raw-path failure inputs are
retained. The exact fresh primary Go CLI is intentionally removed by the
runner's owned temporary-tree cleanup; its real binary SHA remains in the
process proof. The separately owned original SDK-matched Go CLI/source tree
is retained and used for the supplemental closure comparisons. Source paths
make different build hashes; these are not misrepresented as identical
executables.

The final verification JSON names all paths and hashes. Every Linux process
uses disposable HOME/XDG roots and empty PATH, subreaper and umask022; no
operator state, credentials or external MCP/provider process is read. Native
Windows Unicode-path cases and Unix-byte inapplicability remain explicit,
including the inherited Windows audit-open wording deviation. No complete
#770/#769 cutover or foreign-platform acceptance is asserted.
