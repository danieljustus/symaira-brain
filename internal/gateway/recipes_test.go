package gateway

import (
	"context"
	"encoding/json"
	"log/slog"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/broker"
	"github.com/danieljustus/symaira-brain/internal/config"
	"github.com/danieljustus/symaira-brain/internal/recipes"
)

// withDataHome points XDG_DATA_HOME at a fresh temp directory so the
// episode store never touches the real user data dir.
func withDataHome(t *testing.T) string {
	t.Helper()
	dir := t.TempDir()
	t.Setenv("XDG_DATA_HOME", dir)
	return dir
}

// recipesStorePath returns the episode file path under the given
// XDG_DATA_HOME for profile name.
func recipesStorePath(dataHome, profileName string) string {
	return filepath.Join(dataHome, "symbrain", "recipes", profileName+".jsonl")
}

// seedEpisodes writes the given episodes directly into the profile's
// store, as if previous sessions had flushed them.
func seedEpisodes(t *testing.T, dataHome, profileName string, eps []recipes.Episode) {
	t.Helper()
	store := recipes.NewStore(recipesStorePath(dataHome, profileName))
	for _, ep := range eps {
		if err := store.Append(ep); err != nil {
			t.Fatalf("seed Append: %v", err)
		}
	}
}

func recipesTestEpisodes() []recipes.Episode {
	seq := []recipes.Step{{Server: "vault", Tool: "request_credential"}, {Server: "memory", Tool: "memory_search"}}
	base := recipes.Episode{
		Profile:   "test",
		Steps:     seq,
		StartedAt: "2026-08-01T09:00:00Z",
		EndedAt:   "2026-08-01T09:05:00Z",
	}
	return []recipes.Episode{
		base,
		{Profile: "test", Steps: seq, StartedAt: "2026-08-02T09:00:00Z", EndedAt: "2026-08-02T09:05:00Z"},
		{Profile: "test", Steps: seq, StartedAt: "2026-08-03T09:00:00Z", EndedAt: "2026-08-03T09:05:00Z"},
	}
}

func TestRecipesTool_ReturnsPromoted(t *testing.T) {
	home := withDataHome(t)
	seedEpisodes(t, home, "test", recipesTestEpisodes())

	s := New(testProfile(), nil, slog.Default(), &config.Config{
		Recipes: config.RecipesConfig{Enabled: true, PromotionThreshold: 3},
	}, "dev")

	raw, err := s.handleRecipes(context.Background(), json.RawMessage(`{}`))
	if err != nil {
		t.Fatalf("handleRecipes: %v", err)
	}
	data, err := json.Marshal(raw)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	var resp struct {
		Profile   string           `json:"profile"`
		Threshold int              `json:"threshold"`
		Recipes   []recipes.Recipe `json:"recipes"`
	}
	if err := json.Unmarshal(data, &resp); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}

	if resp.Profile != "test" || resp.Threshold != 3 {
		t.Errorf("Profile/Threshold = %q/%d, want test/3", resp.Profile, resp.Threshold)
	}
	if len(resp.Recipes) != 1 {
		t.Fatalf("recipes = %d, want 1", len(resp.Recipes))
	}
	r := resp.Recipes[0]
	if r.Provenance.RecurrenceCount != 3 {
		t.Errorf("RecurrenceCount = %d, want 3", r.Provenance.RecurrenceCount)
	}
	if len(r.Steps) != 2 || r.Steps[0].Tool != "request_credential" {
		t.Errorf("Steps = %+v, want the recurring sequence", r.Steps)
	}
}

func TestRecipesTool_EmptyStore(t *testing.T) {
	withDataHome(t)
	s := New(testProfile(), nil, slog.Default(), &config.Config{
		Recipes: config.RecipesConfig{Enabled: true, PromotionThreshold: 3},
	}, "dev")

	raw, err := s.handleRecipes(context.Background(), json.RawMessage(`{}`))
	if err != nil {
		t.Fatalf("handleRecipes: %v", err)
	}
	data, _ := json.Marshal(raw)
	var resp struct {
		Recipes []recipes.Recipe `json:"recipes"`
	}
	if err := json.Unmarshal(data, &resp); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}
	if len(resp.Recipes) != 0 {
		t.Errorf("recipes = %d, want 0 for an empty store", len(resp.Recipes))
	}
}

