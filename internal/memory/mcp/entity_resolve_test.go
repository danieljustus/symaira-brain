package mcp

import (
	"encoding/json"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/memory/db"
)

func TestEntityResolve_ExactNameMatch(t *testing.T) {
	s := helperServer(t)
	if err := s.service.db.SaveEntity(&db.Entity{ID: "e-alice", Name: "Alice", Type: "person", Aliases: []string{"Ali"}}); err != nil {
		t.Fatalf("seed Alice: %v", err)
	}

	res := callTool(s, "entity_resolve", map[string]interface{}{"query": "Alice"})
	if code, msg := getToolError(res); code != 0 {
		t.Fatalf("unexpected error: %v %s", code, msg)
	}

	var candidates []db.EntityCandidate
	if err := json.Unmarshal([]byte(getToolText(res)), &candidates); err != nil {
		t.Fatalf("entity_resolve output is not valid JSON: %v\noutput: %s", err, getToolText(res))
	}
	if len(candidates) != 1 || candidates[0].EntityID != "e-alice" || candidates[0].MatchKind != "exact_name" {
		t.Fatalf("expected exact_name match for e-alice, got %+v", candidates)
	}
}

func TestEntityResolve_TypeFilter(t *testing.T) {
	s := helperServer(t)
	if err := s.service.db.SaveEntity(&db.Entity{ID: "e-person", Name: "Sam Person", Aliases: []string{"Sam"}, Type: "person"}); err != nil {
		t.Fatalf("seed: %v", err)
	}
	if err := s.service.db.SaveEntity(&db.Entity{ID: "e-project", Name: "Sam Project", Aliases: []string{"Sam"}, Type: "project"}); err != nil {
		t.Fatalf("seed: %v", err)
	}

	res := callTool(s, "entity_resolve", map[string]interface{}{"query": "Sam", "type": "project"})
	if code, msg := getToolError(res); code != 0 {
		t.Fatalf("unexpected error: %v %s", code, msg)
	}

	var candidates []db.EntityCandidate
	if err := json.Unmarshal([]byte(getToolText(res)), &candidates); err != nil {
		t.Fatalf("output is not valid JSON: %v", err)
	}
	if len(candidates) != 1 || candidates[0].EntityID != "e-project" {
		t.Fatalf("expected only e-project, got %+v", candidates)
	}
}

func TestEntityResolve_AliasesHintExpandsMatch(t *testing.T) {
	s := helperServer(t)
	if err := s.service.db.SaveEntity(&db.Entity{ID: "e-hint", Name: "Guillaume Charhon", Aliases: []string{"GC"}, Type: "person"}); err != nil {
		t.Fatalf("seed: %v", err)
	}

	res := callTool(s, "entity_resolve", map[string]interface{}{
		"query":   "nobody matches this text",
		"aliases": "GC, someone@example.com",
	})
	if code, msg := getToolError(res); code != 0 {
		t.Fatalf("unexpected error: %v %s", code, msg)
	}

	var candidates []db.EntityCandidate
	if err := json.Unmarshal([]byte(getToolText(res)), &candidates); err != nil {
		t.Fatalf("output is not valid JSON: %v", err)
	}
	if len(candidates) != 1 || candidates[0].EntityID != "e-hint" || candidates[0].MatchKind != "exact_alias" {
		t.Fatalf("expected exact_alias match via aliases hint, got %+v", candidates)
	}
}

func TestEntityResolve_NoMatch(t *testing.T) {
	s := helperServer(t)
	res := callTool(s, "entity_resolve", map[string]interface{}{"query": "nothing here"})
	if code, msg := getToolError(res); code != 0 {
		t.Fatalf("unexpected error: %v %s", code, msg)
	}
	text := getToolText(res)
	if !strings.Contains(text, "No matching entities found") {
		t.Errorf("expected no-match message, got %q", text)
	}
}

func TestEntityResolve_MissingQuery(t *testing.T) {
	s := helperServer(t)
	res := callTool(s, "entity_resolve", map[string]interface{}{})
	text := getToolText(res)
	if text == "" {
		t.Fatal("expected an error for missing 'query'")
	}
}
