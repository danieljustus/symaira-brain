# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
This file is maintained as part of the release flow: before each release,
the [Unreleased] section is moved into a dated version section.

## [Unreleased]

## [v0.4.1] - 2026-08-18

### Added
- Extract shared `config.DataDir()` helper for XDG data-dir resolution (#144)
- Doctor edge-branch tests: permission denied, valid anchor, spawn allowlist, malformed mcp.json (#144)
- `symguard doctor` reports real config/policy/audit state and exits non-zero on issues (#133)

### Fixed
- Stop doctor from flagging a missing audit anchor as an issue (#141)
- Propagate `symguard decide` exit code through the CLI router (#129)

### Changed
- Bump `github.com/danieljustus/symaira-corekit` (#135)
- Bump zizmorcore/zizmor-action from 0.6.1 to 0.6.2 (#127)
- Bump github/codeql-action/* from 4.37.4 to 4.37.5 (#124, #125, #126)

### Docs
- Regenerate AGENTS.md project-structure section (#140)
- Add pipeline diagram to README (#134)
- Stop enumerating internal packages in the status section (#128)

## [v0.4.0] - 2026-08-07

### Added
- Policy engine: deny/allow/require rule buckets with a fail-closed decision contract and rule tracing (#98)
- `symguard scan`: discover MCP servers across supported AI clients, with findings reporting (#99)
- Persisted `Proposal` type for durable policy-change requests (#102)
- Enumerable, scoped, revocable grant store, exposed as `symguard grants list|revoke` (#103)
- Deny-by-default spawn allowlist for stdio MCP server launches (#104)
- Short-lived purpose-bound capability tokens for headless callers (#105)
- Sequence-aware policy rule to detect agent loops, opt-in via `[sequence]` (#106)
- `symguard decide`: external classifier decision interface over JSON stdin/stdout (#107)
- Bounded update check, skipped on machine-facing commands (#116)

### Fixed
- Add `open-pull-requests-limit` to the dependabot configuration (#80)
- Set an explicit 7-day cooldown for dependabot updates (#100)
- Match `command_contains` rules with `strings.Contains` instead of exact substring equality (#109)

### Changed
- Ignore local review artifacts and tool state; add issue contact links (#110, #119)

### Tests
- Cover the `doctor` output surface (#117)
- Cover `grant.NewID` and `capability.LoadKey` fail-closed paths (#118)

### Docs
- Add community files, dependabot, and README improvements (#74)
- Design capability probing and sandbox confinement for the MCP proxy (#101)
- Document the Room/Guard boundary in AGENTS.md (#108)

### Deps
- Bump `symaira-corekit` (#82)
- Bump third-party GitHub Actions and codeql-action versions (#75, #76, #77, #78, #79, #83, #84, #85)

## [v0.3.0] - 2026-07-31

### Added
- versionkit handshake for GUI detection (#73)

### Changed
- Harden GitHub Actions workflows (#70)

## [v0.2.1] - 2026-07-31

### Fixed
- Repair CI workflow permissions and release signing (#68)
- Use cosign v3 `--bundle` flag for GoReleaser signing (#69)

### Changed
- Harden GitHub Actions workflows (#70)

## [v0.2.0] - 2026-07-30

### Added
- Marginal-capability risk capping in the policy engine (#64, #38)
- Default output format from TTY detection, not from the flag alone (#63)
- Reporter interface with table and JSON implementations (#62)
- Versioned rule catalog with fixtures and policy evaluation (#60)
- Redaction-safe evidence references and audit case bundles (#59)
- Approval-request wire contract in `internal/approval` (#58)
- Versioned action-event contract in `internal/model` (#57)
- Update detection via `corekit/updatecheck` (#66)

### Fixed
- Repair CI workflow permissions and release signing (#68)
- Chain-anchor checkpoint for audit-log truncation detection (#65)
- Use cosign v3 `--bundle` flag for GoReleaser signing (#69)

### Changed
- Split CLI subcommands into per-command packages (#61)

### CI & Security
- Emit build-provenance attestation for release artifacts (#56)
- SHA-pin third-party GitHub Actions and add zizmor workflow-security linting (#55)
- Add pinned govulncheck job to CI (#54)
- Add `-shuffle=on` to the test run (#53)

### Tests
- Add bounded fuzz targets and architecture seam guard (#67)

## [v0.1.0] - 2026-06-26

### Added
- Go module skeleton and CLI entrypoint (#5)
- MCP config discovery for common AI clients (#6)

### Docs
- Add community health files: SECURITY, CODEOWNERS, issue templates, badges (#27)
- Add LICENSE, repo topics, and configure squash-only merge (#28)

### Tests
- Add tests for the CLI entrypoint (#29)
- Add integration tests for OS-backed config and discovery wrappers (#32)

### CI & Security
- Enable branch protection on main (#30)
- Enable security features and add CodeQL scanning (#31)
- Add GitHub Actions workflow for tests and linting (#33)
