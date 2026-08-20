package memorytool

import (
	"time"
)

// ExportRef represents a discovered export file from an external memory tool.
type ExportRef struct {
	Tool       string
	Path       string
	Format     string
	ModifiedAt time.Time
}

// ImportedFact represents a memory fact imported from an external tool.
type ImportedFact struct {
	Content   string
	Source    string
	Timestamp time.Time
	Metadata  map[string]interface{}
}

// ImportResult tracks the outcome of an import operation.
type ImportResult struct {
	Created int
	Merged  int
	Skipped int
}

// MemoryToolImporter defines the interface for importing from memory-focused tools.
type MemoryToolImporter interface {
	Name() string
	DiscoverExports(path string) ([]ExportRef, error)
	ImportExport(ref ExportRef) ([]ImportedFact, error)
}
