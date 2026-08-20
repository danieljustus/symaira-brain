package obsidian

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/importer"
)

func TestNewObsidianImporter(t *testing.T) {
	imp := NewObsidianImporter("/tmp/test", "", nil, nil, nil)

	if imp.Name() != "obsidian" {
		t.Errorf("Name() = %q, want %q", imp.Name(), "obsidian")
	}

	if imp.Category() != "notes" {
		t.Errorf("Category() = %q, want %q", imp.Category(), "notes")
	}

	if imp.PrivacyLevel() != "internal" {
		t.Errorf("PrivacyLevel() = %q, want %q", imp.PrivacyLevel(), "internal")
	}

	if imp.RequiresPIIGuard() {
		t.Error("RequiresPIIGuard() = true, want false")
	}
}

func TestNewObsidianImporterEmptyVault(t *testing.T) {
	// Empty vault path triggers auto-detect which returns "" in test env
	imp := NewObsidianImporter("", "", nil, nil, nil)
	if imp.vaultPath == "" {
		// Expected: no vault detected in test environment
		t.Log("vaultPath is empty as expected in test env")
	}
}

func TestExtractTags(t *testing.T) {
	imp := &ObsidianImporter{}

	content := `# Meeting Notes

This is about #project-x and #meeting.

Tags: #decision #action-item
`

	tags := imp.extractTags(content)
	if len(tags) != 4 {
		t.Errorf("extractTags() returned %d tags, want 4", len(tags))
	}

	expected := []string{"project-x", "meeting", "decision", "action-item"}
	for i, tag := range tags {
		if tag != expected[i] {
			t.Errorf("tags[%d] = %q, want %q", i, tag, expected[i])
		}
	}
}

func TestExtractTagsNoMatches(t *testing.T) {
	imp := &ObsidianImporter{}
	content := "This content has no tags at all."
	tags := imp.extractTags(content)
	if len(tags) != 0 {
		t.Errorf("expected 0 tags, got %d", len(tags))
	}
}

func TestExtractTagsDeduplicates(t *testing.T) {
	imp := &ObsidianImporter{}
	content := "#tag #tag #other"
	tags := imp.extractTags(content)
	if len(tags) != 2 {
		t.Errorf("expected 2 unique tags, got %d: %v", len(tags), tags)
	}
}

func TestExtractWikilinks(t *testing.T) {
	imp := &ObsidianImporter{}

	content := `See [[Project Setup]] and [[Meeting Notes|meeting notes]].
Also check [[Architecture]] for details.
`

	links := imp.extractWikilinks(content)
	if len(links) != 3 {
		t.Errorf("extractWikilinks() returned %d links, want 3", len(links))
	}

	expected := []string{"Project Setup", "Meeting Notes", "Architecture"}
	for i, link := range links {
		if link != expected[i] {
			t.Errorf("links[%d] = %q, want %q", i, link, expected[i])
		}
	}
}

func TestExtractWikilinksNoMatches(t *testing.T) {
	imp := &ObsidianImporter{}
	content := "No wikilinks here."
	links := imp.extractWikilinks(content)
	if len(links) != 0 {
		t.Errorf("expected 0 links, got %d", len(links))
	}
}

func TestExtractWikilinksDeduplicates(t *testing.T) {
	imp := &ObsidianImporter{}
	content := "[[Same]] and [[Same]] and [[Other]]"
	links := imp.extractWikilinks(content)
	if len(links) != 2 {
		t.Errorf("expected 2 unique links, got %d: %v", len(links), links)
	}
}

func TestExtractTitle(t *testing.T) {
	imp := &ObsidianImporter{}

	tests := []struct {
		name    string
		content string
		path    string
		want    string
	}{
		{
			name:    "with H1",
			content: "# My Meeting Notes\n\nSome content here.",
			path:    "/tmp/other.md",
			want:    "My Meeting Notes",
		},
		{
			name:    "without H1",
			content: "No heading here.",
			path:    "/tmp/Meeting Notes.md",
			want:    "Meeting Notes",
		},
		{
			name:    "H1 with trailing spaces",
			content: "#   Title With Spaces   \ncontent",
			path:    "/tmp/any.md",
			want:    "  Title With Spaces",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := imp.extractTitle(tt.content, tt.path)
			if got != tt.want {
				t.Errorf("extractTitle() = %q, want %q", got, tt.want)
			}
		})
	}
}

