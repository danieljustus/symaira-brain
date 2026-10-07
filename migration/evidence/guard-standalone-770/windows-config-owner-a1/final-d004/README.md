# Source-only validation of the Windows physical-owner fixture

Tested source: `d0041887` (full source identity in validation.json).

Run from a clean isolated checkout:

```
python3 migration/evidence/guard-standalone-770/windows-config-owner-a1/validate.py /workspace/owned-new-output
```

This performs ten portable Python tests and four static gates, including the
portable tests themselves, shell syntax, actionlint and whitespace. It verifies
126 original lossless retention payloads, 1760 unchanged production sources,
unchanged normal vectors/comparison criteria and unchanged incidental-failure
assertions. Actual normal cases remain 23 Windows/25 Unix; four typed/discovery
cases remain gated. No Go/native product or Windows API, compiler, target or
port was used. Local CPython source and path-API stand-ins are explicitly local
portable evidence, not a native CI observation.

The failed original CI control's child output was absent from its artifact;
that original attribution gap remains. The first authored portable failure and
the unpublished empty-index preparation mistake remain intact. The invalid
empty-index commit is not an ancestor of the corrected candidate.

Different-author source review and actual full Windows owner-control/config-path
execution, all original Guard gates and mandatory native OS CI remain required.
