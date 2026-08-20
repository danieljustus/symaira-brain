## What's changed

### Features
- #250 Add passthrough subcommands for vault, memory, and skills
- #251 Managed core runtime — setup, doctor --fix, resolution order
- #249 Unified runtime availability and async audit reader in GUI
- #217 Add Memory, Vault and Skills module screens to macOS app

### Fixes
- #230 Bound the Memory list to prevent AppKit row-height cache reentry
- #232 Replace Memory list instead of diffing when search changes rows
- #229 Name component behind each version number; label buttons for assistive tech
- #246 Audit improvements: redaction, scanner fix, --json flag

### CI
- #252 Auto-bump PRs for managed core releases
- #220 Add PR-time build/test gate for Swift GUI targets

### Dependencies
- #224 Bump github.com/danieljustus/symaira-corekit to v0.9.1
- #253 Bump actions-minor-patch group (2 updates)

### Testing
- #256 Make Install/downloadAndVerify testable without network hits
- #259 Add coverage tests for setup commands and audit TailEntries
- #261 Add error-path tests for managed download/extract/install

**Full Changelog**: https://github.com/danieljustus/symaira-brain/compare/v0.6.0...v0.7.0
