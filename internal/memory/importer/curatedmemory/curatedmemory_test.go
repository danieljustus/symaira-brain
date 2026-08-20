package curatedmemory

import (
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/importer"
)

func TestNewCuratedMemoryImporter(t *testing.T) {
	imp := NewCuratedMemoryImporter("")

	if imp.Name() != "curated-memory" {
		t.Errorf("Name() = %q, want %q", imp.Name(), "curated-memory")
	}
	if imp.Category() != "notes" {
		t.Errorf("Category() = %q, want %q", imp.Category(), "notes")
	}
	if imp.PrivacyLevel() != "confidential" {
		t.Errorf("PrivacyLevel() = %q, want %q", imp.PrivacyLevel(), "confidential")
	}
	if !imp.RequiresPIIGuard() {
		t.Error("RequiresPIIGuard() = false, want true")
	}
}

func TestDiscoverClaudeCodeMemoryFiles(t *testing.T) {
	testdata := filepath.Join("testdata", "claude-code")
	imp := NewCuratedMemoryImporter("")

	// Create a fake home so the importer uses testdata instead
	// We'll use the discover method directly with the testdata path
	sessions, err := imp.discoverClaudeCode(filepath.Join(testdata, "projects"), time.Time{})
	if err != nil {
		t.Fatalf("discoverClaudeCode() error: %v", err)
	}

	// Should find: my-project/memory/architecture.md, my-project/memory/preferences.md,
	// other-project/memory/notes.md
	// Should NOT find: my-project/MEMORY.md (index file)
	if len(sessions) != 3 {
		t.Fatalf("discoverClaudeCode() returned %d sessions, want 3", len(sessions))
	}

	// Check that MEMORY.md index is not included
	for _, s := range sessions {
		if filepath.Base(s.Path) == "MEMORY.md" {
			t.Errorf("MEMORY.md index file should be skipped, got session for %s", s.Path)
		}
	}

	// Verify session metadata
	found := make(map[string]bool)
	for _, s := range sessions {
		if s.Tool != "curated-memory" {
			t.Errorf("session Tool = %q, want %q", s.Tool, "curated-memory")
		}
		if s.Metadata["source_tool"] != "claude-code" {
			t.Errorf("session source_tool = %q, want %q", s.Metadata["source_tool"], "claude-code")
		}
		base := filepath.Base(s.Path)
		found[base] = true
	}

	if !found["architecture.md"] {
		t.Error("expected architecture.md in discovered sessions")
	}
	if !found["preferences.md"] {
		t.Error("expected preferences.md in discovered sessions")
	}
	if !found["notes.md"] {
		t.Error("expected notes.md in discovered sessions")
	}
}

func TestDiscoverHermesMemoryFiles(t *testing.T) {
	testdata := filepath.Join("testdata", "hermes")
	imp := NewCuratedMemoryImporter("")

	sessions, err := imp.discoverHermes(filepath.Join(testdata, "memories"), time.Time{})
	if err != nil {
		t.Fatalf("discoverHermes() error: %v", err)
	}

	// Should find MEMORY.md and USER.md
	if len(sessions) != 2 {
		t.Fatalf("discoverHermes() returned %d sessions, want 2", len(sessions))
	}

	found := make(map[string]bool)
	for _, s := range sessions {
		if s.Tool != "curated-memory" {
			t.Errorf("session Tool = %q, want %q", s.Tool, "curated-memory")
		}
		if s.Metadata["source_tool"] != "hermes" {
			t.Errorf("session source_tool = %q, want %q", s.Metadata["source_tool"], "hermes")
		}
		base := filepath.Base(s.Path)
		found[base] = true
	}

	if !found["MEMORY.md"] {
		t.Error("expected MEMORY.md in discovered sessions")
	}
	if !found["USER.md"] {
		t.Error("expected USER.md in discovered sessions")
	}
}

func TestDiscoverSkipsFilesOlderThanSince(t *testing.T) {
	testdata := filepath.Join("testdata", "claude-code")

	// Use a future time so all files are "old"
	future := time.Now().Add(24 * time.Hour)
	imp := NewCuratedMemoryImporter("")

	sessions, err := imp.discoverClaudeCode(filepath.Join(testdata, "projects"), future)
	if err != nil {
		t.Fatalf("discoverClaudeCode() error: %v", err)
	}

	if len(sessions) != 0 {
		t.Errorf("discoverClaudeCode() returned %d sessions with future since, want 0", len(sessions))
	}
}

func TestDiscoverNonexistentDirectory(t *testing.T) {
	imp := NewCuratedMemoryImporter("")

	sessions, err := imp.discoverClaudeCode("/nonexistent/path/to/projects", time.Time{})
	if err != nil {
		// filepath.Walk returns an error for nonexistent root
		t.Logf("discoverClaudeCode() error for nonexistent dir: %v (expected)", err)
	}
	if len(sessions) != 0 {
		t.Errorf("discoverClaudeCode() returned %d sessions for nonexistent dir, want 0", len(sessions))
	}

	sessions, err = imp.discoverHermes("/nonexistent/path/to/memories", time.Time{})
	if err != nil {
		t.Logf("discoverHermes() error for nonexistent dir: %v (expected)", err)
	}
	if len(sessions) != 0 {
		t.Errorf("discoverHermes() returned %d sessions for nonexistent dir, want 0", len(sessions))
	}
}

