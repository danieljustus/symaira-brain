## What's changed

### Memory Web Console and TUI retired (#763)
- The Memory Web Console is removed: `symbrain memory serve` serves only the JSON API (`/api/*`, including `/api/sync/*`); `/`, `/style.css` and `/app.js` answer `404`. Use the native macOS app to browse memories, or `symbrain memory list|search|get` from a terminal.
- The legacy Memory TUI was never reachable from the `symbrain` command tree and is retired. `memory tui|ui|web|dashboard` remain usage errors. No memory-domain operation or stored data is removed.

### Features
- #302 In-process skill sync and remote memory sync — closes #300 #301. `symbrain sync` now runs skill rendering/installation in-process (no archived `symskills` binary needed), and `symbrain memory sync --remote` replaces the archived `symmemory` runtime workflow with pull/push/token/encrypted-relay modes.
- #303 Canonicalize the `symbrain mcp` command and rename the app bundle to "Symaira Brain" — closes #295 #296. `serve` stays as a deprecated alias (stderr-only notice); a thin `config get/set/path` command is added; the macOS app bundle and DMG are renamed.
- #305 Publish the SymBrain GUI as a Homebrew cask — closes #294. Every release now ships `Casks/symbrain.rb` in the tap at the same version as the CLI formula.

### Docs
- #304 Bring the README onto the shared Symaira structure — closes #299.

### Chore
- #298 Stop tracking internal working artifacts.

### Closed Issues
- #300, #301, #295, #296, #299, #294

**Full Changelog**: https://github.com/danieljustus/symaira-brain/compare/v0.7.1...v0.7.2

The bounded native Memory HTTP owner corrects protected-read authentication,
nested search rendering and exact same-origin local writes. These are native
UI migration fixes; the complete `memory serve` command continues through its
existing implementation until configuration, all routes and native platform
gates pass. No shipped command or memory domain operation is removed.
