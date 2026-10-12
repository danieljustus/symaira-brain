# Skills control-reader corrected reference v1

Status: authorized source preparation only, NOT an accepted oracle or migration
result. Owner: Kanban `symaira-brain/t_1a8f3c00`; allocator: `6103fb3fcf3b`.
No compiler, product process, native differential, publication or pin update is
part of this preparation. The existing storage hold remains in force.

## Historical reference is immutable

The original reference is Go source `dcddcef0df5789123c7c9a7ebe6e01f10e941f2c`
with Go 1.26.7. The retained `native-all-capture.json` on candidate `5cbef2d9`
has 1112 actual pairs, 903 matches, 209 failures and zero executed original
controls. It remains a failed diagnostic, never a passing corrected baseline.
The independently source-hashed diagnosis is `contract-diagnostic-b5b70417.json`
(SHA256 `57990732c1feb251a09b9ae0725f16979fe407d570f7b279d3dbf025beeebc0a`).
Both live under `Dev/evidence/symaira-rust/coordination-20261011/brain/`.

That diagnosis disproves the original-Go nofollow-PathError premise in
`skills-native-byte-contracts-764.md`, decision 6: Go 1.26.7 `os.Root.OpenFile`
resolves final links despite caller `O_NOFOLLOW`. Twelve in-root control-link
rows succeeded in the actual frozen Go process but were refused natively.
This is success versus refusal, not an error-formatting difference. The old
ADR, raw captures, original wires, source archives and failed results remain
unchanged as historical evidence.

## Bounded correction, not a compatibility waiver

The allocator authorized repair of the Go reader to the EXISTING mandatory
final-Unix-nofollow rule. The proposed new reference label is
`skills-control-nofollow-v1`. It has no accepted commit, executable or capture
yet. No selected-file/self-computed hash can substitute for its later complete
compiler-input closure and independently anchored build receipt.

- At the shared Go `openRootFile` seam, only exact control basenames `SKILL.md`
  and `symskills.toml` use the dedicated reader. This includes frontmatter-only
  reads without changing the resource/overlay reader's general semantics.
- The dedicated reader opens `.` through the retained `os.Root`, holds that
  directory file through `SyscallConn.Control`, then invokes kernel `Openat`
  with a validated control basename, `O_NOFOLLOW`, `O_NONBLOCK` and `O_CLOEXEC`.
  It never resolves an ambient root pathname or passes the flag back into
  `os.Root.OpenFile`. The existing same-file regular-type and size/read budget
  checks still run. Optional manifest existence uses `Lstat` so a dangling
  final link is not mistaken for an absent manifest.
- A failed final control open is a `PathError` with operation `openat` and the
  exact basename, wrapped once by `read <name>`. The Rust manifest caller must
  therefore not pass an already-prefixed label to its common reader. Native
  outside/unverified refusal remains stronger; no metadata probe reads target
  content. Existing confined discovery identity is a SEPARATE bounded owner.
- Windows retains its existing `Root.Open` behavior and requires actual native
  evidence; this decision does not redefine Windows reparse semantics.
- Resource-link metadata observes the link inode without target recursion or
  reads. Fixture link mtimes are explicit input, one second newer than regular
  entries. This is deterministic fixture preparation, not output normalization.
  The general timestamp/marker/access-time contracts remain unchanged.

The 194 malformed-Skills errors do not need a reference version change. Their
existing Go syntax/depth/EOF messages remain the contract. A separate structural
diagnostic probe reads only direct envelope routing fields and direct params
name over all supplied frame bytes, including overrides after malformed opaque
values. It can select an error only, not construct/admit a request. Ambiguous
routing/structure and ordinary owners retain inherited errors and UTF-8 handling.
Physical Content-Length truncation remains an I/O failure before this probe.

## Required follow-up before promotion

1. After a fresh allocator slot-A grant and healthy `dev-external --status`,
   execute focused Go reader tests, Rust MCP/Skills/CLI tests, formatting and
   strict all-target lint under the existing isolated external build roots.
   Source/static checks do not replace these gates.
2. Freeze the exact source candidate and obtain independent SPEC/parity AND
   QUALITY/security delta review, including post-error routing ambiguity,
   root rename/replacement, malicious/special targets and capability lifetimes.
3. The integration owner must make the reviewed corrected Go source permanently
   reachable, then independently bind the complete compiler inputs, dependency
   modules, pinned SDK, executable and new capture to that source. Use a NEW
   versioned receipt/capture; do not relabel or overwrite the dcddcef0 reference.
   Working-tree-bound pins need a deliberate reviewed follow-up, not automatic
   regeneration during this preparation.
   In particular, `scripts/skills-native-oracle/run.py:166` and
   `ordinary_startup.py:9` still require the historical dcddcef0 identity;
   neither was changed here. Do not feed a corrected binary to that historical
   receipt. The later versioned gate must bind its new identity explicitly and
   keep the old gate/capture available rather than accepting arbitrary pins.
4. Re-execute ALL original 1112 Unix / 1070 Windows pairs plus additive inputs,
   all nine original real-process controls and affected callers with strict
   comparisons and actual native Linux/macOS/Windows receipts. Retain failures
   separately. No reduced corpus, permissive comparator, synthetic success or
   review-by-self is authorized. Existing #834/#835/#856 and whole Brain,
   Memory/Browse/Guard/Scope acceptance residuals remain separate and open.

Only the later integration owner can promote the corrected reference after
these gates. PR #813 remains untouched/draft; no push, merge, release, cutover
or Go deletion is authorized by this document.
