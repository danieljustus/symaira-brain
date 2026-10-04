# Guard generated configuration paths retain Go lexical ownership

Decision: preserve the native ordered-warning slice, and correct generated
configuration paths before probing, opening, or formatting their filenames.
`XDG_CONFIG_HOME` and the HOME fallback use Go lexical join/clean rules. An
explicit nonempty `SYMGUARD_CONFIG` remains the caller's raw OS path. Filesystem
canonicalization would select the wrong file through a symlink followed by `..`;
it is not an implementation of `filepath.Join`.

The independent review of immutable publication `4d78b6e52c53d1c273126b0dac3c554915a15438`
(source `fb2844b02b7c03b7c34650c6d3aed693c47b2997`) requested one P2 correction.
A private `link -> physical/anchor` and generated `link/../cfg` cause frozen Go
to read `cfg/symguard/config.toml`, while the previous native implementation
read `physical/cfg/symguard/config.toml`. The actual semantic variant returned
Go error/exit1 versus native healthy/exit0. Ordinary `cfg/../cfg` also retained
the wrong literal filename in native warnings. Plain paths and explicit raw
configuration overrides matched. Known-only resolver errors were inherited
from the approved parent; unknown-key states had remained delegated there and
became newly admitted by the warning increment. This is a repair of that
admission, not an exception to output equality.

The untouched report/receipt, all 92 independent proof files, and all 180 raw
file/symlink state paths are retained under
`migration/evidence/guard-doctor-config-paths-770/independent-4d78`.
The existing immutable ELF archive retains 69 mappings/46 unique executable
byte sequences; its complete gzip roundtrip, length and SHA were checked before
target reuse. Inactive cached artifacts do not imply new executions. All earlier
ordered-warning, typed/delegation, diagnostic, kernel and actual mutation gates,
the original inputs and all 50 nondeterministic-default reports remain required.

The implementation reuses Guard's private scan path entrypoints, moving the
small lexical helper into a Guard-only module to keep production files below
400 lines. Doctor generated paths use the same helper as scan source discovery.
Unix cleaning retains its existing component algorithm and original OsStr bytes.
No shared Core/Brain/installer/managed-store dependency is introduced. Explicit
configuration overrides bypass cleaning. Typed decode, warning ordering,
semantic validation and complete-report buffering do not change.

The pinned Go 1.26.7 Windows source also supplies rules that Rust Components
alone cannot express: cleaning dot components inside verbatim/device paths,
retaining UNC volumes and rooted boundaries, joining bare drive-relative bases,
and preventing cleanup from creating a drive or Root Local Device path.
The Go lazy-buffer rewrite flag also matters: merely removing a trailing
separator can leave a colon-bearing relative spelling unchanged, while slash
rewrites or removal of interior components trigger postClean. The reference
corpus retains both forms rather than simplifying them into one path type.
A small Windows-only implementation preserves UTF16 units, including unpaired
surrogates. The initial static assumption that Go Getenv replaces those units
was corrected before compilation: this actual SDK's `UTF16ToString` explicitly
uses WTF-8, and the original assumption/correction messages remain in the
session history. `windows-sdk-source.json` pins the exact Clean/Join/tests,
Windows environment/UTF16 code and BSD license source hashes. The owned portable
reference copies the actual SDK algorithms with declared narrow import/driver
adaptations and links the exact production Rust helper. It is source-reference
evidence, not proof that Windows file APIs or CLI diagnostics ran on Linux.

Core GoText's current non-Unix diagnostic conversion remains lossy for unpaired
UTF16 units. This correction preserves file ownership but does not silently
port that separate diagnostic surface: Windows doctor config paths that cannot
be represented as Unicode remain delegated before any read or output. A native
Windows-only exact Brain-adapter child test guards the empty-stream refusal.
Scan's wider raw-Windows diagnostic surface remains unproven; no full scan or
Windows acceptance is inferred from helper/reference tests.

New permanent owned process cases cover XDG/HOME precedence, plain/dot/dotdot,
relative and symlink owners, raw Unix path bytes, explicit override semantics,
semantic errors, and typed/discovery delegation without partial native output.
A genuine process mutant canonicalizes the generated base, reads the physical
owner, and must be rejected by the unchanged full report/state comparator.
The CI runner retains these reports plus the Windows source-reference report
and actual probe executable bytes on all three native jobs.

Validation is pending at this source checkpoint. The compiler hold is respected;
Python syntax, Bash syntax and source formatting do not establish Rust/runtime
acceptance. Fresh ordinary/strict tests, every inherited process gate, original
corpus accounting, all controls, independent review and native Linux/macOS/Windows
protected checks remain necessary. Full issues #770/#769 remain open.