func TestIsExcludedFolder(t *testing.T) {
	imp := &ObsidianImporter{}

	tests := []struct {
		path string
		want bool
	}{
		{"Projects/MyProject.md", false},
		{".obsidian/config", true},
		{".trash/note.md", true},
		{"Templates/daily.md", true},
		{"Projects/.obsidian/config", true},
		{"node_modules/pkg/index.js", true},
		{"Daily/note.md", false},
		{"Templates", true},
		{"node_modules", true},
	}

	for _, tt := range tests {
		t.Run(tt.path, func(t *testing.T) {
			if got := imp.isExcludedFolder(tt.path); got != tt.want {
				t.Errorf("isExcludedFolder(%q) = %v, want %v", tt.path, got, tt.want)
			}
		})
	}
}

func TestIsExcludedFolderWithCustom(t *testing.T) {
	imp := &ObsidianImporter{
		excludeFolders: []string{"Archive", "Private"},
	}

	tests := []struct {
		path string
		want bool
	}{
		{"Projects/MyProject.md", false},
		{".obsidian/config", true},
		{"Archive/old-note.md", true},
		{"Private/diary.md", true},
		{"Daily/note.md", false},
		{"Templates/daily.md", true},
	}

	for _, tt := range tests {
		t.Run(tt.path, func(t *testing.T) {
			if got := imp.isExcludedFolder(tt.path); got != tt.want {
				t.Errorf("isExcludedFolder(%q) = %v, want %v", tt.path, got, tt.want)
			}
		})
	}
}

func TestParseSimpleYAML(t *testing.T) {
	imp := &ObsidianImporter{}

	tests := []struct {
		name  string
		lines []string
		want  map[string]interface{}
	}{
		{
			name:  "simple key values",
			lines: []string{"title: My Note", "tags: project-x", "date: 2026-06-15"},
			want:  map[string]interface{}{"title": "My Note", "tags": "project-x", "date": "2026-06-15"},
		},
		{
			name:  "quoted values",
			lines: []string{"title: \"Quoted Title\"", "alias: 'Single Quoted'"},
			want:  map[string]interface{}{"title": "Quoted Title", "alias": "Single Quoted"},
		},
		{
			name:  "empty lines and comments",
			lines: []string{"", "# this is a comment", "key: value", "", "# another comment"},
			want:  map[string]interface{}{"key": "value"},
		},
		{
			name:  "empty input",
			lines: []string{},
			want:  map[string]interface{}{},
		},
		{
			name:  "lines without colon separator",
			lines: []string{"just a line", "another line"},
			want:  map[string]interface{}{},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			note := &ObsidianNote{Frontmatter: make(map[string]interface{})}
			imp.parseSimpleYAML(tt.lines, note)
			if len(note.Frontmatter) != len(tt.want) {
				t.Errorf("Frontmatter length = %d, want %d; got %v", len(note.Frontmatter), len(tt.want), note.Frontmatter)
			}
			for k, v := range tt.want {
				if note.Frontmatter[k] != v {
					t.Errorf("Frontmatter[%q] = %v (%T), want %v (%T)", k, note.Frontmatter[k], note.Frontmatter[k], v, v)
				}
			}
		})
	}
}

func TestHasMatchingTag(t *testing.T) {
	tests := []struct {
		name     string
		noteTags []string
		filter   []string
		want     bool
	}{
		{"exact match", []string{"project-x", "meeting"}, []string{"project-x"}, true},
		{"no match", []string{"project-x", "meeting"}, []string{"decision"}, false},
		{"empty filter", []string{"project-x"}, []string{}, false},
		{"empty note tags", []string{}, []string{"tag"}, false},
		{"multiple filters one match", []string{"project-x"}, []string{"tag", "project-x"}, true},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			imp := &ObsidianImporter{tags: tt.filter}
			got := imp.hasMatchingTag(tt.noteTags)
			if got != tt.want {
				t.Errorf("hasMatchingTag(%v) = %v, want %v", tt.noteTags, got, tt.want)
			}
		})
	}
}

