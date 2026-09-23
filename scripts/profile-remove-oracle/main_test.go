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
		Cases:           []result{{ID: "darwin-case", Exit: 0}},
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

func TestWindowsProfileRemoveCasesUseNativeReparseCoverageAndExplainFIFO(t *testing.T) {
	runnable, skipped := splitDefinitions("windows")
	if len(runnable) != 19 {
		t.Fatalf("Windows runnable case count = %d, want 19", len(runnable))
	}
	ids := make(map[string]bool, len(runnable))
	for _, definition := range runnable {
		ids[definition.ID] = true
	}
	for _, id := range []string{"bound_project_symlink_refuses", "symlink", "symlink_profiles_root"} {
		if !ids[id] {
			t.Errorf("Windows cases omit native symlink case %q", id)
		}
	}
	if len(skipped) != 1 || skipped[0].ID != "special_file" {
		t.Fatalf("Windows skipped cases = %+v, want only the POSIX FIFO case", skipped)
	}
	if skipped[0].Reason == "" || skipped[0].NativeAlternative == "" {
		t.Fatalf("Windows FIFO skip must explain the platform gap and closest native coverage: %+v", skipped[0])
	}

	unixRunnable, unixSkipped := splitDefinitions("linux")
	if len(unixRunnable) != 20 || len(unixSkipped) != 0 {
		t.Fatalf("Linux cases = %d runnable, %d skipped; want 20, 0", len(unixRunnable), len(unixSkipped))
	}
}
