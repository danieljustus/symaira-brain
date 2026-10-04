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
The first additional before-fix driver compared two relative paths containing
different private side names. Its full receipt/log remain unchanged, and a
separate scope correction records those two confounded comparisons. The future
relative-parent case uses the same `../discard/../<leaf>` spelling on both
sides; this is a harness correction, not a new production difference or a
rewrite of the immutable independent review's owner proofs.
The CI runner retains these reports plus the Windows source-reference report
and actual probe executable bytes on all three native jobs.

Clean source `5bbd42774b73760731801ca148663350b9770b47` passes 177 ordinary
Core/Guard tests in 15 executed test binaries, the separate overlapping 41-test
kernel graph in seven binaries, both strict Clippy graphs, workspace formatting
and actionlint. The fresh actual SDK Go/native process gate passes 122/124 full
reports, all 63 raw cases, 67/78 additive reports, 46/54 ordered-warning reports,
and 21/25 generated-path reports. Each remaining case is an explicit refusal;
all 13 inherited actual mutation controls and the new wrong-owner process
mutation are rejected. The 386/386 portable Windows source comparisons include
all 34 original SDK Clean examples. This is source-reference evidence only.

All 25 corrected before-fix inputs are mapped case by case in the final receipt,
including their original environment spellings, input bytes and readonly state.
Of the 15 original failures, 13 now match completely and two typed-config cases
correctly refuse before native output. The original 20 owner-history pairs and
the later ten fresh owner pairs also match on the corrected executable. Fresh
replays account for all original 80/94 CLI inputs, prior ordered/Unicode/raw and
parent cases, the exact Brain-adapter children, and all nine original Scan
fixtures including TTY/repeats. The prior signed-hex TOML parser refusal remains
explicitly classified; audit filesystem controls assert native safety properties
and retain observed inherited Go writes rather than claiming blanket parity.
These adapter checks execute the unchanged Brain source inside Guard test
binaries; they do not establish acceptance of a complete Brain executable.

The first actual compilation exposed Windows helper backtracking errors; the
copied SDK then exposed stale allocated-buffer and UTF8 byte-width behavior.
Those failed reports, original executable probe bytes, correction commits and
strict-lint failures are retained separately from the final passing source.
The final archive distinguishes all 67 cached target ELF paths from the 22
actually executed test roles, and separately retains the actual public Go/native
pair and two portable SDK probe executables. Intermediate F02 ordinary test logs
have no complete retained executable binding and are not the final acceptance
proof. No original Go input, frozen fixture, independent finding or failure
receipt was rewritten.

Full source maps, original/raw-state retention, exact executable SHA/bytes,
case accounting and gate logs are under
`migration/evidence/guard-doctor-config-paths-770/final-5bb`.
A different author must independently review this immutable candidate. Native
Linux/macOS/Windows protected checks, remaining typed/discovery diagnostics,
raw Windows text and complete standalone/Brain admission remain necessary.
Full issues #770/#769 remain open.
