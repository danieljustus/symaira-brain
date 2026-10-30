# Platform scope decision (2026-10-29, #990)

This note supersedes older native-platform acceptance statements in port plans,
contract matrices, ADRs, and handoffs. Do not rewrite historical run results,
source snapshots, or frozen fixtures: they describe the contracts and evidence
that existed at their recorded revisions.

## Current targets

- **Product:** macOS on Apple Silicon, iOS, and Web.
- **Linux:** remains a CI/server platform and a CLI target where already
  applicable. Linux is not a substitute for Apple-platform product testing.
- **No longer supported or required:** native Windows and macOS Intel, including
  `x86_64-apple-darwin` / Darwin-amd64 release targets.

New acceptance plans must require macOS arm64 and Linux where relevant to the
CLI/server surface, and iOS/Web checks where those products are affected.
Windows- or Intel-only evidence may remain as historical evidence but must not
block current acceptance. Windows-only implementation is tracked separately
for removal and is not removed by this documentation change.
