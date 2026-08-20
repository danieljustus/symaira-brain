package db

import (
	"math"
	"testing"
	"time"
)

func TestDefaultStalenessThreshold(t *testing.T) {
	th := DefaultStalenessThreshold()
	if th.MinAccessCount != 1 {
		t.Errorf("MinAccessCount = %d, want 1", th.MinAccessCount)
	}
	if th.MaxDaysSinceAccess != 90 {
		t.Errorf("MaxDaysSinceAccess = %v, want 90", th.MaxDaysSinceAccess)
	}
	if th.NeverAccessedPenalty != 0.1 {
		t.Errorf("NeverAccessedPenalty = %v, want 0.1", th.NeverAccessedPenalty)
	}
	if th.StaleAccessPenalty != 0.3 {
		t.Errorf("StaleAccessPenalty = %v, want 0.3", th.StaleAccessPenalty)
	}
}

func TestConsolidationPriority(t *testing.T) {
	database := openTestDB(t)
	th := DefaultStalenessThreshold()
	now := time.Now()
	old := now.AddDate(0, 0, -400) // safely beyond MaxDaysSinceAccess (90)

	la := func(ts time.Time) *time.Time { return &ts }

	tests := []struct {
		name      string
		m         *Memory
		threshold StalenessThreshold
		want      float64
		epsilon   float64
	}{
		{
			name: "nil memory scores zero",
			m:    nil,
			want: 0, epsilon: 0,
		},
		{
			name: "never accessed applies NeverAccessedPenalty",
			m: &Memory{
				Importance:  0, // defaults to 0.5
				AccessCount: 0,
				LastAccess:  nil,
				CreatedAt:   now,
			},
			threshold: th,
			want:      0.5 * 0.1, epsilon: 1e-9,
		},
		{
			name: "low access count applies StaleAccessPenalty",
			m: &Memory{
				Importance:  0,
				AccessCount: 0,
				LastAccess:  la(old),
				CreatedAt:   now,
			},
			threshold: th,
			want:      0.5 * 0.3, epsilon: 1e-9,
		},
		{
			name: "old last access applies StaleAccessPenalty despite count",
			m: &Memory{
				Importance:  0,
				AccessCount: 10,
				LastAccess:  la(old),
				CreatedAt:   now,
			},
			threshold: th,
			want:      0.5 * 0.3, epsilon: 1e-9,
		},
		{
			name: "recent access gets boost",
			m: &Memory{
				Importance:  0,
				AccessCount: 10,
				LastAccess:  la(now.AddDate(0, 0, -2)),
				CreatedAt:   now,
			},
			threshold: th,
			want:      0.5 * (1 + (math.Log1p(10)/math.Log1p(100))*0.5), epsilon: 1e-9,
		},
		{
			name: "high importance clamps to 1",
			m: &Memory{
				Importance:  10,
				AccessCount: 1000,
				LastAccess:  la(now.AddDate(0, 0, -1)),
				CreatedAt:   now,
			},
			threshold: th,
			want:      1.0, epsilon: 0,
		},
		{
			name: "age decay beyond two years",
			m: &Memory{
				Importance:  0.5,
				AccessCount: 100,
				LastAccess:  la(now.AddDate(0, 0, -2)),
				CreatedAt:   now.AddDate(0, 0, -800),
			},
			threshold: th,
			want:      0.5 * 1.5 * 0.7, epsilon: 1e-9,
		},
		{
			name: "zero NeverAccessedPenalty excludes never-accessed",
			m: &Memory{
				Importance:  0,
				AccessCount: 0,
				LastAccess:  nil,
				CreatedAt:   now,
			},
			threshold: StalenessThreshold{NeverAccessedPenalty: 0},
			want:      0, epsilon: 0,
		},
		{
			name: "zero StaleAccessPenalty excludes stale",
			m: &Memory{
				Importance:  0,
				AccessCount: 0,
				LastAccess:  la(old),
				CreatedAt:   now,
			},
			threshold: StalenessThreshold{MinAccessCount: 1, StaleAccessPenalty: 0},
			want:      0, epsilon: 0,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := database.ConsolidationPriority(tt.m, tt.threshold)
			if math.Abs(got-tt.want) > tt.epsilon {
				t.Errorf("ConsolidationPriority() = %v, want %v (±%v)", got, tt.want, tt.epsilon)
			}
		})
	}
}

