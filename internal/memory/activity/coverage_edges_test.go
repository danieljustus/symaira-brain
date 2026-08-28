package activity

import (
	"testing"
	"time"
)

func TestActivityValidationAndReadHelperEdges(t *testing.T) {
	base := time.Date(2026, 8, 28, 12, 0, 0, 0, time.UTC)
	segment := Segment{ID: "segment", Source: "source", Granularity: Granularity10Min, StartedAt: base, EndedAt: base.Add(time.Minute), RedactedSummary: "summary", ExpiresAt: base.Add(time.Hour)}
	segmentCases := []Segment{
		{},
		{ID: "segment"},
		{ID: "segment", Source: "source", Granularity: "invalid", StartedAt: base, EndedAt: base.Add(time.Minute), RedactedSummary: "summary", ExpiresAt: base.Add(time.Hour)},
		{ID: "segment", Source: "source", Granularity: Granularity10Min, EndedAt: base.Add(time.Minute), RedactedSummary: "summary", ExpiresAt: base.Add(time.Hour)},
		{ID: "segment", Source: "source", Granularity: Granularity10Min, StartedAt: base, EndedAt: base, RedactedSummary: "summary", ExpiresAt: base.Add(time.Hour)},
		{ID: "segment", Source: "source", Granularity: Granularity10Min, StartedAt: base, EndedAt: base.Add(time.Minute), ExpiresAt: base.Add(time.Hour)},
		{ID: "segment", Source: "source", Granularity: Granularity10Min, StartedAt: base, EndedAt: base.Add(time.Minute), RedactedSummary: "summary"},
	}
	for i, value := range segmentCases {
		if err := value.validate(); err == nil {
			t.Errorf("segment case %d unexpectedly valid", i)
		}
	}
	if err := segment.validate(); err != nil {
		t.Fatalf("valid segment: %v", err)
	}

	episodeCases := []Episode{
		{},
		{ID: "episode"},
		{ID: "episode", Title: "title", StartedAt: base, EndedAt: base},
		{ID: "episode", Title: "title", StartedAt: base, EndedAt: base.Add(time.Minute), Confidence: -0.1},
		{ID: "episode", Title: "title", StartedAt: base, EndedAt: base.Add(time.Minute), Confidence: 1.1},
		{ID: "episode", Title: "title", StartedAt: base, EndedAt: base.Add(time.Minute), Confidence: 0.5},
		{ID: "episode", Title: "title", StartedAt: base, EndedAt: base.Add(time.Minute), Confidence: 0.5, ExpiresAt: base.Add(time.Hour)},
	}
	for i, value := range episodeCases {
		if i == len(episodeCases)-1 {
			if err := value.validate(); err != nil {
				t.Fatalf("valid episode: %v", err)
			}
			continue
		}
		if err := value.validate(); err == nil {
			t.Errorf("episode case %d unexpectedly valid", i)
		}
	}
	if episodeID("source", base, base.Add(time.Hour)) == "" {
		t.Fatal("episode ID is empty")
	}

	valid := SearchOptions{Query: "query", From: base, To: base.Add(time.Hour), Limit: 1, MaxTokens: 1}
	invalidSearches := []SearchOptions{
		{From: valid.From, To: valid.To, Limit: valid.Limit, MaxTokens: valid.MaxTokens},
		{Query: string(make([]rune, MaxQueryLength+1)), From: valid.From, To: valid.To, Limit: valid.Limit, MaxTokens: valid.MaxTokens},
		{Query: "query", To: valid.To, Limit: valid.Limit, MaxTokens: valid.MaxTokens},
		{Query: "query", From: valid.From, To: valid.From, Limit: valid.Limit, MaxTokens: valid.MaxTokens},
		{Query: "query", From: valid.From, To: valid.From.Add(MaxRange + time.Second), Limit: valid.Limit, MaxTokens: valid.MaxTokens},
		{Query: "query", From: valid.From, To: valid.To, Limit: 0, MaxTokens: valid.MaxTokens},
		{Query: "query", From: valid.From, To: valid.To, Limit: MaxResults + 1, MaxTokens: valid.MaxTokens},
		{Query: "query", From: valid.From, To: valid.To, Limit: valid.Limit, MaxTokens: 0},
		{Query: "query", From: valid.From, To: valid.To, Limit: valid.Limit, MaxTokens: MaxTokens + 1},
	}
	for i, opts := range invalidSearches {
		if err := ValidateSearchOptions(opts); err == nil {
			t.Errorf("search case %d unexpectedly valid", i)
		}
	}
	if err := ValidateSearchOptions(valid); err != nil {
		t.Fatalf("valid search: %v", err)
	}

	if got := fitSummary("abcdef", 0); got != "" || fitSummary("abcdef", 1) != "abcd" || fitSummary("abc", 1) != "abc" {
		t.Fatalf("fitSummary edge results: %q", got)
	}
	if estimateActivityTokens("") != 0 || estimateActivityTokens("abcd") != 2 || nonEmptyRefs("") != nil || len(nonEmptyRefs("ref")) != 1 {
		t.Fatal("read helper edge result mismatch")
	}
	items := []ReadItem{{ID: "z", StartedAt: base}, {ID: "a", StartedAt: base}, {ID: "b", StartedAt: base.Add(time.Hour)}}
	sortReadItems(items)
	if items[0].ID != "a" || items[1].ID != "z" || !readItemBefore(items[0], items[1]) {
		t.Fatalf("sorted read items = %#v", items)
	}
}

