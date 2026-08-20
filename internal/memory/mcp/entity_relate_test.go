package mcp

import (
	"encoding/json"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/memory/db"
)

func TestEntityRelate_CreateAndDelete(t *testing.T) {
	s := helperServer(t)

	if err := s.service.db.SaveEntity(&db.Entity{ID: "e-alice", Name: "Alice", Type: "person"}); err != nil {
		t.Fatalf("seed Alice: %v", err)
	}
	if err := s.service.db.SaveEntity(&db.Entity{ID: "e-bob", Name: "Bob", Type: "person"}); err != nil {
		t.Fatalf("seed Bob: %v", err)
	}

	createRes := callTool(s, "entity_relate", map[string]interface{}{
		"from":     "Alice",
		"relation": "works-with",
		"to":       "Bob",
	})
	if code, msg := getToolError(createRes); code != 0 {
		t.Fatalf("unexpected error creating relation: %v %s", code, msg)
	}
	text := getToolText(createRes)
	if !strings.Contains(text, "Related") || !strings.Contains(text, "works-with") {
		t.Errorf("unexpected create response: %q", text)
	}

	out, err := s.service.db.OutgoingRelations("e-alice")
	if err != nil {
		t.Fatalf("OutgoingRelations: %v", err)
	}
	if len(out) != 1 || out[0].ToEntityID != "e-bob" {
		t.Fatalf("expected relation to be persisted, got %+v", out)
	}

	deleteRes := callTool(s, "entity_relate", map[string]interface{}{
		"from":     "Alice",
		"relation": "works-with",
		"to":       "Bob",
		"action":   "delete",
	})
	if code, msg := getToolError(deleteRes); code != 0 {
		t.Fatalf("unexpected error deleting relation: %v %s", code, msg)
	}

	out, err = s.service.db.OutgoingRelations("e-alice")
	if err != nil {
		t.Fatalf("OutgoingRelations after delete: %v", err)
	}
	if len(out) != 0 {
		t.Fatalf("expected 0 relations after delete, got %d", len(out))
	}
}

func TestEntityRelate_UnknownEntity(t *testing.T) {
	s := helperServer(t)
	if err := s.service.db.SaveEntity(&db.Entity{ID: "e-alice", Name: "Alice", Type: "person"}); err != nil {
		t.Fatalf("seed Alice: %v", err)
	}

	res := callTool(s, "entity_relate", map[string]interface{}{
		"from":     "Alice",
		"relation": "works-with",
		"to":       "Nonexistent",
	})
	text := getToolText(res)
	if text == "" || !strings.Contains(text, "not found") {
		t.Fatalf("expected an error mentioning 'not found', got %q", text)
	}
}

func TestEntityRelate_MissingRequiredArgs(t *testing.T) {
	s := helperServer(t)
	res := callTool(s, "entity_relate", map[string]interface{}{
		"from": "Alice",
	})
	text := getToolText(res)
	if text == "" {
		t.Fatal("expected an error for missing 'relation'/'to'")
	}
}

func TestGraphNeighbors_MatchesCLIShape(t *testing.T) {
	s := helperServer(t)
	if err := s.service.db.SaveEntity(&db.Entity{ID: "e-alice", Name: "Alice", Type: "person"}); err != nil {
		t.Fatalf("seed Alice: %v", err)
	}
	if err := s.service.db.SaveEntity(&db.Entity{ID: "e-bob", Name: "Bob", Type: "person"}); err != nil {
		t.Fatalf("seed Bob: %v", err)
	}
	if err := s.service.db.SaveEntityRelation(&db.EntityRelation{FromEntityID: "e-alice", ToEntityID: "e-bob", RelationType: "works-with"}); err != nil {
		t.Fatalf("seed relation: %v", err)
	}

	res := callTool(s, "graph_neighbors", map[string]interface{}{"entity": "Alice", "depth": float64(1)})
	if code, msg := getToolError(res); code != 0 {
		t.Fatalf("unexpected error: %v %s", code, msg)
	}

	var parsed struct {
		Nodes []*db.Entity         `json:"nodes"`
		Edges []*db.EntityRelation `json:"edges"`
	}
	if err := json.Unmarshal([]byte(getToolText(res)), &parsed); err != nil {
		t.Fatalf("graph_neighbors output is not valid JSON: %v", err)
	}
	if len(parsed.Nodes) != 2 {
		t.Fatalf("expected 2 nodes, got %d", len(parsed.Nodes))
	}
	if len(parsed.Edges) != 1 {
		t.Fatalf("expected 1 edge, got %d", len(parsed.Edges))
	}
}

func TestGraphNeighbors_UnknownEntity(t *testing.T) {
	s := helperServer(t)
	res := callTool(s, "graph_neighbors", map[string]interface{}{"entity": "Nonexistent"})
	text := getToolText(res)
	if text == "" || !strings.Contains(text, "not found") {
		t.Fatalf("expected an error mentioning 'not found', got %q", text)
	}
}
