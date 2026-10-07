Author handoff: native State-key bridge / #772 OsString successor

Validated immutable source: ccc5c87eb3bf69cf4adbf054b60d5e5342af3891.
Worktree: /workspace/symaira-daemon772-state-key-osargs.
Target: /workspace/symaira-daemon772-state-key-osargs/target (exclusive author until explicit release to Root).
Normal source ancestors: approved Registry b42730bfea8224d1cba1bb7883708b61e1ac096e, Windows pipe fixture c31b557be18adfdb32d81212e21b23c3d5e53cd4, main 2b6d49f250a81650eda1b93cec7d859a171873bb; old bridge publication 3c2d731b2b113df81db3601b73f3036c8a558e5c is untouched.

The public startup-owner boundary now passes native OsString arguments without UTF-8 conversion. Real ASCII, Unicode and Unix byteFF provider paths succeed through both the public API and the same internal CLI route, each actually querying its owned provider. A genuine path-corrupting Go child is rejected. The standalone provider file is byte-identical to the independently reviewed parent; existing key resolver, precedence, crypto helpers and standalone semantics remain. Registry progress is separate harness-only diagnostics, bounded to8192 events/8192 bytes and retains original Windows cancellations with no root-cause/runtime-fix claim.

The author's fresh all-target Darwin check exposed the existing rustix Apple FIFO API exclusion. The macOS test fixture now invokes absolute /usr/bin/mkfifo -m600 and verifies genuine FIFO/exact0600 before all original assertions. Other Unix retains rustix. No production change, platform skip or unsafe/dependency addition. After extraction, two missing Python imports were exposed by actual full runs and preserved before correction. Current runner/comparator restore datetime/json; normalize/compare/all8control functions remain text-identical to the original (only one empty module EOF line differs). All5 extracted-module global-name audits are clean. The supplementary original046 raw comparison explicitly remains an old-process-source observation.

Fresh cleanCCC gates PASS:
- Strict workspace/all-target/all-feature Clippy, fmt, actionlint, matrix88/17 acyclic/links; Windows core/all-target strict cross-Clippy and Darwin core/all-target cross-check (SOURCE ONLY).
- 209 affected tests +16 MCP library =225 passed,3 owned-test ignores; proper owned MCP-fixture FULL WORKSPACE310 passed/3 ignored.
-31 real Go/native key startup pairs +3 actual controls; six frozen encrypted v1/v2/v3 fixtures and authenticated destructive-cleanup observations retained.
-8 real ownership observations,9 Unix refusals,5 supervisor boundary observations including actual false-absence control,3 novel provider-path pairs +real path mutation.
-63 actual process pairs +3 actual controls;510 Registry CLI observations PER BINARY,20 recorded raw frames/8 concurrent clients/3 persisted Go files,7 cache-root observations via actual oracle supplemental,8 real comparator controls.
-13 actual MCP byte comparisons, all4 historical Go source digests match; FULL WORKSPACE test_exit0 and3 genuine Cargo101 missing-output/changed-output/missing-daemon controls rejected.
-8 pipe helper tests;5 Registry progress portable tests;6 platform-appropriate runner tests;3 genuine progress-module mutants rejected.
-Original root's exact3 public API/internal raw path observations rerun against same physically retained provider. Original4cd timeout input rerun with the same provider: both actual CLI exits1 after~5.027s; both provider PIDs gone at16.5s. No blanket instant-shutdown claim.
-2180 complete journal events, ending receipt.complete(matches=true), within original8192 bound.

Attribution/retention:
- Every197 candidate and670 immutableGo source entry rehashed against cleanCCC / clean frozen dcddcef0df5789123c7c9a7ebe6e01f10e941f2c. No Go/frozen-fixture edits.
- Actual current247 ELF paths/187 unique are retained, raw/compressed roundtrip verified;61 explicitly bound to fresh Cargo run/build logs. Other inherited Usage/shared cache files are not claimed to originate from State.
-524 distinct archive-payload roundtrips verified across original4cd/407/3db,1a91/330/046 andCCC receipts; original30 independentRoot proof files reverified against its REQUEST_CHANGES receipt. Old original review remains unchanged.
-Current archive /workspace/oracles/symaira-state-key-osargs-ccc-final/receipt.json,55849156 new compressed bytes; no target deletion. /proc exe/cwd/fd/maps users0.
-Original1a91 Darwin failure/225passes+ELF map,330 datetime failure/993journal,046 json failure/2178journal/all1020 executions, and all root3db/rawpath/orphan/registry baselines remain tracked and unchanged. Actual cancelled Windows full logs/ARM ZIP remain exact. Their cause remains unproved.

Raw reports/logs/scripts are under /tmp/symaira-state-key-osargs-jsonfix-* and tracked migration/evidence/browse-state-key-772/osargs-validated-ccc. Full executable archive and raw/runtime source verification: /tmp/symaira-state-key-osargs-jsonfix-verification.json. The full driver uses only the explicitly allocated target, jobs2/debug0/incremental0, pinnedGo1.26.7, subreaper and700MiB pre-stage guards. Full driver exits0; auxiliary exact originals/mutants exit0. Heavy compiler slot has been released to Root.

This is AUTHOR evidence, not independent approval. Required: different-author full review, fresh six native platform lanes (including real Windows descendant/closed-lifetime/sibling survival and macOS FIFO), full#772/release acceptance. Inherited root-prefix session parser gap and future explicit same-name keyless overwrite hardening stay documented/open; no E012 exception or global Crypto/Store weakening was introduced. No GitHub write/push/PR/main merge/issue closure.
