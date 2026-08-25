// Package usage implements per-harness AI subscription/token quota
// reporting ("symbrain usage"): a stable, machine-readable schema (issue
// #289) plus ten ported provider clients (issue #290) mirroring
// symaira-cockpit's Swift AI-usage providers. The schema is the agreed
// contract between this port and symaira-cockpit's usage screen (issue
// #22), which otherwise deadlock on each other's completion.
//
// Every provider's credential resolution here is portable — env vars and
// plain file reads — never the macOS Keychain, and two providers (Cursor,
// OpenCode) drop a SQLite-backed local-history strategy that would need a
// new dependency decision to port cross-platform. See AllProviders and
// each provider's doc comment for exactly what was and wasn't ported.
package usage