// TestGetStaleMemoriesThresholdFiltering verifies that GetStaleMemories
// excludes archived and expired working memories, includes stale candidates,
// and orders results by ConsolidationPriority ascending (stalest first).
func TestGetStaleMemoriesThresholdFiltering(t *testing.T) {
	database := openTestDB(t)
	now := time.Now()
	la := func(ts time.Time) *time.Time { return &ts }

	seed := []*Memory{
		{
			ID: "stale-low-count", Content: "stale by low count", Scope: "global",
			ConsolidationStatus: "raw", Metadata: map[string]string{},
			Importance: 0.5, AccessCount: 1, LastAccess: la(now.AddDate(0, 0, -400)),
			CreatedAt: now,
		},
		{
			ID: "stale-old-access", Content: "stale by old access", Scope: "global",
			ConsolidationStatus: "raw", Metadata: map[string]string{},
			Importance: 0.5, AccessCount: 5, LastAccess: la(now.AddDate(0, 0, -500)),
			CreatedAt: now,
		},
		{
			ID: "fresh", Content: "recently accessed", Scope: "global",
			ConsolidationStatus: "raw", Metadata: map[string]string{},
			Importance: 0.5, AccessCount: 50, LastAccess: la(now.AddDate(0, 0, -1)),
			CreatedAt: now,
		},
		{
			ID: "archived", Content: "already archived", Scope: "global",
			ConsolidationStatus: "archived", Metadata: map[string]string{},
			Importance: 0.5, AccessCount: 1, LastAccess: la(now.AddDate(0, 0, -400)),
			CreatedAt: now,
		},
		{
			ID: "expired-working", Content: "expired working memory", Scope: "global",
			ConsolidationStatus: "raw", Metadata: map[string]string{},
			Importance: 0.5, AccessCount: 1, LastAccess: la(now.AddDate(0, 0, -400)),
			CreatedAt: now, Tier: "working", ExpiresAt: la(now.Add(-time.Hour)),
		},
	}
	for _, m := range seed {
		if err := database.SaveMemory(m); err != nil {
			t.Fatalf("failed to seed %s: %v", m.ID, err)
		}
	}

	th := DefaultStalenessThreshold()
	got, err := database.GetStaleMemories(10, th)
	if err != nil {
		t.Fatalf("GetStaleMemories: %v", err)
	}

	gotIDs := make(map[string]bool, len(got))
	for _, m := range got {
		gotIDs[m.ID] = true
	}
	for _, want := range []string{"stale-low-count", "stale-old-access", "fresh"} {
		if !gotIDs[want] {
			t.Errorf("expected %s among stale memories, got %v", want, gotIDs)
		}
	}
	for _, excluded := range []string{"archived", "expired-working"} {
		if gotIDs[excluded] {
			t.Errorf("expected %s to be excluded, got %v", excluded, gotIDs)
		}
	}
	if len(got) != 3 {
		t.Fatalf("expected 3 stale memories, got %d", len(got))
	}

	// Ordering: stalest first, priorities non-decreasing.
	prev := -1.0
	for _, m := range got {
		p := database.ConsolidationPriority(m, th)
		if p < prev {
			t.Errorf("priorities not ascending: %v then %v", prev, p)
		}
		prev = p
	}
	if got[0].ID == "fresh" {
		t.Errorf("expected fresh memory last (highest priority), got first: %v", got[0].ID)
	}

	// Limit is honored.
	limited, err := database.GetStaleMemories(2, th)
	if err != nil {
		t.Fatalf("GetStaleMemories(2): %v", err)
	}
	if len(limited) != 2 {
		t.Errorf("expected 2 results with limit 2, got %d", len(limited))
	}

	// Non-positive limit defaults to 100.
	all, err := database.GetStaleMemories(0, th)
	if err != nil {
		t.Fatalf("GetStaleMemories(0): %v", err)
	}
	if len(all) != 3 {
		t.Errorf("expected default limit to return all 3, got %d", len(all))
	}
}
