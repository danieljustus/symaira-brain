package mcp

import (
	"encoding/base64"
	"encoding/json"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/db"
)

func TestDecodeMemoryCursor_CompositeAndLegacy(t *testing.T) {
	ts := time.Date(2026, 1, 1, 12, 0, 0, 0, time.UTC)

	// Composite round-trip.
	comp := encodeMemoryCursor(ts, "mem-42")
	got, err := decodeMemoryCursor(comp)
	if err != nil {
		t.Fatalf("decode composite: %v", err)
	}
	if !got.Timestamp.Equal(ts) {
		t.Fatalf("composite timestamp: got %v, want %v", got.Timestamp, ts)
	}
	if got.ID != "mem-42" {
		t.Fatalf("composite id: got %q, want %q", got.ID, "mem-42")
	}

	// Legacy round-trip: raw RFC3339Nano string.
	legacy := ts.UTC().Format(time.RFC3339Nano)
	got, err = decodeMemoryCursor(legacy)
	if err != nil {
		t.Fatalf("decode legacy: %v", err)
	}
	if !got.Timestamp.Equal(ts) {
		t.Fatalf("legacy timestamp: got %v, want %v", got.Timestamp, ts)
	}
	if got.ID != "" {
		t.Fatalf("legacy id should be empty, got %q", got.ID)
	}

	// Legacy base64-encoded RFC3339Nano string (used by HTTP sync handler).
	legacyB64 := base64.StdEncoding.EncodeToString([]byte(legacy))
	got, err = decodeMemoryCursor(legacyB64)
	if err != nil {
		t.Fatalf("decode legacy base64: %v", err)
	}
	if !got.Timestamp.Equal(ts) {
		t.Fatalf("legacy base64 timestamp: got %v, want %v", got.Timestamp, ts)
	}
	if got.ID != "" {
		t.Fatalf("legacy base64 id should be empty, got %q", got.ID)
	}
}

func TestToolMemoryList_CursorPaginationNoDuplicates(t *testing.T) {
	s := helperServer(t)
	shared := time.Date(2026, 6, 15, 10, 0, 0, 0, time.UTC)

	mems := []*db.Memory{
		{ID: "p1", Content: "1", Scope: "global", CreatedAt: shared.Add(-3 * time.Hour), ReviewStatus: db.ReviewApproved},
		{ID: "p2", Content: "2", Scope: "global", CreatedAt: shared.Add(-2 * time.Hour), ReviewStatus: db.ReviewApproved},
		{ID: "p3", Content: "3", Scope: "global", CreatedAt: shared, ReviewStatus: db.ReviewApproved},
		{ID: "p4", Content: "4", Scope: "global", CreatedAt: shared, ReviewStatus: db.ReviewApproved},
		{ID: "p5", Content: "5", Scope: "global", CreatedAt: shared, ReviewStatus: db.ReviewApproved},
		{ID: "p6", Content: "6", Scope: "global", CreatedAt: shared.Add(1 * time.Hour), ReviewStatus: db.ReviewApproved},
	}
	for _, m := range mems {
		if err := s.DB().SaveMemory(m); err != nil {
			t.Fatalf("save %s: %v", m.ID, err)
		}
	}

	pageSize := 2
	res1 := callTool(s, "memory_list", map[string]interface{}{"limit": pageSize})
	text1 := getToolText(res1)
	var page1 memoryListPageResponse
	if err := json.Unmarshal([]byte(text1), &page1); err != nil {
		t.Fatalf("unmarshal page1: %v", err)
	}
	if len(page1.Memories) != pageSize {
		t.Fatalf("page1 size %d, want %d", len(page1.Memories), pageSize)
	}
	if !page1.Truncated {
		t.Fatalf("page1 should be truncated when more data exists")
	}

	// Page 2 using next_cursor.
	if page1.NextCursor == "" {
		t.Fatalf("page1 missing next_cursor")
	}
	res2 := callTool(s, "memory_list", map[string]interface{}{"limit": pageSize, "cursor": page1.NextCursor})
	text2 := getToolText(res2)
	var page2 memoryListPageResponse
	if err := json.Unmarshal([]byte(text2), &page2); err != nil {
		t.Fatalf("unmarshal page2: %v", err)
	}
	if len(page2.Memories) != pageSize {
		t.Fatalf("page2 size %d, want %d", len(page2.Memories), pageSize)
	}

	ids1 := map[string]bool{}
	for _, m := range page1.Memories {
		ids1[m.ID] = true
	}
	ids2 := map[string]bool{}
	for _, m := range page2.Memories {
		ids2[m.ID] = true
	}

	// Disjoint.
	for id := range ids1 {
		if ids2[id] {
			t.Fatalf("duplicate memory %q across pages", id)
		}
	}

	// Complete for first 2*pageSize items: union must contain the 4 newest memories.
	expected := map[string]bool{"p6": true, "p5": true, "p4": true, "p3": true}
	all := map[string]bool{}
	for id := range ids1 {
		all[id] = true
	}
	for id := range ids2 {
		all[id] = true
	}
	for id := range expected {
		if !all[id] {
			t.Fatalf("missing expected memory %q in pages 1+2", id)
		}
	}
	for id := range all {
		if !expected[id] {
			t.Fatalf("unexpected memory %q in pages 1+2", id)
		}
	}

	// Page 2 must be strictly older than page 1's last item.
	last1 := page1.Memories[len(page1.Memories)-1]
	for _, m := range page2.Memories {
		if m.CreatedAt.After(last1.CreatedAt) {
			t.Fatalf("page2 contains newer memory than page1 last item")
		}
		if m.CreatedAt.Equal(last1.CreatedAt) && m.ID >= last1.ID {
			t.Fatalf("page2 contains same-timestamp memory with id >= page1 last id")
		}
	}
}

