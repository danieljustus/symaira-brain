package tui

import (
	"os"
	"strings"
	"testing"

	tea "github.com/charmbracelet/bubbletea"
	"github.com/danieljustus/symaira-brain/internal/memory/config"
	"github.com/danieljustus/symaira-brain/internal/memory/db"
)

// helperDB creates a temporary in-memory-like database for testing.
func helperDB(t *testing.T) *db.DB {
	t.Helper()
	tempDir, err := os.MkdirTemp("", "symmemory-tui-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	t.Cleanup(func() { os.RemoveAll(tempDir) })

	oldHome := os.Getenv("HOME")
	os.Setenv("HOME", tempDir)
	t.Cleanup(func() { os.Setenv("HOME", oldHome) })

	database, err := db.Open(config.Defaults())
	if err != nil {
		t.Fatalf("failed to open test database: %v", err)
	}
	t.Cleanup(func() { _ = database.Close() })
	return database
}

// --------------------------------------------------------------------------
// Model Init
// --------------------------------------------------------------------------

func newTestModel(database *db.DB) model {
	return InitialModel(database, ":memory:", "http://localhost:11434/api/embeddings", "nomic-embed-text", 0, false)
}

func TestInitialModel(t *testing.T) {
	database := helperDB(t)
	m := newTestModel(database)

	if m.db != database {
		t.Error("expected model.db to be set")
	}
	if len(m.memories) != 0 {
		t.Errorf("expected empty memories slice, got %d items", len(m.memories))
	}
	if m.scope != "" {
		t.Errorf("expected empty scope, got %q", m.scope)
	}
}

func TestModelInit(t *testing.T) {
	database := helperDB(t)
	m := newTestModel(database)
	cmd := m.Init()
	if cmd != nil {
		t.Error("expected Init() to return nil command")
	}
}

// --------------------------------------------------------------------------
// Model Update: navigation keys
// --------------------------------------------------------------------------

func TestModelUpdateQuit(t *testing.T) {
	database := helperDB(t)
	m := newTestModel(database)

	updated, cmd := m.Update(tea.KeyMsg{Type: tea.KeyRunes, Runes: []rune("q")})
	if cmd == nil {
		t.Error("expected quit command on 'q'")
	}
	_ = updated
}

func TestModelUpdateNavigationEmpty(t *testing.T) {
	database := helperDB(t)
	m := newTestModel(database)

	// Navigating down on empty list should not crash
	updated, _ := m.Update(tea.KeyMsg{Type: tea.KeyDown})
	_ = updated

	// Navigating up on empty list should not crash
	updated2, _ := m.Update(tea.KeyMsg{Type: tea.KeyUp})
	_ = updated2
}

func TestModelUpdateNavigationWithMemories(t *testing.T) {
	database := helperDB(t)
	// Seed some test memories
	_ = database.SaveMemory(&db.Memory{
		ID:        "tui-mem-1",
		Content:   "First memory",
		Scope:     "global",
		Embedding: testEmbedding(1.0),
	})
	_ = database.SaveMemory(&db.Memory{
		ID:        "tui-mem-2",
		Content:   "Second memory",
		Scope:     "global",
		Embedding: testEmbedding(2.0),
	})

	m := newTestModel(database)

	if len(m.memories) != 2 {
		t.Fatalf("expected 2 memories, got %d", len(m.memories))
	}
	if m.selected != 0 {
		t.Errorf("expected selected=0, got %d", m.selected)
	}

	// Move down
	updated, _ := m.Update(tea.KeyMsg{Type: tea.KeyDown})
	um, ok := updated.(model)
	if !ok {
		t.Fatal("expected model type")
	}
	if um.selected != 1 {
		t.Errorf("expected selected=1 after down, got %d", um.selected)
	}

	// Move down at boundary (should not wrap)
	updated2, _ := um.Update(tea.KeyMsg{Type: tea.KeyDown})
	um2, _ := updated2.(model)
	if um2.selected != 1 {
		t.Errorf("expected selected=1 at bottom, got %d", um2.selected)
	}

	// Move up
	updated3, _ := um.Update(tea.KeyMsg{Type: tea.KeyUp})
	um3, _ := updated3.(model)
	if um3.selected != 0 {
		t.Errorf("expected selected=0 after up, got %d", um3.selected)
	}

	// Move up at boundary (should not wrap)
	updated4, _ := um3.Update(tea.KeyMsg{Type: tea.KeyUp})
	um4, _ := updated4.(model)
	if um4.selected != 0 {
		t.Errorf("expected selected=0 at top, got %d", um4.selected)
	}
}

// --------------------------------------------------------------------------
// Model Update: scope filters
// --------------------------------------------------------------------------

func TestModelUpdateScopeFilters(t *testing.T) {
	database := helperDB(t)
	m := newTestModel(database)

	tests := []struct {
		key           string
		expectedScope string
	}{
		{"g", "global"},
		{"p", "project"},
		{"a", "agent"},
		{"u", "user"},
		{"s", "session"},
		{"*", ""},
		{"c", ""},
	}

	for _, tt := range tests {
		t.Run("key_"+tt.key, func(t *testing.T) {
			updated, _ := m.Update(tea.KeyMsg{Type: tea.KeyRunes, Runes: []rune(tt.key)})
			um, ok := updated.(model)
			if !ok {
				t.Fatal("expected model type")
			}
			if um.scope != tt.expectedScope {
				t.Errorf("after key %q: expected scope %q, got %q", tt.key, tt.expectedScope, um.scope)
			}
		})
	}
}

// --------------------------------------------------------------------------
// Model Update: search mode
// --------------------------------------------------------------------------

func TestModelUpdateSearchMode(t *testing.T) {
	database := helperDB(t)
	m := newTestModel(database)

	// Enter search mode
	updated, _ := m.Update(tea.KeyMsg{Type: tea.KeyRunes, Runes: []rune("/")})
	um, ok := updated.(model)
	if !ok {
		t.Fatal("expected model type")
	}
	if !um.searching {
		t.Error("expected searching=true after '/'")
	}

	// Type a character in search mode
	updated2, _ := um.Update(tea.KeyMsg{Type: tea.KeyRunes, Runes: []rune("t")})
	um2, _ := updated2.(model)
	if um2.search != "t" {
		t.Errorf("expected search='t', got %q", um2.search)
	}

	// Escape exits search and clears search
	updated3, _ := um2.Update(tea.KeyMsg{Type: tea.KeyEscape})
	um3, _ := updated3.(model)
	if um3.searching {
		t.Error("expected searching=false after escape")
	}
	if um3.search != "" {
		t.Errorf("expected search='' after escape, got %q", um3.search)
	}
}

func TestModelUpdateSearchBackspace(t *testing.T) {
	database := helperDB(t)
	m := newTestModel(database)

	// Enter search mode and type 'ab'
	updated, _ := m.Update(tea.KeyMsg{Type: tea.KeyRunes, Runes: []rune("/")})
	um, _ := updated.(model)
	ua, _ := um.Update(tea.KeyMsg{Type: tea.KeyRunes, Runes: []rune("a")})
	um, _ = ua.(model)
	ub, _ := um.Update(tea.KeyMsg{Type: tea.KeyRunes, Runes: []rune("b")})
	um, _ = ub.(model)

	// Backspace
	updated2, _ := um.Update(tea.KeyMsg{Type: tea.KeyBackspace})
	um2, _ := updated2.(model)
	if um2.search != "a" {
		t.Errorf("expected search='a' after backspace, got %q", um2.search)
	}
}

// --------------------------------------------------------------------------
// Model Update: delete
// --------------------------------------------------------------------------

func TestModelUpdateDeleteMemory(t *testing.T) {
	database := helperDB(t)
	// Seed a memory
	_ = database.SaveMemory(&db.Memory{
		ID:        "delete-me",
		Content:   "Delete me",
		Scope:     "global",
		Embedding: testEmbedding(1.0),
	})

	m := newTestModel(database)
	if len(m.memories) != 1 {
		t.Fatalf("expected 1 memory before delete, got %d", len(m.memories))
	}

	// Press 'd' to delete selected memory
	updated, _ := m.Update(tea.KeyMsg{Type: tea.KeyRunes, Runes: []rune("d")})
	um, ok := updated.(model)
	if !ok {
		t.Fatal("expected model type")
	}
	if len(um.memories) != 0 {
		t.Errorf("expected 0 memories after delete, got %d", len(um.memories))
	}
}

// --------------------------------------------------------------------------
// Model View: output rendering
// --------------------------------------------------------------------------

func TestModelView(t *testing.T) {
	database := helperDB(t)
	m := newTestModel(database)

	view := m.View()
	if !strings.Contains(view, "SYMAIRA MEMORY") {
		t.Error("expected view to contain title")
	}
	if !strings.Contains(view, "ALL SCOPES") {
		t.Error("expected view to show 'ALL SCOPES' filter")
	}
	if !strings.Contains(view, "Controls:") {
		t.Error("expected view to contain keyboard controls")
	}
}

func TestModelViewWithMemories(t *testing.T) {
	database := helperDB(t)
	_ = database.SaveMemory(&db.Memory{
		ID:        "view-1",
		Content:   "Test memory view",
		Scope:     "global",
		Embedding: testEmbedding(0.5),
	})

	m := newTestModel(database)
	view := m.View()

	if !strings.Contains(view, "Test memory view") {
		t.Error("expected view to contain memory content")
	}
	if !strings.Contains(view, "GLOBAL") {
		t.Error("expected view to show scope badge")
	}
	if !strings.Contains(view, "Total Memories: 1") {
		t.Error("expected view to show memory count")
	}
}

func TestModelViewWithScopeFilter(t *testing.T) {
	database := helperDB(t)
	m := newTestModel(database)
	m.scope = "project"
	m.loadMemories()

	view := m.View()
	if strings.Contains(view, "ALL SCOPES") {
		t.Error("expected view NOT to show 'ALL SCOPES' when filtered")
	}
	if !strings.Contains(view, "PROJECT") {
		t.Error("expected view to show 'PROJECT' filter")
	}
}

func TestModelViewEmptyState(t *testing.T) {
	database := helperDB(t)
	m := newTestModel(database)
	view := m.View()

	if !strings.Contains(view, "No memories match") {
		t.Error("expected view to show empty state message")
	}
}

// --------------------------------------------------------------------------
// Model View: search indicator
// --------------------------------------------------------------------------

func TestModelViewSearchIndicator(t *testing.T) {
	database := helperDB(t)
	m := newTestModel(database)

	// Active search shows search indicator
	m.search = "test-query"
	view := m.View()
	if !strings.Contains(view, "Active Search: test-query") {
		t.Error("expected view to show active search indicator")
	}

	// Searching mode shows different indicator
	m.searching = true
	view = m.View()
	if !strings.Contains(view, "Search Keyword: test-query") {
		t.Error("expected view to show search keyword prompt")
	}
}

// testEmbedding builds a valid db.EmbeddingDim-sized vector with the given
// leading values (dimension mismatches are now a handled error).
func testEmbedding(leading ...float32) []float32 {
	v := make([]float32, db.EmbeddingDim)
	copy(v, leading)
	return v
}
