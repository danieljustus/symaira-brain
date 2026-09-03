// Package usage implements per-harness AI subscription/token quota
// reporting ("symbrain usage"): a stable, machine-readable schema (issue
// #289) plus ten ported provider clients (issue #290) mirroring
// symaira-cockpit's Swift AI-usage providers. The schema is the agreed
// contract between this port and symaira-cockpit's usage screen (issue
// #22), which otherwise deadlock on each other's completion.
//
// Credential resolution (issue #290): every provider reads its credentials
// from an explicit env var first, and a symvault://<path> (or deprecated
// vault://<path>) value is resolved through the secret store
// (internal/memory/secrets) — never a second credential store. When the env
// var is unset, providers with a native CLI credential file fall back to
// that file strictly read-only (Codex, Copilot, Kimi, Claude, Nous Portal).
// The one credential this reaches past a file for is Claude's: Claude Code
// stores its OAuth token in the macOS login keychain and writes no
// credentials file there, so a Mac with a signed-in Claude Code read as
// "not configured". It is read last (after the env var and the file) via a
// build-tagged helper that is a no-op off macOS, so nothing else in the
// package becomes platform-specific. Two providers (Cursor, OpenCode) still
// drop a SQLite-backed local-history strategy that would need a new
// dependency decision to port cross-platform. See AllProviders and each
// provider's doc comment for exactly what was and wasn't ported.
package usage
