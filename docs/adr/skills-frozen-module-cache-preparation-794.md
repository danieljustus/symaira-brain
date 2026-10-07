# Skills oracle cache preparation on fresh native runners

The Windows PR794 job 111390294303 at head
`cec81cfc0acdc9f4bb567f85fa6a27c03cb74467` failed before any of the 312
original or 112 supplemental Skills process cases. Its actual merge checkout
was `1fb22f06b16dcf7e08c0c1531d827aee3a8c399d`; both commits have tree
`cfedf54df4c0d39a7b9685586ace3e69f2093de4`.

The four Git-source admission tests passed. The pinned Go 1.26.7 Windows/amd64
builder then verified all 2,438 frozen source files but failed at `go mod
verify`, with the retained child stderr:

```text
go: github.com/aymanbagabas/go-osc52/v2@v2.0.1: module lookup disabled by GOPROXY=off
```

Prior package tests and a successful current CLI build do not prove that every
module needed for the independently frozen dependency graph is cached. Cache
restoration is an optimization, not a prerequisite that a fresh native runner
may silently assume.

The dedicated builder therefore offers an explicit `--prepare-module-cache`
phase, enabled by the Windows Skills workflow. In the same verified frozen
clone and private HOME, `go mod download` uses only the public Go module proxy
and checksum database. Exact child argv, stdout/stderr bytes, exit and the
separate network policy are retained in the build receipt. Its existing
five-minute command budget is unchanged. The complete source map and clean
Git status must still match afterwards, including go.mod and go.sum. A failed
download or changed source stops acceptance.

The verification and actual build still use the original environment with
`GOPROXY=off`, `GOSUMDB=off`, `GOTOOLCHAIN=local`, and two workers. Preparation
does not mutate that environment or enable build-time fallback, direct VCS
network access, credentials, dependency upgrades, or frozen-source edits.
Existing source admission, binary identity, comparison, parent restoration,
filesystem snapshots and every process/control criterion remain intact.

Portable helper tests exercise environment separation, failure propagation,
and rejection of an actual changed frozen go.sum in an owned Git clone. They
mock the SDK download and establish no successful Go build or native Windows
result. The original complete native failure is retained separately; the
corrected native 312/112 cases, actual Windows parent build/restoration and
five input controls must pass in CI before this correction is accepted.