func TestToolMemoryList_LegacyCursorStillWorks(t *testing.T) {
	s := helperServer(t)
	ts := time.Date(2026, 3, 1, 0, 0, 0, 0, time.UTC)
	m1 := &db.Memory{ID: "legacy-1", Content: "old", Scope: "global", CreatedAt: ts.Add(-1 * time.Hour), ReviewStatus: db.ReviewApproved}
	m2 := &db.Memory{ID: "legacy-2", Content: "new", Scope: "global", CreatedAt: ts.Add(1 * time.Hour), ReviewStatus: db.ReviewApproved}
	for _, m := range []*db.Memory{m1, m2} {
		if err := s.DB().SaveMemory(m); err != nil {
			t.Fatalf("save: %v", err)
		}
	}

	legacyCursor := ts.UTC().Format(time.RFC3339Nano)
	res := callTool(s, "memory_list", map[string]interface{}{"limit": 10, "cursor": legacyCursor})
	text := getToolText(res)
	var page memoryListPageResponse
	if err := json.Unmarshal([]byte(text), &page); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}
	if len(page.Memories) != 1 {
		t.Fatalf("expected 1 result with legacy cursor, got %d", len(page.Memories))
	}
	if page.Memories[0].ID != "legacy-1" {
		t.Fatalf("expected legacy-1, got %s", page.Memories[0].ID)
	}
}

func TestToolMemoryList_NextCursorIsComposite(t *testing.T) {
	s := helperServer(t)
	mems := []*db.Memory{
		{ID: "comp-a", Content: "a", Scope: "global", ReviewStatus: db.ReviewApproved},
		{ID: "comp-b", Content: "b", Scope: "global", ReviewStatus: db.ReviewApproved},
		{ID: "comp-c", Content: "c", Scope: "global", ReviewStatus: db.ReviewApproved},
	}
	for _, m := range mems {
		if err := s.DB().SaveMemory(m); err != nil {
			t.Fatalf("save: %v", err)
		}
	}

	res := callTool(s, "memory_list", map[string]interface{}{"limit": 1})
	text := getToolText(res)
	var page memoryListPageResponse
	if err := json.Unmarshal([]byte(text), &page); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}
	if page.NextCursor == "" {
		t.Fatalf("expected next_cursor on truncated page")
	}

	// Decode and verify it's composite JSON, not bare timestamp.
	b, err := base64.StdEncoding.DecodeString(page.NextCursor)
	if err != nil {
		t.Fatalf("next_cursor is not base64: %v", err)
	}
	var obj map[string]interface{}
	if err := json.Unmarshal(b, &obj); err != nil {
		t.Fatalf("next_cursor is not JSON: %v", err)
	}
	if _, ok := obj["ts"]; !ok {
		t.Fatalf("next_cursor missing ts field")
	}
	if _, ok := obj["id"]; !ok {
		t.Fatalf("next_cursor missing id field")
	}
}