// runOneSession drives one ServeIO connection over pipes, calling the
// given tools in order, and waits for the session to end so the episode
// flush (a deferred call) has completed.
func runOneSession(t *testing.T, cfg *config.Config, tools []string) {
	t.Helper()
	vault := newManagedFake(t, "vault",
		`[{"name":"get_entry","description":"fetch secret"},{"name":"health","description":"healthcheck"}]`)

	s := New(testProfile(), map[string]*broker.ManagedServer{"vault": vault}, slog.Default(), cfg, "dev")

	sr, sw, cr, cw := bidirectionalPipe(t)
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	done := make(chan struct{})
	go func() {
		_ = s.ServeIO(ctx, sr, sw)
		close(done)
	}()

	writeJSON(t, cw, initializeRequest(1))
	if resp := readJSONResponse(t, cr); resp.Error != nil {
		t.Fatalf("initialize error: %v", resp.Error)
	}
	for i, tool := range tools {
		writeJSON(t, cw, toolsCallRequest(float64(i+2), tool))
		if resp := readJSONResponse(t, cr); resp.Error != nil {
			t.Fatalf("tools/call %s error: %v", tool, resp.Error)
		}
	}
	cw.Close()
	select {
	case <-done:
	case <-time.After(10 * time.Second):
		t.Fatal("ServeIO did not return after client disconnect")
	}
}

func TestServeIO_RecordsAndPromotesEpisodes(t *testing.T) {
	home := withDataHome(t)
	cfg := &config.Config{Recipes: config.RecipesConfig{Enabled: true, PromotionThreshold: 2}}

	runOneSession(t, cfg, []string{"vault_health", "vault_get_entry"})
	runOneSession(t, cfg, []string{"vault_health", "vault_get_entry"})

	// The store now has two identical episodes.
	store := recipes.NewStore(recipesStorePath(home, "test"))
	eps, err := store.Load()
	if err != nil {
		t.Fatalf("store.Load: %v", err)
	}
	if len(eps) != 2 {
		t.Fatalf("store has %d episodes, want 2", len(eps))
	}
	for _, ep := range eps {
		if len(ep.Steps) != 2 || ep.Steps[0].Tool != "health" || ep.Steps[1].Tool != "get_entry" {
			t.Errorf("episode steps = %+v, want health, get_entry", ep.Steps)
		}
	}

	// With threshold 2 the sequence is promoted and exposed.
	s := New(testProfile(), nil, slog.Default(), cfg, "dev")
	raw, err := s.handleRecipes(context.Background(), json.RawMessage(`{}`))
	if err != nil {
		t.Fatalf("handleRecipes: %v", err)
	}
	data, _ := json.Marshal(raw)
	var resp struct {
		Recipes []recipes.Recipe `json:"recipes"`
	}
	if err := json.Unmarshal(data, &resp); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}
	if len(resp.Recipes) != 1 {
		t.Fatalf("recipes = %d, want 1 after 2 sessions at threshold 2", len(resp.Recipes))
	}
}

func TestServeIO_NoRecordingWhenDisabled(t *testing.T) {
	home := withDataHome(t)

	// No config attached (nil) — recording must be off.
	runOneSession(t, nil, []string{"vault_health"})
	if _, err := os.Stat(recipesStorePath(home, "test")); !os.IsNotExist(err) {
		t.Error("episode store created although recording is disabled (nil config)")
	}

	// Explicitly disabled.
	home2 := withDataHome(t)
	cfg := &config.Config{Recipes: config.RecipesConfig{Enabled: false, PromotionThreshold: 2}}
	runOneSession(t, cfg, []string{"vault_health"})
	if _, err := os.Stat(recipesStorePath(home2, "test")); !os.IsNotExist(err) {
		t.Error("episode store created although [recipes] enabled=false")
	}
}
