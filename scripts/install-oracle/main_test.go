package main

import (
	"encoding/json"
	"testing"
)

func TestFixtureMatchesAllowsHistoryRewriteWithSameContent(t *testing.T) {
	fixture := oracle{
		SchemaVersion:   1,
		GoRevision:      "old-squashed-parent",
		GeneratorSHA256: "generator",
		GoSources:       map[string]string{"source.go": "content-digest"},
		Cases:           []result{{ID: "linux-case", Exit: 0}},
	}
	current := fixture
	current.GoRevision = "new-squashed-commit"
	data, err := json.Marshal(fixture)
	if err != nil {
		t.Fatal(err)
	}
	if !fixtureMatches(data, current) {
		t.Fatal("history rewrite with identical source content must remain fresh")
	}
}

func TestFixtureMatchesRejectsSourceMutation(t *testing.T) {
	fixture := oracle{SchemaVersion: 1, GeneratorSHA256: "generator", GoSources: map[string]string{"source.go": "old"}, Cases: []result{{ID: "case"}}}
	current := fixture
	current.GoRevision = "new-history"
	current.GoSources = map[string]string{"source.go": "mutated"}
	data, err := json.Marshal(fixture)
	if err != nil {
		t.Fatal(err)
	}
	if fixtureMatches(data, current) {
		t.Fatal("source mutation must invalidate the fixture")
	}
}
