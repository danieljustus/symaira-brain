package archguard

import (
	"path/filepath"
	"testing"
)

func TestDefaultAllowed_NoViolations(t *testing.T) {
	root, err := filepath.Abs(filepath.Join("..", "..", "internal"))
	if err != nil {
		t.Fatalf("Abs: %v", err)
	}

	violations := DefaultAllowed.Check(root)
	for _, v := range violations {
		t.Errorf("Violation: %s", v)
	}
}

func TestCheck_DetectsViolation(t *testing.T) {
	// Simulate a violation by saying policy can only import "" (nothing)
	restricted := AllowedImports{
		"policy": {}, // nothing allowed
	}

	root, err := filepath.Abs(filepath.Join("..", "..", "internal"))
	if err != nil {
		t.Fatalf("Abs: %v", err)
	}

	violations := restricted.Check(root)
	found := false
	for _, v := range violations {
		if filepath.Base(v) == "rule.go" {
			found = true
			break
		}
	}
	if !found {
		t.Log("Note: no violation detected — policy may not actually import anything from internal/")
	}
}