func TestParseFrontmatter(t *testing.T) {
	imp := NewCuratedMemoryImporter("")

	tests := []struct {
		name     string
		lines    []string
		wantKeys map[string]string
	}{
		{
			name:  "simple key-value",
			lines: []string{"name: Test Name", "description: A test", "type: preference"},
			wantKeys: map[string]string{
				"name":        "Test Name",
				"description": "A test",
				"type":        "preference",
			},
		},
		{
			name:  "quoted values",
			lines: []string{`name: "Quoted Name"`, `description: 'Single Quoted'`},
			wantKeys: map[string]string{
				"name":        "Quoted Name",
				"description": "Single Quoted",
			},
		},
		{
			name:  "skips comments and empty lines",
			lines: []string{"", "# This is a comment", "name: Real Value", ""},
			wantKeys: map[string]string{
				"name": "Real Value",
			},
		},
		{
			name:     "empty input",
			lines:    []string{},
			wantKeys: map[string]string{},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			memFile := &CuratedMemoryFile{
				Frontmatter: make(map[string]interface{}),
			}
			imp.parseSimpleYAML(tt.lines, memFile)

			if len(memFile.Frontmatter) != len(tt.wantKeys) {
				t.Errorf("parseSimpleYAML() produced %d keys, want %d", len(memFile.Frontmatter), len(tt.wantKeys))
			}

			for k, want := range tt.wantKeys {
				got, ok := memFile.Frontmatter[k]
				if !ok {
					t.Errorf("parseSimpleYAML() missing key %q", k)
					continue
				}
				if got.(string) != want {
					t.Errorf("parseSimpleYAML() key %q = %q, want %q", k, got, want)
				}
			}
		})
	}
}

func TestExtractLinks(t *testing.T) {
	imp := NewCuratedMemoryImporter("")

	tests := []struct {
		name    string
		content string
		want    []string
	}{
		{
			name:    "single link",
			content: "See [[Architecture]] for details.",
			want:    []string{"Architecture"},
		},
		{
			name:    "multiple links",
			content: "See [[Project Setup]] and [[Meeting Notes|meeting notes]].",
			want:    []string{"Project Setup", "Meeting Notes"},
		},
		{
			name:    "no links",
			content: "No links here.",
			want:    nil,
		},
		{
			name:    "duplicate links",
			content: "See [[Project]] and [[Project]] again.",
			want:    []string{"Project"},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := imp.extractLinks(tt.content)
			if len(got) != len(tt.want) {
				t.Errorf("extractLinks() returned %d links, want %d: %v", len(got), len(tt.want), got)
				return
			}
			for i, link := range got {
				if link != tt.want[i] {
					t.Errorf("extractLinks()[%d] = %q, want %q", i, link, tt.want[i])
				}
			}
		})
	}
}

func TestImportSessionWithFrontmatter(t *testing.T) {
	testdata, err := filepath.Abs(filepath.Join("testdata", "claude-code", "projects", "my-project", "memory", "architecture.md"))
	if err != nil {
		t.Fatal(err)
	}

	imp := NewCuratedMemoryImporter("")
	ref := importer.SessionRef{
		Tool:       "curated-memory",
		SessionID:  "claude-code:my-project/memory/architecture.md",
		Path:       testdata,
		ModifiedAt: time.Now(),
		Metadata: map[string]string{
			"source_tool": "claude-code",
			"project":     "my-project",
		},
	}

	facts, err := imp.ImportSession(ref)
	if err != nil {
		t.Fatalf("ImportSession() error: %v", err)
	}

	if len(facts) != 1 {
		t.Fatalf("ImportSession() returned %d facts, want 1", len(facts))
	}

	fact := facts[0]

	if fact.Source != "curated-memory" {
		t.Errorf("fact.Source = %q, want %q", fact.Source, "curated-memory")
	}

	// Check frontmatter was parsed into metadata
	if fact.Metadata["title"] != "Project Architecture Notes" {
		t.Errorf("metadata title = %q, want %q", fact.Metadata["title"], "Project Architecture Notes")
	}
	if fact.Metadata["description"] != "Key architectural decisions for the main project" {
		t.Errorf("metadata description = %q, want %q", fact.Metadata["description"], "Key architectural decisions for the main project")
	}
	if fact.Metadata["memory_type"] != "architecture" {
		t.Errorf("metadata memory_type = %q, want %q", fact.Metadata["memory_type"], "architecture")
	}
	if fact.Metadata["project"] != "my-project" {
		t.Errorf("metadata project = %q, want %q", fact.Metadata["project"], "my-project")
	}

	// Check frontmatter JSON is present
	if fact.Metadata["frontmatter"] == "" {
		t.Error("expected frontmatter JSON in metadata")
	}

	// Check links were extracted
	if fact.Metadata["links"] == "" {
		t.Error("expected links JSON in metadata")
	}

	// Verify content contains the memory text
	if len(fact.Content) == 0 {
		t.Error("expected non-empty content")
	}
}

