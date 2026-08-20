package email

import (
	"strings"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/importer"
)

func TestNewEmailImporter(t *testing.T) {
	imp := NewEmailImporter("INBOX", "high", 2000)

	if imp.Name() != "email" {
		t.Errorf("Name() = %q, want %q", imp.Name(), "email")
	}

	if imp.Category() != "communication" {
		t.Errorf("Category() = %q, want %q", imp.Category(), "communication")
	}

	if imp.PrivacyLevel() != "confidential" {
		t.Errorf("PrivacyLevel() = %q, want %q", imp.PrivacyLevel(), "confidential")
	}

	if !imp.RequiresPIIGuard() {
		t.Error("RequiresPIIGuard() = false, want true")
	}
}

func TestNewEmailImporterDefaults(t *testing.T) {
	imp := NewEmailImporter("", "", 0)

	if imp.folder != "" {
		t.Errorf("folder = %q, want empty", imp.folder)
	}

	if imp.importance != "" {
		t.Errorf("importance = %q, want empty", imp.importance)
	}

	if imp.maxBody != 2000 {
		t.Errorf("maxBody = %d, want %d", imp.maxBody, 2000)
	}
}

func TestNewEmailImporterCustomMaxBody(t *testing.T) {
	imp := NewEmailImporter("Sent", "", 500)
	if imp.maxBody != 500 {
		t.Errorf("maxBody = %d, want %d", imp.maxBody, 500)
	}
}

func TestMatchesSender(t *testing.T) {
	tests := []struct {
		name           string
		includeSenders []string
		excludeSenders []string
		from           string
		want           bool
	}{
		{
			name:           "no filters",
			includeSenders: nil,
			excludeSenders: nil,
			from:           "test@example.com",
			want:           true,
		},
		{
			name:           "include match",
			includeSenders: []string{"example.com"},
			excludeSenders: nil,
			from:           "test@example.com",
			want:           true,
		},
		{
			name:           "include no match",
			includeSenders: []string{"other.com"},
			excludeSenders: nil,
			from:           "test@example.com",
			want:           false,
		},
		{
			name:           "exclude match",
			includeSenders: nil,
			excludeSenders: []string{"spam.com"},
			from:           "test@spam.com",
			want:           false,
		},
		{
			name:           "exclude no match",
			includeSenders: nil,
			excludeSenders: []string{"spam.com"},
			from:           "test@example.com",
			want:           true,
		},
		{
			name:           "include takes precedence",
			includeSenders: []string{"example.com"},
			excludeSenders: []string{"spam@example.com"},
			from:           "spam@example.com",
			want:           true,
		},
		{
			name:           "partial address match include",
			includeSenders: []string{"newsletter"},
			excludeSenders: nil,
			from:           "newsletter@company.com",
			want:           true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			imp := &EmailImporter{
				includeSenders: tt.includeSenders,
				excludeSenders: tt.excludeSenders,
			}
			if got := imp.matchesSender(tt.from); got != tt.want {
				t.Errorf("matchesSender(%q) = %v, want %v", tt.from, got, tt.want)
			}
		})
	}
}

func TestDiscoverSessionsFailsNoHimalaya(t *testing.T) {
	imp := NewEmailImporter("INBOX", "", 2000)

	_, err := imp.DiscoverSessions(time.Now().Add(-24 * time.Hour))
	if err == nil {
		t.Fatal("expected error (himalaya not available)")
	}
	if !strings.Contains(err.Error(), "himalaya") {
		t.Errorf("expected error about himalaya, got %q", err.Error())
	}
}

func TestDiscoverSessionsEmptyFolderDefaultsToINBOX(t *testing.T) {
	imp := NewEmailImporter("", "", 2000)

	_, err := imp.DiscoverSessions(time.Now().Add(-24 * time.Hour))
	if err == nil {
		t.Fatal("expected error")
	}
}

func TestImportSessionFailsNoHimalaya(t *testing.T) {
	imp := NewEmailImporter("INBOX", "", 2000)
	ref := importer.SessionRef{
		SessionID: "test-msg-123",
		Metadata: map[string]string{
			"folder":  "INBOX",
			"subject": "Test Subject",
			"from":    "sender@example.com",
			"date":    "2026-07-01T10:00:00Z",
		},
	}

	_, err := imp.ImportSession(ref)
	if err == nil {
		t.Fatal("expected error (himalaya not available)")
	}
}

func TestImportSessionEmptyFolderDefaultsToINBOX(t *testing.T) {
	imp := NewEmailImporter("", "", 2000)
	ref := importer.SessionRef{
		SessionID: "test-msg-456",
		Metadata: map[string]string{
			"subject": "No Folder",
			"from":    "test@example.com",
		},
	}

	_, err := imp.ImportSession(ref)
	if err == nil {
		t.Fatal("expected error")
	}
}

func TestDiscoverSessionsWithImportanceFilter(t *testing.T) {
	imp := NewEmailImporter("INBOX", "high", 2000)

	_, err := imp.DiscoverSessions(time.Now().Add(-24 * time.Hour))
	if err == nil {
		t.Fatal("expected error")
	}
}

func TestDiscoverSessionsWithSenderFilters(t *testing.T) {
	imp := NewEmailImporter("INBOX", "", 2000)
	imp.includeSenders = []string{"work@example.com"}

	_, err := imp.DiscoverSessions(time.Now().Add(-24 * time.Hour))
	if err == nil {
		t.Fatal("expected error")
	}

	imp2 := NewEmailImporter("INBOX", "", 2000)
	imp2.excludeSenders = []string{"spam@example.com"}

	_, err = imp2.DiscoverSessions(time.Now().Add(-24 * time.Hour))
	if err == nil {
		t.Fatal("expected error")
	}
}
