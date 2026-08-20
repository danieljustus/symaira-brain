package mcp

import (
	"strings"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
	"github.com/danieljustus/symaira-brain/internal/memory/db"
)

// ---------------------------------------------------------------------------
// Write-path governance (#485 staging, #486 semantic kind)
// ---------------------------------------------------------------------------

func TestMemorySetRequiresKind(t *testing.T) {
	s := helperServer(t)

	// Missing kind → refusal that names the buckets.
	res := callTool(s, "memory_set", map[string]interface{}{"content": "x"})
	text := getToolText(res)
	if !strings.Contains(text, "'kind' is required") {
		t.Fatalf("expected kind refusal, got %q", text)
	}
	for _, bucket := range []string{"user", "feedback", "project", "reference"} {
		if !strings.Contains(text, bucket) {
			t.Fatalf("kind refusal must name bucket %q, got %q", bucket, text)
		}
	}

	// Unclassifiable kind → refusal.
	res = callTool(s, "memory_set", map[string]interface{}{"content": "x", "kind": "banana"})
	text = getToolText(res)
	if !strings.Contains(text, "'kind' is required") {
		t.Fatalf("expected kind refusal for unknown kind, got %q", text)
	}

	// Synonym is accepted.
	res = callTool(s, "memory_set", map[string]interface{}{"content": "user likes tea", "kind": "preferences"})
	text = getToolText(res)
	if !strings.Contains(text, "Memory saved successfully") {
		t.Fatalf("synonym kind should be accepted, got %q", text)
	}
	id := memoryIDFromSetText(t, text)
	m, err := s.service.Get(id)
	if err != nil {
		t.Fatal(err)
	}
	if m.Kind != db.KindUser {
		t.Fatalf("kind = %q, want %q", m.Kind, db.KindUser)
	}
	if m.ReviewStatus != db.ReviewApproved {
		t.Fatalf("default review status = %q, want approved", m.ReviewStatus)
	}
}

func TestMemorySetStagedFlow(t *testing.T) {
	s := helperServer(t)

	setRes := callTool(s, "memory_set", map[string]interface{}{
		"content": "autonomous guess about the api",
		"kind":    "project",
		"staged":  true,
	})
	text := getToolText(setRes)
	if !strings.Contains(text, "staged as candidate") {
		t.Fatalf("expected staged confirmation, got %q", text)
	}
	id := memoryIDFromSetText(t, text)

	// The staged candidate is not retrievable.
	searchRes := callTool(s, "memory_search", map[string]interface{}{"query": "autonomous guess about the api", "limit": 5})
	if strings.Contains(getToolText(searchRes), "autonomous guess") {
		t.Fatalf("staged candidate leaked into search: %q", getToolText(searchRes))
	}

	// It shows up in the candidates queue.
	candRes := callTool(s, "memory_candidates", map[string]interface{}{})
	if !strings.Contains(getToolText(candRes), id) {
		t.Fatalf("candidates queue missing staged id %s: %q", id, getToolText(candRes))
	}

	// Promote → retrievable.
	promoteRes := callTool(s, "memory_promote", map[string]interface{}{"id": id})
	if !strings.Contains(getToolText(promoteRes), "promoted") {
		t.Fatalf("promote failed: %q", getToolText(promoteRes))
	}
	searchRes = callTool(s, "memory_search", map[string]interface{}{"query": "autonomous guess about the api", "limit": 5})
	if !strings.Contains(getToolText(searchRes), "autonomous guess") {
		t.Fatalf("promoted candidate missing from search: %q", getToolText(searchRes))
	}
}

func TestMemoryRejectRefusesLiveMemories(t *testing.T) {
	s := helperServer(t)

	setRes := callTool(s, "memory_set", map[string]interface{}{"content": "live fact", "kind": "user"})
	liveID := memoryIDFromSetText(t, getToolText(setRes))

	res := callTool(s, "memory_reject", map[string]interface{}{"id": liveID})
	if !strings.Contains(getToolText(res), "Failed to reject memory") {
		t.Fatalf("rejecting a live memory must fail, got %q", getToolText(res))
	}
}

func TestMemoryRejectRemovesCandidate(t *testing.T) {
	s := helperServer(t)

	setRes := callTool(s, "memory_set", map[string]interface{}{"content": "discard me", "kind": "reference", "staged": true})
	id := memoryIDFromSetText(t, getToolText(setRes))

	res := callTool(s, "memory_reject", map[string]interface{}{"id": id})
	if !strings.Contains(getToolText(res), "rejected") {
		t.Fatalf("reject failed: %q", getToolText(res))
	}

	m, err := s.service.Get(id)
	if err == nil && m != nil {
		t.Fatalf("rejected candidate still present: %+v", m)
	}
	if err == nil {
		t.Fatal("rejected candidate still resolvable")
	}
	if _, ok := err.(*NotFoundError); !ok {
		t.Fatalf("expected NotFoundError after reject, got %v", err)
	}
}

func TestMemorySetStageWritesByDefaultConfig(t *testing.T) {
	cfg := config.Defaults()
	cfg.Memory.StageWritesByDefault = true
	s := helperServerCfg(t, cfg)

	// No explicit staged flag → server default stages the write.
	res := callTool(s, "memory_set", map[string]interface{}{"content": "default staged fact", "kind": "user"})
	text := getToolText(res)
	if !strings.Contains(text, "staged as candidate") {
		t.Fatalf("stage_writes_by_default should stage writes, got %q", text)
	}
	id := memoryIDFromSetText(t, text)
	m, err := s.service.Get(id)
	if err != nil {
		t.Fatal(err)
	}
	if m.ReviewStatus != db.ReviewStaged {
		t.Fatalf("review status = %q, want staged", m.ReviewStatus)
	}

	// Explicit staged=false still wins.
	res = callTool(s, "memory_set", map[string]interface{}{"content": "explicit live fact", "kind": "user", "staged": false})
	text = getToolText(res)
	if !strings.Contains(text, "Memory saved successfully") {
		t.Fatalf("explicit staged=false must bypass the default, got %q", text)
	}
	id = memoryIDFromSetText(t, text)
	m, err = s.service.Get(id)
	if err != nil {
		t.Fatal(err)
	}
	if m.ReviewStatus != db.ReviewApproved {
		t.Fatalf("review status = %q, want approved", m.ReviewStatus)
	}
}