func TestImportSessionWithoutFrontmatter(t *testing.T) {
	testdata, err := filepath.Abs(filepath.Join("testdata", "claude-code", "projects", "other-project", "memory", "notes.md"))
	if err != nil {
		t.Fatal(err)
	}

	imp := NewCuratedMemoryImporter("")
	ref := importer.SessionRef{
		Tool:       "curated-memory",
		SessionID:  "claude-code:other-project/memory/notes.md",
		Path:       testdata,
		ModifiedAt: time.Now(),
		Metadata: map[string]string{
			"source_tool": "claude-code",
			"project":     "other-project",
		},
	}

	facts, err := imp.ImportSession(ref)
	if err != nil {
		t.Fatalf("ImportSession() error: %v", err)
	}

	if len(facts) != 1 {
		t.Fatalf("ImportSession() returned %d facts, want 1", len(facts))
	}

	fact := facts[0]

	// No frontmatter, so no title/description/type
	if fact.Metadata["title"] != "" {
		t.Errorf("expected empty title, got %q", fact.Metadata["title"])
	}

	// Content should be present
	if len(fact.Content) == 0 {
		t.Error("expected non-empty content")
	}
}

func TestImportSessionHermes(t *testing.T) {
	testdata, err := filepath.Abs(filepath.Join("testdata", "hermes", "memories", "USER.md"))
	if err != nil {
		t.Fatal(err)
	}

	imp := NewCuratedMemoryImporter("")
	ref := importer.SessionRef{
		Tool:       "curated-memory",
		SessionID:  "hermes:USER.md",
		Path:       testdata,
		ModifiedAt: time.Now(),
		Metadata: map[string]string{
			"source_tool": "hermes",
		},
	}

	facts, err := imp.ImportSession(ref)
	if err != nil {
		t.Fatalf("ImportSession() error: %v", err)
	}

	if len(facts) != 1 {
		t.Fatalf("ImportSession() returned %d facts, want 1", len(facts))
	}

	fact := facts[0]
	if fact.Metadata["title"] != "User Profile" {
		t.Errorf("metadata title = %q, want %q", fact.Metadata["title"], "User Profile")
	}
	if fact.Metadata["memory_type"] != "user" {
		t.Errorf("metadata memory_type = %q, want %q", fact.Metadata["memory_type"], "user")
	}
}

func TestDiscoverSessionsIntegration(t *testing.T) {
	// Test the full DiscoverSessions flow using a fake home directory
	tmpDir, err := os.MkdirTemp("", "curatedmemory-test-*")
	if err != nil {
		t.Fatal(err)
	}
	defer os.RemoveAll(tmpDir)

	// Set up Claude Code structure
	claudeDir := filepath.Join(tmpDir, ".claude", "projects", "test-project", "memory")
	if err := os.MkdirAll(claudeDir, 0o755); err != nil {
		t.Fatal(err)
	}
	memFile := filepath.Join(claudeDir, "test-memory.md")
	if err := os.WriteFile(memFile, []byte("---\nname: Test\n---\nTest content"), 0o644); err != nil {
		t.Fatal(err)
	}
	// Create MEMORY.md index (should be skipped)
	indexFile := filepath.Join(tmpDir, ".claude", "projects", "test-project", "MEMORY.md")
	if err := os.WriteFile(indexFile, []byte("# Index\nThis should be skipped"), 0o644); err != nil {
		t.Fatal(err)
	}

	// Set up Hermes structure
	hermesDir := filepath.Join(tmpDir, ".hermes", "memories")
	if err := os.MkdirAll(hermesDir, 0o755); err != nil {
		t.Fatal(err)
	}
	hermesMem := filepath.Join(hermesDir, "MEMORY.md")
	if err := os.WriteFile(hermesMem, []byte("---\nname: Hermes Memory\n---\nHermes content"), 0o644); err != nil {
		t.Fatal(err)
	}

	imp := NewCuratedMemoryImporter(tmpDir)
	sessions, err := imp.DiscoverSessions(time.Time{})
	if err != nil {
		t.Fatalf("DiscoverSessions() error: %v", err)
	}

	// Should find 2 sessions: claude-code memory file + hermes MEMORY.md
	if len(sessions) != 2 {
		t.Fatalf("DiscoverSessions() returned %d sessions, want 2", len(sessions))
	}

	sources := make(map[string]bool)
	for _, s := range sessions {
		sources[s.Metadata["source_tool"]] = true
	}
	if !sources["claude-code"] {
		t.Error("expected claude-code session")
	}
	if !sources["hermes"] {
		t.Error("expected hermes session")
	}
}
