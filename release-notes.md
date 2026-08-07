## What's changed

### Features
- #190 Add gateway-owned bootstrap tool — closes #185
- #191 Support room-local profiles via `serve --profile-file` — closes #189
- #192 Promote recurring sessions into portable agent context (recipes) — closes #186
- #195 Unify DMG branding with branded installer window — closes #193

### Fixes
- #188 Harden gateway identity, degradation, and MCP handshakes — closes #180, #181, #182, #184
- #196 Apply Cycle-03 audit findings (store perms, O(1) prune, coverage, artifact hygiene)
- #201 Plain-text tool errors and caller-wins identity injection — closes #197, #198

### Documentation
- #204 Refresh command reference and align version references — closes #203

### Dependencies
- #178 Bump symaira-corekit from 0.7.0 to 0.8.0
- #179 Bump actions-minor-patch group

### Closed Issues
- #180 Identity injection overrides caller-supplied parameters
- #181 Degradation reasons not surfaced to harnesses
- #182 Broker/tool failures not classified
- #184 Child MCP protocol-version mismatch not rejected
- #185 Gateway bootstrap tool
- #186 Recipes for recurring sessions
- #189 Room-local profiles
- #193 Unified DMG branding
- #197 Plain-text classified errors
- #198 Caller-wins identity injection
- #203 README command reference stale

**Full Changelog**: https://github.com/danieljustus/symaira-brain/compare/v0.4.3...v0.5.0