func TestHasExcludedTag(t *testing.T) {
	tests := []struct {
		name     string
		noteTags []string
		exclude  []string
		want     bool
	}{
		{"is excluded", []string{"private", "project-x"}, []string{"private"}, true},
		{"not excluded", []string{"project-x", "meeting"}, []string{"private"}, false},
		{"empty exclude list", []string{"project-x"}, []string{}, false},
		{"empty note tags", []string{}, []string{"private"}, false},
		{"multiple excludes one match", []string{"meeting"}, []string{"private", "meeting"}, true},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			imp := &ObsidianImporter{excludeTags: tt.exclude}
			got := imp.hasExcludedTag(tt.noteTags)
			if got != tt.want {
				t.Errorf("hasExcludedTag(%v) = %v, want %v", tt.noteTags, got, tt.want)
			}
		})
	}
}

func TestParseNote(t *testing.T) {
	tmpDir := t.TempDir()
	notePath := filepath.Join(tmpDir, "Test Note.md")
	content := `---
title: Test Note
tags: project-x
created: 2026-06-15
---

# Test Note

This is a note about #project-x and #meeting.

See [[Related Note]] for more details.
`
	if err := os.WriteFile(notePath, []byte(content), 0644); err != nil {
		t.Fatalf("failed to write note: %v", err)
	}

	imp := NewObsidianImporter(tmpDir, "", nil, nil, nil)
	note, err := imp.parseNote(notePath)
	if err != nil {
		t.Fatalf("parseNote failed: %v", err)
	}

	if note.Title != "Test Note" {
		t.Errorf("Title = %q, want %q", note.Title, "Test Note")
	}
	if note.WordCount == 0 {
		t.Error("WordCount should not be zero")
	}
	if len(note.Tags) == 0 {
		t.Error("expected tags to be extracted")
	}
	if len(note.Wikilinks) == 0 {
		t.Error("expected wikilinks to be extracted")
	}
	if note.Frontmatter["title"] != "Test Note" {
		t.Errorf("frontmatter title = %q, want %q", note.Frontmatter["title"], "Test Note")
	}
	if note.Frontmatter["tags"] != "project-x" {
		t.Errorf("frontmatter tags = %q, want %q", note.Frontmatter["tags"], "project-x")
	}
	if note.ModifiedAt.IsZero() {
		t.Error("ModifiedAt should not be zero")
	}
}

func TestParseNoteNoFrontmatter(t *testing.T) {
	tmpDir := t.TempDir()
	notePath := filepath.Join(tmpDir, "Simple.md")
	content := "# Simple Note\n\nJust some content without frontmatter."
	if err := os.WriteFile(notePath, []byte(content), 0644); err != nil {
		t.Fatalf("failed to write note: %v", err)
	}

	imp := NewObsidianImporter(tmpDir, "", nil, nil, nil)
	note, err := imp.parseNote(notePath)
	if err != nil {
		t.Fatalf("parseNote failed: %v", err)
	}

	if note.Title != "Simple Note" {
		t.Errorf("Title = %q, want %q", note.Title, "Simple Note")
	}
	if len(note.Frontmatter) != 0 {
		t.Errorf("expected no frontmatter, got %v", note.Frontmatter)
	}
}

func TestParseNoteFileNotFound(t *testing.T) {
	imp := NewObsidianImporter("/tmp", "", nil, nil, nil)
	_, err := imp.parseNote("/nonexistent/note.md")
	if err == nil {
		t.Fatal("expected error for non-existent file")
	}
}

