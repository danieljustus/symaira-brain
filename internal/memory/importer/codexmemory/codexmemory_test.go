package codexmemory

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/importer"
)

func fixtureRoot(t *testing.T) string {
	t.Helper()
	root, err := filepath.Abs("testdata")
	if err != nil {
		t.Fatal(err)
	}
	return root
}

func TestImporterClassificationAndIncrementalCursor(t *testing.T) {
	imp := NewCodexMemoryImporter(fixtureRoot(t))
	if imp.Name() != "codex-memory" || imp.Category() != "activity" {
		t.Fatalf("classification = %q/%q, want codex-memory/activity", imp.Name(), imp.Category())
	}
	if imp.PrivacyLevel() != importer.PrivacyConfidential || !imp.RequiresPIIGuard() || !imp.IsTranscript() {
		t.Fatal("activity importer must be confidential, PII guarded, and transcript-like")
	}
	if !imp.StageImportedFacts() || !imp.ContentIsUntrusted() {
		t.Fatal("activity importer must be staged and marked untrusted")
	}
	before, err := imp.LastImportTime()
	if err != nil || !before.IsZero() {
		t.Fatalf("initial cursor = %v, %v", before, err)
	}
	modified := time.Date(2026, 8, 28, 10, 0, 0, 0, time.UTC)
	if err := imp.MarkImported(importer.SessionRef{ModifiedAt: modified}); err != nil {
		t.Fatal(err)
	}
	after, err := imp.LastImportTime()
	if err != nil || !after.Equal(modified) {
		t.Fatalf("cursor = %v, %v, want %v", after, err, modified)
	}
}

func TestDiscoverSessionsMtimeMetadataAndSuppression(t *testing.T) {
	imp := NewCodexMemoryImporter(fixtureRoot(t))
	sessions, err := imp.DiscoverSessions(time.Time{})
	if err != nil {
		t.Fatal(err)
	}
	if len(sessions) != 6 {
		t.Fatalf("discovered %d sessions, want 6 (covered 10min suppressed)", len(sessions))
	}
	for _, session := range sessions {
		if filepath.Base(session.Path) == "instructions.md" {
			t.Fatal("instructions.md must never be discovered")
		}
		if session.Metadata["sync_exclude"] != "true" {
			t.Errorf("%s missing sync exclusion", session.Path)
		}
	}
	for _, session := range sessions {
		if strings.Contains(session.Path, "10-05-00-abcd-10min") {
			t.Fatal("10min resource covered by 6h summary was not suppressed")
		}
		if strings.Contains(session.Path, "6h-settings") {
			if session.Metadata["granularity"] != "6h" || session.Metadata["extension"] != "skysight" {
				t.Errorf("6h metadata = %#v", session.Metadata)
			}
		}
	}

	future := time.Now().Add(24 * time.Hour)
	sessions, err = imp.DiscoverSessions(future)
	if err != nil {
		t.Fatal(err)
	}
	if len(sessions) != 0 {
		t.Fatalf("future cutoff discovered %d sessions, want 0", len(sessions))
	}
}

func TestImportSessionFrontmatterCitationsAndEvidence(t *testing.T) {
	imp := NewCodexMemoryImporter(fixtureRoot(t))
	path := filepath.Join(fixtureRoot(t), "extensions", "skysight", "resources", "2026-08-28T10-00-00-abcd-6h-settings.md")
	facts, err := imp.ImportSession(importer.SessionRef{
		Tool:       "codex-memory",
		SessionID:  "codex-memory:extensions/skysight/resources/resource.md",
		Path:       path,
		ModifiedAt: time.Date(2026, 8, 28, 10, 0, 0, 0, time.UTC),
		Metadata:   map[string]string{"sync_exclude": "true"},
	})
	if err != nil {
		t.Fatal(err)
	}
	if len(facts) != 1 {
		t.Fatalf("facts = %d, want 1", len(facts))
	}
	fact := facts[0]
	if !strings.Contains(fact.Content, "Memory summary") || strings.Contains(fact.Content, "title: ChatGPT") {
		t.Errorf("frontmatter leaked into body: %q", fact.Content)
	}
	if fact.Metadata["title"] != "ChatGPT settings and Appshots setup" {
		t.Errorf("title metadata = %q", fact.Metadata["title"])
	}
	if fact.Metadata["applications"] != "com.apple.UserNotificationCenter,com.openai.codex" {
		t.Errorf("applications metadata = %q", fact.Metadata["applications"])
	}
	if !strings.Contains(fact.Metadata["citations"], "segments/2026-08-28T10-00-00.md") {
		t.Errorf("citations metadata = %q", fact.Metadata["citations"])
	}
	if len(fact.Evidence) != 1 || fact.Evidence[0].EvidenceText != fact.Content {
		t.Fatalf("evidence = %#v, want one body span", fact.Evidence)
	}
}

func TestApplicationAllowDeny(t *testing.T) {
	root := fixtureRoot(t)
	allowed := NewWithOptions(Options{Root: root, ApplicationPolicy: ApplicationPolicy{Allowed: []string{"com.example.editor"}}})
	sessions, err := allowed.DiscoverSessions(time.Time{})
	if err != nil {
		t.Fatal(err)
	}
	if len(sessions) != 1 || !strings.Contains(sessions[0].Path, "10min-editor") {
		t.Fatalf("allow-list sessions = %#v, want editor resource only", sessions)
	}

	denied := NewWithOptions(Options{Root: root, ApplicationPolicy: ApplicationPolicy{Denied: []string{"com.openai.codex"}}})
	sessions, err = denied.DiscoverSessions(time.Time{})
	if err != nil {
		t.Fatal(err)
	}
	for _, session := range sessions {
		if strings.Contains(session.Path, "settings") {
			t.Fatalf("denied Codex application discovered %s", session.Path)
		}
	}
}

func TestMissingRootIsNoOp(t *testing.T) {
	imp := NewCodexMemoryImporter(filepath.Join(t.TempDir(), "missing"))
	sessions, err := imp.DiscoverSessions(time.Time{})
	if err != nil {
		t.Fatal(err)
	}
	if sessions != nil && len(sessions) != 0 {
		t.Fatalf("missing root returned %#v", sessions)
	}
}

func TestInstructionsAreNotImportedEvenWhenOpenedDirectly(t *testing.T) {
	imp := NewCodexMemoryImporter(fixtureRoot(t))
	path := filepath.Join(fixtureRoot(t), "extensions", "skysight", "instructions.md")
	info, err := os.Stat(path)
	if err != nil {
		t.Fatal(err)
	}
	facts, err := imp.ImportSession(importer.SessionRef{Path: path, ModifiedAt: info.ModTime()})
	if err != nil {
		t.Fatal(err)
	}
	if len(facts) != 0 {
		t.Fatal("instructions.md must not be imported even through a direct reference")
	}
}
