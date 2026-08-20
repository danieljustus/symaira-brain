package db

import (
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
)

func TestTemporalFields_RoundTrip(t *testing.T) {
	cfg := config.Defaults()
	cfg.Database.Path = t.TempDir() + "/test.db"
	database, err := Open(cfg)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = database.Close() }()

	now := time.Now().UTC()
	m := &Memory{
		ID:        "temporal-test",
		Content:   "User lives in Berlin",
		Scope:     "global",
		Metadata:  map[string]string{},
		ValidFrom: &now,
	}
	if err := database.SaveMemory(m); err != nil {
		t.Fatal(err)
	}

	got, err := database.GetMemory("temporal-test")
	if err != nil {
		t.Fatal(err)
	}
	if got.ValidFrom == nil {
		t.Fatal("expected valid_from to be set")
	}
	if got.ValidTo != nil {
		t.Error("expected valid_to to be nil for current fact")
	}
	if got.SupersededBy != "" {
		t.Error("expected superseded_by to be empty")
	}
}

func TestSupersedeFact(t *testing.T) {
	cfg := config.Defaults()
	cfg.Database.Path = t.TempDir() + "/test.db"
	database, err := Open(cfg)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = database.Close() }()

	old := &Memory{
		ID:       "old-fact",
		Content:  "User lives in Berlin",
		Scope:    "global",
		Metadata: map[string]string{},
	}
	if err := database.SaveMemory(old); err != nil {
		t.Fatal(err)
	}

	new_ := &Memory{
		ID:       "new-fact",
		Content:  "User lives in Munich",
		Scope:    "global",
		Metadata: map[string]string{},
	}
	if err := database.SaveMemory(new_); err != nil {
		t.Fatal(err)
	}

	if err := database.SupersedeFact("old-fact", "new-fact"); err != nil {
		t.Fatal(err)
	}

	superseded, err := database.GetMemory("old-fact")
	if err != nil {
		t.Fatal(err)
	}
	if superseded.SupersededBy != "new-fact" {
		t.Errorf("expected superseded_by='new-fact', got %q", superseded.SupersededBy)
	}
	if superseded.ValidTo == nil {
		t.Error("expected valid_to to be set on superseded fact")
	}

	history, err := database.GetSupersededHistory("new-fact")
	if err != nil {
		t.Fatal(err)
	}
	if len(history) != 1 {
		t.Fatalf("expected 1 superseded history entry, got %d", len(history))
	}
	if history[0].ID != "old-fact" {
		t.Errorf("expected history entry to be 'old-fact', got %q", history[0].ID)
	}
}

func TestListMemoriesAsOf_ThreeVersionChain(t *testing.T) {
	cfg := config.Defaults()
	cfg.Database.Path = t.TempDir() + "/test.db"
	database, err := Open(cfg)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = database.Close() }()

	t0 := time.Date(2026, 1, 1, 0, 0, 0, 0, time.UTC)
	t1 := t0.Add(1 * time.Hour)
	t2 := t0.Add(2 * time.Hour)

	m1 := &Memory{ID: "m1", Content: "v1", Scope: "global", Metadata: map[string]string{}, ValidFrom: &t0, ValidTo: &t1, SupersededBy: "m2"}
	m2 := &Memory{ID: "m2", Content: "v2", Scope: "global", Metadata: map[string]string{}, ValidFrom: &t1, ValidTo: &t2, SupersededBy: "m3"}
	m3 := &Memory{ID: "m3", Content: "v3", Scope: "global", Metadata: map[string]string{}, ValidFrom: &t2}

	for _, m := range []*Memory{m1, m2, m3} {
		if err := database.SaveMemory(m); err != nil {
			t.Fatalf("save %s: %v", m.ID, err)
		}
	}

	cases := []struct {
		name   string
		asOf   time.Time
		wantID string
	}{
		{"within v1 window", t0.Add(30 * time.Minute), "m1"},
		{"within v2 window", t1.Add(30 * time.Minute), "m2"},
		{"within v3 window (current, no valid_to)", t2.Add(30 * time.Minute), "m3"},
		{"before v1 valid_from", t0.Add(-1 * time.Hour), ""},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			got, err := database.ListMemoriesAsOf("global", tc.asOf, 0, 10)
			if err != nil {
				t.Fatalf("ListMemoriesAsOf: %v", err)
			}
			if tc.wantID == "" {
				if len(got) != 0 {
					t.Fatalf("expected 0 memories, got %d", len(got))
				}
				return
			}
			if len(got) != 1 {
				t.Fatalf("expected exactly 1 memory valid at %v, got %d", tc.asOf, len(got))
			}
			if got[0].ID != tc.wantID {
				t.Errorf("expected %s, got %s", tc.wantID, got[0].ID)
			}
		})
	}
}

func TestListMemoriesAsOf_DefaultMatchesListMemories(t *testing.T) {
	cfg := config.Defaults()
	cfg.Database.Path = t.TempDir() + "/test.db"
	database, err := Open(cfg)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = database.Close() }()

	for _, m := range []*Memory{
		{ID: "a", Content: "alpha", Scope: "global", Metadata: map[string]string{}},
		{ID: "b", Content: "beta", Scope: "global", Metadata: map[string]string{}},
	} {
		if err := database.SaveMemory(m); err != nil {
			t.Fatalf("save %s: %v", m.ID, err)
		}
	}

	want, err := database.ListMemories("global", 0, 10)
	if err != nil {
		t.Fatalf("ListMemories: %v", err)
	}
	got, err := database.ListMemoriesAsOf("global", time.Now().UTC().Add(time.Hour), 0, 10)
	if err != nil {
		t.Fatalf("ListMemoriesAsOf: %v", err)
	}
	if len(got) != len(want) {
		t.Fatalf("expected %d memories, got %d", len(want), len(got))
	}
	for i := range want {
		if got[i].ID != want[i].ID || got[i].Content != want[i].Content {
			t.Errorf("entry %d differs: want %+v got %+v", i, want[i], got[i])
		}
	}
}

func TestTemporalFields_NilDefaults(t *testing.T) {
	cfg := config.Defaults()
	cfg.Database.Path = t.TempDir() + "/test.db"
	database, err := Open(cfg)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = database.Close() }()

	m := &Memory{
		ID:       "no-temporal",
		Content:  "A simple fact",
		Scope:    "global",
		Metadata: map[string]string{},
	}
	if err := database.SaveMemory(m); err != nil {
		t.Fatal(err)
	}

	got, err := database.GetMemory("no-temporal")
	if err != nil {
		t.Fatal(err)
	}
	if got.ValidFrom == nil {
		t.Error("expected valid_from to be auto-set on save")
	}
}
