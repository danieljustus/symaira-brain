package discovery

import (
	"errors"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/importer"
)

func TestScanBuildsStableDeduplicatedSources(t *testing.T) {
	first := time.Date(2026, 8, 1, 12, 0, 0, 0, time.UTC)
	second := first.Add(time.Hour)

	response := Scan([]Provider{
		{
			ID:          "claude-code",
			Tool:        "symmemory",
			Kind:        "session-data",
			DisplayName: "Claude Code Sessions",
			Location:    "~/.claude/projects",
			PrivacyHint: "may_contain_personal_data",
			Discover: func() ([]importer.SessionRef, error) {
				return []importer.SessionRef{
					{Tool: "claude-code", SessionID: "session-1", ModifiedAt: first},
					{Tool: "claude-code", SessionID: "session-1", ModifiedAt: first},
					{Tool: "claude-code", SessionID: "session-2", ModifiedAt: second},
				}, nil
			},
		},
		{
			ID:       "unavailable",
			Tool:     "symmemory",
			Discover: func() ([]importer.SessionRef, error) { return nil, errors.New("offline") },
		},
	}, second.Add(time.Hour))

	if response.SchemaVersion != SchemaVersion {
		t.Fatalf("schema version = %d, want %d", response.SchemaVersion, SchemaVersion)
	}
	if len(response.Sources) != 1 {
		t.Fatalf("source count = %d, want 1", len(response.Sources))
	}

	source := response.Sources[0]
	if source.SourceID != "symmemory:claude-code" {
		t.Errorf("source ID = %q, want stable logical ID", source.SourceID)
	}
	if source.ItemCount != 2 {
		t.Errorf("item count = %d, want 2 after deduplication", source.ItemCount)
	}
	if source.LastSeen != second.Format(time.RFC3339) {
		t.Errorf("last seen = %q, want %q", source.LastSeen, second.Format(time.RFC3339))
	}
	if len(source.Capabilities) != 1 || source.Capabilities[0] != "import" {
		t.Errorf("capabilities = %#v, want [import]", source.Capabilities)
	}
}

func TestScanUsesScanTimeWhenSessionsHaveNoTimestamp(t *testing.T) {
	now := time.Date(2026, 8, 1, 12, 0, 0, 0, time.UTC)
	response := Scan([]Provider{{
		ID:   "paperless",
		Tool: "symmemory",
		Discover: func() ([]importer.SessionRef, error) {
			return []importer.SessionRef{{Tool: "paperless", SessionID: "1"}}, nil
		},
	}}, now)

	if got := response.Sources[0].LastSeen; got != now.Format(time.RFC3339) {
		t.Errorf("last seen = %q, want scan time %q", got, now.Format(time.RFC3339))
	}
}
