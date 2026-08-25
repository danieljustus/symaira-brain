// Package usage defines the stable, machine-readable contract for
// per-harness AI subscription/token quota reporting ("symbrain usage").
//
// This package currently holds only the schema (issue #289) — no provider
// fetch logic yet. The schema is the agreed contract between the Go port
// (issue #290) and symaira-cockpit's usage screen (issue #22), which
// otherwise deadlock on each other's completion.
package usage
