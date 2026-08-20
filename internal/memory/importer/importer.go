package importer

import (
	"time"

	"github.com/danieljustus/symaira-corekit/evidencekit"
)

// SessionRef represents a discovered session from an external tool.
type SessionRef struct {
	Tool       string            // "claude-code", "codex", "hermes", "aider", "git", "github"
	SessionID  string            // Unique identifier for the session
	Path       string            // File path or DB location
	ModifiedAt time.Time         // Last modification time
	Metadata   map[string]string // Tool-specific metadata
}

// ImportedFact represents a memory fact extracted from an external session.
type ImportedFact struct {
	Content   string                   // The memory content
	Source    string                   // Tool name (e.g., "claude-code", "git")
	SessionID string                   // Session identifier
	Timestamp time.Time                // When the fact was created
	Metadata  map[string]string        // Tool-specific context
	Evidence  []evidencekit.Extraction // Grounded span(s) backing Content, if any
}

// SessionImporter defines the interface for importing sessions from external tools.
type SessionImporter interface {
	// Name returns the tool name (e.g., "claude-code", "hermes", "git")
	Name() string

	// DiscoverSessions finds sessions modified since the given time.
	DiscoverSessions(since time.Time) ([]SessionRef, error)

	// ImportSession imports a single session and returns extracted facts.
	ImportSession(ref SessionRef) ([]ImportedFact, error)
}

// Categorizable is an optional interface for importers that belong to a specific category.
// Categories group related importers: "code", "communication", "calendar", "documents", "notes".
type Categorizable interface {
	Category() string
}

// PrivacyLevel represents the sensitivity level of imported data.
type PrivacyLevel string

const (
	PrivacyPublic       PrivacyLevel = "public"       // Non-sensitive (e.g., git commits)
	PrivacyInternal     PrivacyLevel = "internal"     // Internal tooling data
	PrivacyConfidential PrivacyLevel = "confidential" // PII, business data
	PrivacySecret       PrivacyLevel = "secret"       // Credentials, keys
)

// PrivacyAware is an optional interface for importers that handle sensitive data.
// Importers implementing this interface declare their privacy level and whether
// the PII guard should run on their output.
type PrivacyAware interface {
	PrivacyLevel() PrivacyLevel
	RequiresPIIGuard() bool
}

// IncrementalImporter is an optional interface for importers that track their own
// last import time. When implemented, the registry uses this instead of the
// global import_state table for discovery scheduling.
type IncrementalImporter interface {
	LastImportTime() (time.Time, error)
	MarkImported(ref SessionRef) error
}

// TranscriptImporter is an optional interface for importers that produce raw
// conversation transcripts (e.g. JSONL logs, chat databases). When implemented
// and extract-on-import is enabled, the registry runs extraction/summarization
// on the raw facts before storing them. Curated-memory sources that are already
// distilled should NOT implement this interface.
type TranscriptImporter interface {
	IsTranscript() bool
}
