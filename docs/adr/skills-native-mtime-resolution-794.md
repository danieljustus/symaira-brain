# Skills snapshot test timestamp resolution

The current PR794 Windows job111396574636 passed all312 original process pairs, then failed the portable snapshot self-test before the actual parent build or112 supplemental process cases. The test restored the original file bytes and requested an mtime change from10^18 to10^18+1 nanoseconds. Both complete22-row snapshots were equal; the owned file still reported10^18 nanoseconds.

Use a one-second mtime delta for this self-test. Windows FILETIME represents100-nanosecond intervals; requesting one nanosecond cannot establish a real metadata mutation there. A second exercises an observable metadata change without relying on Unix nanosecond resolution. Preserve the complete before/after snapshots, content and metadata assertions, immutable plan,312 original cases,112 Windows cases and all real failure controls. No product behavior or comparison is changed.

Four portable Python self-tests pass locally. The actual native Windows112-case run, parent restoration and controls remain required; this source correction does not claim those have executed. Full original logs, artifact ZIP, decoded equal snapshots and source bindings are retained in migration/evidence/skills-preflight-793/windows-mtime-resolution.
