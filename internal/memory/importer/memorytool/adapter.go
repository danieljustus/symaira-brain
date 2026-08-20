package memorytool

import (
	"fmt"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/importer"
)

// SessionAdapter wraps a MemoryToolImporter so it can be registered in the
// importer.Registry alongside session-based importers.
type SessionAdapter struct {
	imp  MemoryToolImporter
	path string
}

// NewSessionAdapter creates an adapter for the given memory-tool importer.
// If path is empty, the underlying importer will use its default location.
func NewSessionAdapter(imp MemoryToolImporter, path string) *SessionAdapter {
	return &SessionAdapter{imp: imp, path: path}
}

// Name returns the wrapped importer's name.
func (s *SessionAdapter) Name() string {
	return s.imp.Name()
}

// Category returns "memory-tool" for Categorizable consumers.
func (s *SessionAdapter) Category() string {
	return "memory-tool"
}

// PrivacyLevel returns confidential because memory-tool exports may contain PII.
func (s *SessionAdapter) PrivacyLevel() importer.PrivacyLevel {
	return importer.PrivacyConfidential
}

// RequiresPIIGuard returns true because memory-tool exports may contain PII.
func (s *SessionAdapter) RequiresPIIGuard() bool {
	return true
}

// DiscoverSessions discovers export files modified since the given time.
func (s *SessionAdapter) DiscoverSessions(since time.Time) ([]importer.SessionRef, error) {
	refs, err := s.imp.DiscoverExports(s.path)
	if err != nil {
		return nil, err
	}

	var sessions []importer.SessionRef
	for _, ref := range refs {
		if ref.ModifiedAt.Before(since) {
			continue
		}
		sessions = append(sessions, importer.SessionRef{
			Tool:       ref.Tool,
			SessionID:  ref.Path,
			Path:       ref.Path,
			ModifiedAt: ref.ModifiedAt,
		})
	}
	return sessions, nil
}

// ImportSession imports a single export file and converts facts to the
// importer.Registry format.
func (s *SessionAdapter) ImportSession(ref importer.SessionRef) ([]importer.ImportedFact, error) {
	facts, err := s.imp.ImportExport(ExportRef{
		Tool:       ref.Tool,
		Path:       ref.Path,
		Format:     "json",
		ModifiedAt: ref.ModifiedAt,
	})
	if err != nil {
		return nil, err
	}

	result := make([]importer.ImportedFact, 0, len(facts))
	for _, f := range facts {
		metadata := make(map[string]string, len(f.Metadata))
		for k, v := range f.Metadata {
			metadata[k] = fmt.Sprint(v)
		}
		result = append(result, importer.ImportedFact{
			Content:   f.Content,
			Source:    f.Source,
			SessionID: ref.SessionID,
			Timestamp: f.Timestamp,
			Metadata:  metadata,
		})
	}
	return result, nil
}
