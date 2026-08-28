package activity

import (
	"strings"
	"testing"
	"time"
)

func TestActivityReadAPIRejectsUnboundedAndWideQueries(t *testing.T) {
	base := time.Date(2026, 8, 28, 12, 0, 0, 0, time.UTC)
	valid := SearchOptions{Query: "editor", From: base, To: base.Add(time.Hour), Limit: 1, MaxTokens: 100}
	cases := []SearchOptions{
		valid,
		{Query: "editor", From: base, To: base.Add(time.Hour), Limit: 0, MaxTokens: 100},
		{Query: "editor", From: base, To: base.Add(time.Hour), Limit: 1, MaxTokens: 0},
		{Query: "editor", From: base, To: base.Add(MaxRange + time.Nanosecond), Limit: 1, MaxTokens: 100},
	}
	if err := ValidateSearchOptions(cases[0]); err != nil {
		t.Fatal(err)
	}
	for i, opts := range cases[1:] {
		if err := ValidateSearchOptions(opts); err == nil {
			t.Errorf("case %d unexpectedly accepted", i)
		}
	}
}

func TestActivityReadAPIReturnsRedactedProvenanceOnly(t *testing.T) {
	base := time.Date(2026, 8, 28, 12, 0, 0, 0, time.UTC)
	store, _, _ := newActivityStore(t, Options{Now: func() time.Time { return base }})
	if err := store.SaveSegment(Segment{
		Source: "editor", Granularity: Granularity10Min, StartedAt: base,
		EndedAt: base.Add(10 * time.Minute), RedactedSummary: "edited code", RawRef: "/opaque/source.ref",
		Applications: []string{"Editor"},
	}); err != nil {
		t.Fatal(err)
	}
	page, err := store.Search(SearchOptions{Query: "edited", From: base.Add(-time.Minute), To: base.Add(time.Hour), Limit: 1, MaxTokens: 100})
	if err != nil {
		t.Fatal(err)
	}
	if len(page.Results) != 1 || page.Results[0].Kind != "segment" {
		t.Fatalf("unexpected results: %+v", page.Results)
	}
	if page.Results[0].Provenance.Reference != "/opaque/source.ref" {
		t.Errorf("provenance reference lost: %+v", page.Results[0].Provenance)
	}
	if strings.Contains(page.Results[0].Summary, "raw_payload") {
		t.Error("raw payload leaked into summary")
	}
	if item, err := store.GetReadItem(page.Results[0].ID); err != nil || item == nil {
		t.Fatalf("get read item: %v, %v", item, err)
	}
}

func TestActivityReadAPIStatusOmitsContent(t *testing.T) {
	store, _, _ := newActivityStore(t, Options{})
	status, err := store.Status()
	if err != nil {
		t.Fatal(err)
	}
	if status.ActiveSegments != 0 || status.ActiveEpisodes != 0 {
		t.Fatalf("unexpected empty status: %+v", status)
	}
}
