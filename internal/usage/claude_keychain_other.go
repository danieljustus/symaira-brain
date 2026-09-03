//go:build !darwin

package usage

import "time"

// readClaudeKeychainCredential is macOS-only: there is no cross-platform
// keychain to read, and the file fallback in claude.go is what other
// platforms have. Mirrors the split in corekit's secretref package.
func readClaudeKeychainCredential() (string, *time.Time) { return "", nil }
