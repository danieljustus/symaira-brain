// Package patterns turns repeated gateway sessions into reviewable,
// versioned pattern artifacts (issue #186).
//
// An episode is the ordered tool-call sequence of one harness connection
// as seen by the gateway routing layer — names only, never arguments or
// values. A candidate is promoted to an exposable pattern only after the
// same sequence recurs across a configurable number of sessions; the
// promotion gate is what keeps the store from filling with one-off
// noise. Brain exposes patterns as read-only context; it never executes
// them, and anything that stabilizes into a durable authored artifact
// belongs to symskills, not here.
package patterns
