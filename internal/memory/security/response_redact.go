package security

import "github.com/danieljustus/symaira-brain/internal/memory/db"

// RedactMemory redacts a Memory's content and metadata in place. It is
// defense-in-depth for records that reached storage without write-time
// redaction (legacy imports, or content that predates the redaction
// pipeline) — every retrieval transport applies it just before content
// leaves the process, on top of the write-time redaction already applied
// in internal/memory.Prepare and the importers.
func RedactMemory(m *db.Memory) {
	if m == nil {
		return
	}
	m.Content = Redact(m.Content)
	m.Metadata = RedactMap(m.Metadata)
}

// RedactMemories redacts every Memory in place. Nil entries are skipped.
func RedactMemories(mems []*db.Memory) {
	for _, m := range mems {
		RedactMemory(m)
	}
}

// RedactSearchResults redacts every result's Memory in place. Nil entries
// and results with a nil Memory are skipped.
func RedactSearchResults(results []db.SearchResult) {
	for _, r := range results {
		RedactMemory(r.Memory)
	}
}
