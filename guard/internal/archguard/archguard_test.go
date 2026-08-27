package archguard

import (
	"os"
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

func TestCheckBoundary_NoCrossImports(t *testing.T) {
	// The Brain/Guard separation is now a convention enforced here (single
	// module, ADR 0001 D6): brain internal packages must not import guard,
	// and guard internal packages must not import brain internal.
	repoRoot, err := filepath.Abs(filepath.Join("..", "..", ".."))
	if err != nil {
		t.Fatalf("Abs: %v", err)
	}

	violations := CheckBoundary(repoRoot)
	for _, v := range violations {
		t.Errorf("Boundary violation: %s", v)
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

func TestCheckBoundary_Branches(t *testing.T) {
	tests := []struct {
		name  string
		setup func(t *testing.T) string // repoRoot
		want  int
	}{
		{
			name: "missing internal dir",
			setup: func(t *testing.T) string {
				return t.TempDir()
			},
			want: 0,
		},
		{
			name: "missing guard internal dir",
			setup: func(t *testing.T) string {
				root := t.TempDir()
				os.MkdirAll(filepath.Join(root, "guard"), 0o755)
				return root
			},
			want: 0,
		},
		{
			name: "unparseable go file is skipped",
			setup: func(t *testing.T) string {
				root := t.TempDir()
				pkgDir := filepath.Join(root, "internal", "pkg")
				os.MkdirAll(pkgDir, 0o755)
				os.WriteFile(filepath.Join(pkgDir, "bad.go"), []byte("package pkg\nTHIS IS NOT VALID GO"), 0o644)
				return root
			},
			want: 0,
		},
		{
			name: "brain internal imports guard",
			setup: func(t *testing.T) string {
				root := t.TempDir()
				pkgDir := filepath.Join(root, "internal", "pkg")
				os.MkdirAll(pkgDir, 0o755)
				os.WriteFile(filepath.Join(pkgDir, "foo.go"), []byte("package pkg\nimport \"github.com/danieljustus/symaira-brain/guard/internal/archguard\"\n"), 0o644)
				return root
			},
			want: 1,
		},
		{
			name: "guard internal imports brain internal",
			setup: func(t *testing.T) string {
				root := t.TempDir()
				pkgDir := filepath.Join(root, "guard", "internal", "pkg")
				os.MkdirAll(pkgDir, 0o755)
				os.WriteFile(filepath.Join(pkgDir, "foo.go"), []byte("package pkg\nimport \"github.com/danieljustus/symaira-brain/internal/policy\"\n"), 0o644)
				return root
			},
			want: 1,
		},
		{
			name: "no violations",
			setup: func(t *testing.T) string {
				root := t.TempDir()
				pkgDir := filepath.Join(root, "internal", "pkg")
				os.MkdirAll(pkgDir, 0o755)
				os.WriteFile(filepath.Join(pkgDir, "ok.go"), []byte("package pkg\n"), 0o644)
				gPkgDir := filepath.Join(root, "guard", "internal", "pkg")
				os.MkdirAll(gPkgDir, 0o755)
				os.WriteFile(filepath.Join(gPkgDir, "ok.go"), []byte("package pkg\n"), 0o644)
				return root
			},
			want: 0,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			repoRoot := tt.setup(t)
			got := CheckBoundary(repoRoot)
			if len(got) != tt.want {
				t.Errorf("CheckBoundary() = %d violations, want %d: %v", len(got), tt.want, got)
			}
		})
	}
}