func TestActivitySearchTruncationAndExpiredReadItems(t *testing.T) {
	base := time.Date(2026, 8, 28, 12, 0, 0, 0, time.UTC)
	clock := base
	store, database, _ := newActivityStore(t, Options{Now: func() time.Time { return clock }})
	for _, value := range []Segment{
		{ID: "search-a", Source: "editor", Granularity: Granularity10Min, StartedAt: base, EndedAt: base.Add(time.Minute), RedactedSummary: "query match"},
		{ID: "search-b", Source: "editor", Granularity: Granularity10Min, StartedAt: base.Add(time.Minute), EndedAt: base.Add(2 * time.Minute), RedactedSummary: "query match"},
	} {
		if err := store.SaveSegment(value); err != nil {
			t.Fatal(err)
		}
	}
	page, err := store.Search(SearchOptions{Query: "query", Source: "missing", From: base.Add(-time.Minute), To: base.Add(time.Hour), Limit: 1, MaxTokens: 10})
	if err != nil || len(page.Results) != 0 {
		t.Fatalf("source-filtered search = %#v, err=%v", page, err)
	}
	page, err = store.Search(SearchOptions{Query: "query", From: base.Add(-time.Minute), To: base.Add(time.Hour), Limit: 1, MaxTokens: 10})
	if err != nil || len(page.Results) != 1 || !page.Truncated {
		t.Fatalf("truncated search = %#v, err=%v", page, err)
	}
	if _, err := store.GetReadItem(""); err == nil {
		t.Fatal("empty activity ID unexpectedly accepted")
	}
	clock = base.Add(72 * time.Hour)
	if item, err := store.GetReadItem("search-a"); err != nil || item != nil {
		t.Fatalf("expired read item = %#v, err=%v", item, err)
	}
	if _, err := database.Conn().Exec("UPDATE activity_segments SET started_at = ?, ended_at = ? WHERE source = ?", "not-a-timestamp", "not-a-timestamp", "editor"); err != nil {
		t.Fatal(err)
	}
	clock = base
	if _, err := store.Status(); err == nil {
		t.Fatal("invalid stored timestamp unexpectedly accepted")
	}
}

func TestActivityRetentionExplicitTimesAndMissingSession(t *testing.T) {
	base := time.Date(2026, 8, 28, 12, 0, 0, 0, time.UTC)
	store, _, _ := newActivityStore(t, Options{Now: func() time.Time { return base }})
	if result, err := store.ClearLastSession(); err != nil || result != (RetentionResult{}) {
		t.Fatalf("empty clear-last-session = %+v, err=%v", result, err)
	}
	if _, err := store.ClearTimeRange(base, base); err == nil {
		t.Fatal("non-increasing clear range unexpectedly accepted")
	}
	if result, err := store.Expire(base.Add(time.Hour)); err != nil || result != (RetentionResult{}) {
		t.Fatalf("explicit empty expiry = %+v, err=%v", result, err)
	}
}