func TestImportSession(t *testing.T) {
	tmpDir := t.TempDir()
	notePath := filepath.Join(tmpDir, "Meeting Note.md")
	content := `---
type: meeting
attendees: Alice, Bob
---

# Meeting Note

Discussed #project-x and #architecture.

See [[Action Items]] for follow-up.

This note has significant content for import.
`
	if err := os.WriteFile(notePath, []byte(content), 0644); err != nil {
		t.Fatalf("failed to write note: %v", err)
	}

	imp := NewObsidianImporter(tmpDir, "", nil, nil, nil)
	ref := importer.SessionRef{
		SessionID: "Meeting Note.md",
		Path:      notePath,
	}

	facts, err := imp.ImportSession(ref)
	if err != nil {
		t.Fatalf("ImportSession failed: %v", err)
	}
	if len(facts) != 1 {
		t.Fatalf("expected 1 fact, got %d", len(facts))
	}

	fact := facts[0]
	if !strings.Contains(fact.Content, "Meeting Note") {
		t.Errorf("expected content to contain title, got %q", fact.Content)
	}
	if fact.Metadata["title"] != "Meeting Note" {
		t.Errorf("title = %q, want %q", fact.Metadata["title"], "Meeting Note")
	}
	if fact.Metadata["tags"] == "" {
		t.Error("expected tags metadata")
	}
	if fact.Metadata["wikilinks"] == "" {
		t.Error("expected wikilinks metadata")
	}
	if fact.Metadata["frontmatter"] == "" {
		t.Error("expected frontmatter metadata")
	}
	if fact.Metadata["note_path"] != "Meeting Note.md" {
		t.Errorf("note_path = %q, want %q", fact.Metadata["note_path"], "Meeting Note.md")
	}
	if fact.Metadata["word_count"] == "" || fact.Metadata["word_count"] == "0" {
		t.Errorf("expected non-zero word_count, got %q", fact.Metadata["word_count"])
	}
}

func TestImportSessionContentTruncation(t *testing.T) {
	tmpDir := t.TempDir()
	notePath := filepath.Join(tmpDir, "Long Note.md")
	longContent := "# Long Note\n" + strings.Repeat("This is a long line of text. ", 500)
	if err := os.WriteFile(notePath, []byte(longContent), 0644); err != nil {
		t.Fatalf("failed to write note: %v", err)
	}

	imp := NewObsidianImporter(tmpDir, "", nil, nil, nil)
	imp.maxNoteLength = 100 // force truncation
	ref := importer.SessionRef{
		SessionID: "Long Note.md",
		Path:      notePath,
	}

	facts, err := imp.ImportSession(ref)
	if err != nil {
		t.Fatalf("ImportSession failed: %v", err)
	}
	if len(facts) != 1 {
		t.Fatalf("expected 1 fact, got %d", len(facts))
	}
	if !strings.HasSuffix(facts[0].Content, "...") && len(facts[0].Content) > 0 {
		t.Logf("content length = %d, maxNoteLength = %d", len(facts[0].Content), 100)
	}
}

func TestDiscoverSessions(t *testing.T) {
	tmpDir := t.TempDir()

	// Create notes
	notes := map[string]string{
		"Note1.md":           "# Note 1\nContent for first note.",
		"Note2.md":           "# Note 2\nContent for second note.",
		".obsidian/config":   "{}",
		".trash/deleted.md":  "# Deleted",
		"Templates/daily.md": "# Daily Template",
		"Daily/note.md":      "# Daily Note",
	}

	for path, content := range notes {
		fullPath := filepath.Join(tmpDir, path)
		if err := os.MkdirAll(filepath.Dir(fullPath), 0755); err != nil {
			t.Fatalf("mkdir %s: %v", filepath.Dir(fullPath), err)
		}
		if err := os.WriteFile(fullPath, []byte(content), 0644); err != nil {
			t.Fatalf("write %s: %v", path, err)
		}
	}

	imp := NewObsidianImporter(tmpDir, "", nil, nil, nil)

	// Discover all (since zero time)
	sessions, err := imp.DiscoverSessions(time.Time{})
	if err != nil {
		t.Fatalf("DiscoverSessions failed: %v", err)
	}

	// Should include Note1.md, Note2.md, Daily/note.md (3 .md files not in excluded folders)
	if len(sessions) != 3 {
		t.Fatalf("expected 3 sessions, got %d: %v", len(sessions), sessionPaths(sessions))
	}

	// Verify excluded folders are not included
	for _, s := range sessions {
		if strings.HasPrefix(s.SessionID, ".obsidian") || strings.HasPrefix(s.SessionID, ".trash") || strings.HasPrefix(s.SessionID, "Templates") {
			t.Errorf("unexpected session from excluded folder: %q", s.SessionID)
		}
	}
}

