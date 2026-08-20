package mcp

import (
	"encoding/json"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/memory/db"
)

func TestEntityRelate_IDBasedWithProvenance(t *testing.T) {
	s := helperServer(t)
	if err := s.service.db.SaveEntity(&db.Entity{ID: "mcp-alice", Name: "Alice", Type: "person"}); err != nil {
		t.Fatalf("seed Alice: %v", err)
	}
	if err := s.service.db.SaveEntity(&db.Entity{ID: "mcp-meeting", Name: "Standup", Type: "event"}); err != nil {
		t.Fatalf("seed meeting: %v", err)
	}

	res := callTool(s, "entity_relate", map[string]interface{}{
		"from_id":      "mcp-alice",
		"to_id":        "mcp-meeting",
		"relation":     "attended",
		"source":       "symdesk",
		"source_ref":   "meeting-123",
		"verification": "verified",
		"evidence":     `{"source_doc_id":"doc-1","char_start":0,"char_end":10}`,
	})
	if code, msg := getToolError(res); code != 0 {
		t.Fatalf("unexpected error: %v %s", code, msg)
	}

	var saved db.EntityRelation
	if err := json.Unmarshal([]byte(getToolText(res)), &saved); err != nil {
		t.Fatalf("entity_relate output is not valid JSON: %v\noutput: %s", err, getToolText(res))
	}
	if saved.ID == "" || saved.Source != "symdesk" || saved.SourceRef != "meeting-123" || saved.Verification != "verified" {
		t.Fatalf("unexpected saved relation: %+v", saved)
	}

	fetched, err := s.service.db.GetEntityRelationByID(saved.ID)
	if err != nil {
		t.Fatalf("GetEntityRelationByID: %v", err)
	}
	if fetched == nil {
		t.Fatal("expected relation to be persisted")
	}
}

func TestEntityRelate_RetryWithSameProvenanceIsIdempotent(t *testing.T) {
	s := helperServer(t)
	if err := s.service.db.SaveEntity(&db.Entity{ID: "mcp-a", Name: "A", Type: "person"}); err != nil {
		t.Fatalf("seed: %v", err)
	}
	if err := s.service.db.SaveEntity(&db.Entity{ID: "mcp-b", Name: "B", Type: "event"}); err != nil {
		t.Fatalf("seed: %v", err)
	}

	args := map[string]interface{}{
		"from_id": "mcp-a", "to_id": "mcp-b", "relation": "attended",
		"source": "symdesk", "source_ref": "meeting-1",
	}

	first := callTool(s, "entity_relate", args)
	if code, msg := getToolError(first); code != 0 {
		t.Fatalf("unexpected error: %v %s", code, msg)
	}
	second := callTool(s, "entity_relate", args)
	if code, msg := getToolError(second); code != 0 {
		t.Fatalf("unexpected error: %v %s", code, msg)
	}

	var firstRel, secondRel db.EntityRelation
	if err := json.Unmarshal([]byte(getToolText(first)), &firstRel); err != nil {
		t.Fatalf("first output not JSON: %v", err)
	}
	if err := json.Unmarshal([]byte(getToolText(second)), &secondRel); err != nil {
		t.Fatalf("second output not JSON: %v", err)
	}
	if firstRel.ID != secondRel.ID {
		t.Fatalf("expected idempotent retry to return the same relation, got %s vs %s", firstRel.ID, secondRel.ID)
	}

	out, err := s.service.db.OutgoingRelations("mcp-a")
	if err != nil {
		t.Fatalf("OutgoingRelations: %v", err)
	}
	if len(out) != 1 {
		t.Fatalf("expected exactly 1 relation after idempotent retry, got %d", len(out))
	}
}

func TestEntityRelate_MixedIDAndNameRejected(t *testing.T) {
	s := helperServer(t)
	if err := s.service.db.SaveEntity(&db.Entity{ID: "mcp-mix-a", Name: "MixA", Type: "person"}); err != nil {
		t.Fatalf("seed: %v", err)
	}

	res := callTool(s, "entity_relate", map[string]interface{}{
		"from_id":  "mcp-mix-a",
		"to":       "SomeName",
		"relation": "works-with",
	})
	text := getToolText(res)
	if text == "" {
		t.Fatal("expected an error for mixed from_id/to name input")
	}
}

func TestEntityRelate_VerifiedConflictRejected(t *testing.T) {
	s := helperServer(t)
	if err := s.service.db.SaveEntity(&db.Entity{ID: "mcp-vc-a", Name: "VCA", Type: "person"}); err != nil {
		t.Fatalf("seed: %v", err)
	}
	if err := s.service.db.SaveEntity(&db.Entity{ID: "mcp-vc-b", Name: "VCB", Type: "event"}); err != nil {
		t.Fatalf("seed: %v", err)
	}

	first := callTool(s, "entity_relate", map[string]interface{}{
		"from_id": "mcp-vc-a", "to_id": "mcp-vc-b", "relation": "attended",
		"source": "symdesk", "source_ref": "meeting-1", "verification": "verified",
	})
	if code, msg := getToolError(first); code != 0 {
		t.Fatalf("unexpected error on initial verified create: %v %s", code, msg)
	}

	conflict := callTool(s, "entity_relate", map[string]interface{}{
		"from_id": "mcp-vc-a", "to_id": "mcp-vc-b", "relation": "attended",
		"source": "other-tool", "source_ref": "different-ref", "verification": "unverified",
	})
	text := getToolText(conflict)
	if text == "" {
		t.Fatal("expected an error when overwriting a verified relation with different provenance")
	}
}

func TestEntityRelate_LegacyNameBasedPathUnaffected(t *testing.T) {
	s := helperServer(t)
	if err := s.service.db.SaveEntity(&db.Entity{ID: "mcp-legacy-a", Name: "Alice", Type: "person"}); err != nil {
		t.Fatalf("seed: %v", err)
	}
	if err := s.service.db.SaveEntity(&db.Entity{ID: "mcp-legacy-b", Name: "Bob", Type: "person"}); err != nil {
		t.Fatalf("seed: %v", err)
	}

	res := callTool(s, "entity_relate", map[string]interface{}{
		"from": "Alice", "relation": "works-with", "to": "Bob",
	})
	if code, msg := getToolError(res); code != 0 {
		t.Fatalf("unexpected error: %v %s", code, msg)
	}
	text := getToolText(res)
	if !strings.Contains(text, "Related") || !strings.Contains(text, "works-with") {
		t.Errorf("expected legacy plain-text response unchanged, got %q", text)
	}
}

func TestEntityRelate_MissingRelationStillRejected(t *testing.T) {
	s := helperServer(t)
	res := callTool(s, "entity_relate", map[string]interface{}{
		"from_id": "does-not-matter",
	})
	text := getToolText(res)
	if text == "" {
		t.Fatal("expected an error for missing 'relation'")
	}
}
