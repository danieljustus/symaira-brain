package db

import (
	"os"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
)

func TestRuleCRUD(t *testing.T) {
	tempDir, err := os.MkdirTemp("", "symmemory-rule-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tempDir)

	oldHome := os.Getenv("HOME")
	os.Setenv("HOME", tempDir)
	defer os.Setenv("HOME", oldHome)

	database, err := Open(config.Defaults())
	if err != nil {
		t.Fatalf("failed to open database: %v", err)
	}
	defer func() { _ = database.Close() }()

	// Save rules with varying scopes
	rules := []*Rule{
		{ID: "rule-1", Content: "Always respond in Spanish", Scope: "global", Metadata: map[string]string{"priority": "high"}},
		{ID: "rule-2", Content: "Use technical language", Scope: "project", Metadata: map[string]string{"priority": "medium"}},
		{ID: "rule-3", Content: "Refer to docs first", Scope: "global", Metadata: map[string]string{"priority": "low"}},
		{ID: "rule-4", Content: "Summarize before answering", Scope: "agent", Metadata: map[string]string{}},
	}

	for _, r := range rules {
		if err := database.SaveRule(r); err != nil {
			t.Fatalf("failed to save rule %s: %v", r.ID, err)
		}
	}

	// List all rules (no scope filter)
	all, err := database.ListRules("")
	if err != nil {
		t.Fatalf("ListRules failed: %v", err)
	}
	if len(all) != 4 {
		t.Errorf("expected 4 rules, got %d", len(all))
	}

	// List rules filtered by scope
	globalRules, err := database.ListRules("global")
	if err != nil {
		t.Fatalf("ListRules(global) failed: %v", err)
	}
	if len(globalRules) != 2 {
		t.Errorf("expected 2 global rules, got %d", len(globalRules))
	}
	for _, r := range globalRules {
		if r.Scope != "global" {
			t.Errorf("expected scope 'global', got '%s'", r.Scope)
		}
	}

	projectRules, err := database.ListRules("project")
	if err != nil {
		t.Fatalf("ListRules(project) failed: %v", err)
	}
	if len(projectRules) != 1 {
		t.Errorf("expected 1 project rule, got %d", len(projectRules))
	}

	agentRules, err := database.ListRules("agent")
	if err != nil {
		t.Fatalf("ListRules(agent) failed: %v", err)
	}
	if len(agentRules) != 1 {
		t.Errorf("expected 1 agent rule, got %d", len(agentRules))
	}

	// List rules for nonexistent scope returns empty
	nonexistent, err := database.ListRules("nonexistent")
	if err != nil {
		t.Fatalf("ListRules(nonexistent) failed: %v", err)
	}
	if len(nonexistent) != 0 {
		t.Errorf("expected 0 rules for nonexistent scope, got %d", len(nonexistent))
	}

	// Update an existing rule via ON CONFLICT
	updated := &Rule{
		ID:       "rule-1",
		Content:  "Always respond in French",
		Scope:    "global",
		Metadata: map[string]string{"priority": "urgent"},
	}
	if err := database.SaveRule(updated); err != nil {
		t.Fatalf("failed to update rule rule-1: %v", err)
	}

	allAgain, err := database.ListRules("")
	if err != nil {
		t.Fatalf("ListRules after update failed: %v", err)
	}
	if len(allAgain) != 4 {
		t.Errorf("expected 4 rules after update, got %d", len(allAgain))
	}
	found := false
	for _, r := range allAgain {
		if r.ID == "rule-1" {
			found = true
			if r.Content != "Always respond in French" {
				t.Errorf("expected updated content 'Always respond in French', got '%s'", r.Content)
			}
			if r.Metadata["priority"] != "urgent" {
				t.Errorf("expected metadata priority 'urgent', got '%s'", r.Metadata["priority"])
			}
			break
		}
	}
	if !found {
		t.Error("updated rule rule-1 not found in list")
	}

	// Delete a rule by ID
	if err := database.DeleteRule("rule-2"); err != nil {
		t.Fatalf("DeleteRule failed: %v", err)
	}

	afterDelete, err := database.ListRules("")
	if err != nil {
		t.Fatalf("ListRules after delete failed: %v", err)
	}
	if len(afterDelete) != 3 {
		t.Errorf("expected 3 rules after deletion, got %d", len(afterDelete))
	}
	for _, r := range afterDelete {
		if r.ID == "rule-2" {
			t.Error("deleted rule rule-2 still present in list")
		}
	}

	// Deleting a non-existent ID should not error
	if err := database.DeleteRule("nonexistent-id"); err != nil {
		t.Fatalf("DeleteRule on non-existent ID should not error: %v", err)
	}

	// Verify total count is unchanged after no-op delete
	final, err := database.ListRules("")
	if err != nil {
		t.Fatalf("ListRules final failed: %v", err)
	}
	if len(final) != 3 {
		t.Errorf("expected 3 rules after no-op delete, got %d", len(final))
	}
}

func TestRuleEmptyScope(t *testing.T) {
	tempDir, err := os.MkdirTemp("", "symmemory-rule-scope-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tempDir)

	oldHome := os.Getenv("HOME")
	os.Setenv("HOME", tempDir)
	defer os.Setenv("HOME", oldHome)

	database, err := Open(config.Defaults())
	if err != nil {
		t.Fatalf("failed to open database: %v", err)
	}
	defer func() { _ = database.Close() }()

	r := &Rule{
		ID:       "empty-scope-rule",
		Content:  "Rule with empty scope still saved",
		Scope:    "",
		Metadata: map[string]string{},
	}
	if err := database.SaveRule(r); err != nil {
		t.Fatalf("failed to save rule with empty scope: %v", err)
	}

	// List with empty string should return all (including empty-scoped)
	all, err := database.ListRules("")
	if err != nil {
		t.Fatalf("ListRules failed: %v", err)
	}
	if len(all) != 1 {
		t.Errorf("expected 1 rule with empty scope, got %d", len(all))
	}

	// ListRules("") returns all regardless; scope lookup for exact empty string
	emptyScope, err := database.ListRules("")
	if err != nil {
		t.Fatalf("ListRules('') failed: %v", err)
	}
	if len(emptyScope) != 1 {
		t.Errorf("expected 1 rule when listing with empty string, got %d", len(emptyScope))
	}
}