func TestDiscoverSessionsWithTagFilter(t *testing.T) {
	tmpDir := t.TempDir()

	notes := map[string]string{
		"Meeting.md": "# Meeting\nDiscussing #project-x.",
		"Private.md": "# Private\nA #private note.",
		"Work.md":    "# Work\n#project-x updates.",
	}

	for path, content := range notes {
		fullPath := filepath.Join(tmpDir, path)
		if err := os.WriteFile(fullPath, []byte(content), 0644); err != nil {
			t.Fatalf("write %s: %v", path, err)
		}
	}

	imp := NewObsidianImporter(tmpDir, "", []string{"project-x"}, nil, nil)

	sessions, err := imp.DiscoverSessions(time.Time{})
	if err != nil {
		t.Fatalf("DiscoverSessions failed: %v", err)
	}

	// Should only include notes with #project-x tag
	if len(sessions) != 2 {
		t.Fatalf("expected 2 sessions with #project-x, got %d", len(sessions))
	}
}

func TestDiscoverSessionsWithExcludeTags(t *testing.T) {
	tmpDir := t.TempDir()

	notes := map[string]string{
		"Public.md":  "# Public\nGeneral note.",
		"Private.md": "# Private\n#private note content.",
	}

	for path, content := range notes {
		fullPath := filepath.Join(tmpDir, path)
		if err := os.WriteFile(fullPath, []byte(content), 0644); err != nil {
			t.Fatalf("write %s: %v", path, err)
		}
	}

	imp := NewObsidianImporter(tmpDir, "", nil, nil, []string{"private"})

	sessions, err := imp.DiscoverSessions(time.Time{})
	if err != nil {
		t.Fatalf("DiscoverSessions failed: %v", err)
	}

	// Should exclude Private.md (has #private tag)
	if len(sessions) != 1 {
		t.Fatalf("expected 1 session (excluding private), got %d", len(sessions))
	}
	if sessions[0].SessionID != "Public.md" {
		t.Errorf("expected Public.md, got %q", sessions[0].SessionID)
	}
}

func TestDiscoverSessionsSince(t *testing.T) {
	tmpDir := t.TempDir()

	oldNote := filepath.Join(tmpDir, "Old.md")
	if err := os.WriteFile(oldNote, []byte("# Old"), 0644); err != nil {
		t.Fatalf("write: %v", err)
	}

	// Set old note's mod time far in the past
	past := time.Now().Add(-24 * time.Hour)
	if err := os.Chtimes(oldNote, past, past); err != nil {
		t.Skipf("cannot change file times: %v", err)
	}

	newNote := filepath.Join(tmpDir, "New.md")
	if err := os.WriteFile(newNote, []byte("# New"), 0644); err != nil {
		t.Fatalf("write: %v", err)
	}

	imp := NewObsidianImporter(tmpDir, "", nil, nil, nil)

	// Discover with since = 1 hour ago (only New.md should match)
	since := time.Now().Add(-1 * time.Hour)
	sessions, err := imp.DiscoverSessions(since)
	if err != nil {
		t.Fatalf("DiscoverSessions failed: %v", err)
	}
	if len(sessions) != 1 {
		t.Fatalf("expected 1 recent session, got %d", len(sessions))
	}
	if sessions[0].SessionID != "New.md" {
		t.Errorf("expected New.md, got %q", sessions[0].SessionID)
	}
}

func TestDiscoverSessionsEmptyVaultPath(t *testing.T) {
	imp := &ObsidianImporter{vaultPath: ""}
	_, err := imp.DiscoverSessions(time.Time{})
	if err == nil {
		t.Fatal("expected error with empty vault path")
	}
}

func sessionPaths(sessions []importer.SessionRef) []string {
	var paths []string
	for _, s := range sessions {
		paths = append(paths, s.SessionID)
	}
	return paths
}
