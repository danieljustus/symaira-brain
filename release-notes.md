## What's changed

### Fixes
- #208 Protect the gateway-managed recipe history directory with private
  permissions while preserving permissions for external directories.
- #209 Serialize scheduled broker child restarts and re-check lifecycle state
  to prevent duplicate replacement processes during concurrent requests.
- #210 Make `make lint` propagate failures from an installed `golangci-lint`
  instead of falling through to `go vet`, and resolve the existing lint
  baseline.

### Closed Issues
- #206 Scheduled broker child restart race
- #207 `make lint` masked linter failures

**Full Changelog**: https://github.com/danieljustus/symaira-brain/compare/v0.5.0...v0.5.1
