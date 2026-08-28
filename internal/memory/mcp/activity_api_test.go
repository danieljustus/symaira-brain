package mcp

import (
	"context"
	"encoding/json"
	"fmt"
	"strings"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/activity"
)

func TestActivityToolsAreBoundedRedactedAndAudited(t *testing.T) {
	s := helperServer(t)
	base := time.Now().UTC().Add(-time.Hour).Truncate(time.Second)
	if err := s.activityStore.SaveSegment(activity.Segment{
		Source: "editor", Granularity: activity.Granularity10Min,
		StartedAt: base, EndedAt: base.Add(10 * time.Minute),
		RedactedSummary: "edited a bounded file", RawRef: "capture-ref-1",
	}); err != nil {
		t.Fatal(err)
	}
	input := json.RawMessage(fmt.Sprintf(`{"query":"bounded","from":%q,"to":%q,"limit":5,"max_tokens":200}`, base.Add(-time.Hour).Format(time.RFC3339Nano), base.Add(time.Hour).Format(time.RFC3339Nano)))
	value, err := s.handleActivitySearch(context.Background(), input)
	if err != nil {
		t.Fatal(err)
	}
	text, ok := value.(string)
	if !ok {
		t.Fatalf("activity response type = %T", value)
	}
	if !strings.Contains(text, activityUntrustedFenceStart) || !strings.Contains(text, "capture-ref-1") {
		t.Fatalf("missing fenced summary/provenance: %s", text)
	}
	if _, err := s.handleActivitySearch(context.Background(), json.RawMessage(`{"query":"bounded"}`)); err == nil || !strings.Contains(err.Error(), "unbounded") {
		t.Fatalf("unbounded search error = %v", err)
	}
	logs, err := s.service.db.GetQueryLogEntries(20)
	if err != nil {
		t.Fatal(err)
	}
	found := false
	for _, entry := range logs {
		if entry.Tool == "activity_search" {
			found = true
		}
	}
	if !found {
		t.Fatalf("activity query was not written to query_log: %+v", logs)
	}
}
