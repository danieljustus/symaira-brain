package github

import (
	"strings"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/importer"
)

func TestNewGitHubImporter(t *testing.T) {
	imp := NewGitHubImporter("owner", "repo", "token123")

	if imp.Name() != "github" {
		t.Errorf("Name() = %q, want %q", imp.Name(), "github")
	}

	if imp.Category() != "code" {
		t.Errorf("Category() = %q, want %q", imp.Category(), "code")
	}

	if imp.PrivacyLevel() != "internal" {
		t.Errorf("PrivacyLevel() = %q, want %q", imp.PrivacyLevel(), "internal")
	}

	if imp.RequiresPIIGuard() {
		t.Error("RequiresPIIGuard() = true, want false")
	}
}

func TestNewGitHubImporterDefaults(t *testing.T) {
	imp := NewGitHubImporter("", "", "")

	if imp.owner != "" {
		t.Errorf("owner = %q, want empty", imp.owner)
	}

	if imp.repo != "" {
		t.Errorf("repo = %q, want empty", imp.repo)
	}

	if imp.token != "" {
		t.Errorf("token = %q, want empty", imp.token)
	}
}

func TestImportSessionInvalidSessionID(t *testing.T) {
	imp := NewGitHubImporter("owner", "repo", "token123")

	ref := importer.SessionRef{
		SessionID: "invalid-format-no-slash",
	}

	_, err := imp.ImportSession(ref)
	if err == nil {
		t.Fatal("expected error for invalid session ID format")
	}

	if !strings.Contains(err.Error(), "invalid session ID format") {
		t.Errorf("expected error containing 'invalid session ID format', got %q", err.Error())
	}
}

func TestImportSessionUnknownItemType(t *testing.T) {
	imp := NewGitHubImporter("owner", "repo", "token123")

	ref := importer.SessionRef{
		SessionID: "unknown/123",
	}

	_, err := imp.ImportSession(ref)
	if err == nil {
		t.Fatal("expected error for unknown item type")
	}

	if !strings.Contains(err.Error(), "unknown item type") {
		t.Errorf("expected 'unknown item type' error, got %q", err.Error())
	}
}

func TestImportSessionPRFailsCLINotAvailable(t *testing.T) {
	// Exercises the exec.Command error path for "gh pr view"
	imp := NewGitHubImporter("owner", "repo", "token123")
	ref := importer.SessionRef{SessionID: "pr/42"}

	_, err := imp.ImportSession(ref)
	if err == nil {
		t.Fatal("expected error from gh pr view (CLI not available)")
	}
}

func TestImportSessionIssueFailsCLINotAvailable(t *testing.T) {
	// Exercises the exec.Command error path for "gh issue view"
	imp := NewGitHubImporter("owner", "repo", "token123")
	ref := importer.SessionRef{SessionID: "issue/42"}

	_, err := imp.ImportSession(ref)
	if err == nil {
		t.Fatal("expected error from gh issue view (CLI not available)")
	}
}

func TestDiscoverSessionsSwallowsCLErrors(t *testing.T) {
	// Exercises both discoverPRs and discoverIssues error paths.
	// The method catches errors internally and writes warnings to stderr.
	imp := NewGitHubImporter("owner", "repo", "token123")

	sessions, err := imp.DiscoverSessions(time.Now())
	if err != nil {
		t.Fatalf("DiscoverSessions should swallow CLI errors, got: %v", err)
	}
	if len(sessions) != 0 {
		t.Logf("DiscoverSessions returned %d sessions (expected 0 with no gh CLI)", len(sessions))
	}
}
