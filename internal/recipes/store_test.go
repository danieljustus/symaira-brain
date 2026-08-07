package recipes

import (
	"os"
	"path/filepath"
	"testing"
)

func TestStore_AppendLoadRoundTrip(t *testing.T) {
	path := filepath.Join(t.TempDir(), "p.jsonl")
	store := NewStore(path)

	ep := episode("p", []Step{step("memory", "memory_search"), step("vault", "request_credential")})
	if err := store.Append(ep); err != nil {
		t.Fatalf("Append: %v", err)
	}
	ep2 := episode("p", []Step{step("vault", "health")})
	if err := store.Append(ep2); err != nil {
		t.Fatalf("Append: %v", err)
	}

	got, err := store.Load()
	if err != nil {
		t.Fatalf("Load: %v", err)
	}
	if len(got) != 2 {
		t.Fatalf("Load() = %d episodes, want 2", len(got))
	}
	if got[0].Profile != "p" || len(got[0].Steps) != 2 {
		t.Errorf("episode 0 = %+v, want profile p with 2 steps", got[0])
	}
	if got[1].Steps[0].Tool != "health" {
		t.Errorf("episode 1 first tool = %q, want health", got[1].Steps[0].Tool)
	}
}

func TestStore_LoadMissingFileYieldsEmpty(t *testing.T) {
	store := NewStore(filepath.Join(t.TempDir(), "ghost.jsonl"))
	got, err := store.Load()
	if err != nil {
		t.Fatalf("Load: %v", err)
	}
	if len(got) != 0 {
		t.Errorf("Load() = %d episodes, want 0", len(got))
	}
}

func TestStore_SkipsMalformedLines(t *testing.T) {
	path := filepath.Join(t.TempDir(), "p.jsonl")
	// Two valid lines, one malformed in the middle.
	valid := `{"profile":"p","steps":[{"server":"vault","tool":"health"}],"started_at":"a","ended_at":"b"}` + "\n"
	if err := os.WriteFile(path, []byte(valid+"not json\n"+valid), 0o644); err != nil {
		t.Fatalf("WriteFile: %v", err)
	}

	store := NewStore(path)
	got, err := store.Load()
	if err != nil {
		t.Fatalf("Load: %v", err)
	}
	if len(got) != 2 {
		t.Errorf("Load() = %d episodes, want 2 (malformed line skipped)", len(got))
	}
}

func TestStore_PrunesBeyondMax(t *testing.T) {
	path := filepath.Join(t.TempDir(), "p.jsonl")
	store := NewStore(path)

	one := []Step{step("vault", "health"), step("memory", "memory_list")}
	for i := 0; i < maxEpisodesPerStore+50; i++ {
		if err := store.Append(episode("p", one)); err != nil {
			t.Fatalf("Append #%d: %v", i, err)
		}
	}

	got, err := store.Load()
	if err != nil {
		t.Fatalf("Load: %v", err)
	}
	if len(got) != maxEpisodesPerStore {
		t.Errorf("Load() = %d episodes, want %d (pruned)", len(got), maxEpisodesPerStore)
	}
}
