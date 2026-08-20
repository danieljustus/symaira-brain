package security

import (
	"os"
	"path/filepath"
	"testing"
)

func TestValidateScope(t *testing.T) {
	tests := []struct {
		name    string
		scope   string
		wantErr bool
	}{
		{"valid global", "global", false},
		{"valid project", "project", false},
		{"valid agent", "agent", false},
		{"valid user", "user", false},
		{"valid session", "session", false},
		{"empty string", "", false},
		{"invalid scope", "invalid", true},
		{"random string", "foobar", true},
		{"uppercase", "GLOBAL", true},
		{"mixed case", "Project", true},
		{"whitespace", " agent ", true},
		{"known misspelling", "projct", true},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			err := ValidateScope(tt.scope)
			if (err != nil) != tt.wantErr {
				t.Errorf("ValidateScope(%q) error = %v, wantErr = %v", tt.scope, err, tt.wantErr)
			}
		})
	}
}

func TestValidateScope_AllValidScopes(t *testing.T) {
	for scope := range ValidScopes {
		t.Run(scope, func(t *testing.T) {
			if err := ValidateScope(scope); err != nil {
				t.Errorf("ValidateScope(%q) returned unexpected error: %v", scope, err)
			}
		})
	}
}

func TestDetectActiveProject_SymmemoryTomlBoundary(t *testing.T) {
	detector := NewProjectScopeDetector()

	tempDir := t.TempDir()
	projectRoot := filepath.Join(tempDir, "symaira-test-repo")
	nestedFolder := filepath.Join(projectRoot, "internal", "nested", "src")

	if err := os.MkdirAll(nestedFolder, 0755); err != nil {
		t.Fatalf("failed to create nested subfolder structure: %v", err)
	}

	tomlFile := filepath.Join(projectRoot, ".symmemory.toml")
	if err := os.WriteFile(tomlFile, []byte("# Mock project configuration"), 0644); err != nil {
		t.Fatalf("failed to write mock .symmemory.toml: %v", err)
	}

	oldCwd, err := os.Getwd()
	if err != nil {
		t.Fatalf("failed to get current working directory: %v", err)
	}
	defer os.Chdir(oldCwd)

	if err := os.Chdir(nestedFolder); err != nil {
		t.Fatalf("failed to chdir to nested subfolder: %v", err)
	}

	detectedProject := detector.DetectActiveProject()
	if detectedProject != "symaira-test-repo" {
		t.Errorf("expected detected project 'symaira-test-repo', got '%s'", detectedProject)
	}
}

func TestDetectActiveProject_GitBoundary(t *testing.T) {
	detector := NewProjectScopeDetector()

	tempDir := t.TempDir()
	projectRoot := filepath.Join(tempDir, "my-project")
	deepDir := filepath.Join(projectRoot, "a", "b", "c")

	if err := os.MkdirAll(deepDir, 0755); err != nil {
		t.Fatalf("failed to create deep directory: %v", err)
	}

	gitDir := filepath.Join(projectRoot, ".git")
	if err := os.MkdirAll(gitDir, 0755); err != nil {
		t.Fatalf("failed to create .git directory: %v", err)
	}

	oldCwd, err := os.Getwd()
	if err != nil {
		t.Fatalf("failed to get current working directory: %v", err)
	}
	defer os.Chdir(oldCwd)

	if err := os.Chdir(deepDir); err != nil {
		t.Fatalf("failed to chdir to deep directory: %v", err)
	}

	detected := detector.DetectActiveProject()
	if detected != "my-project" {
		t.Errorf("expected 'my-project', got '%s'", detected)
	}
}

func TestDetectActiveProject_SymmemoryTomlPreferredOverGit(t *testing.T) {
	detector := NewProjectScopeDetector()

	tempDir := t.TempDir()
	projectRoot := filepath.Join(tempDir, "preferred-project")
	subDir := filepath.Join(projectRoot, "sub")

	if err := os.MkdirAll(subDir, 0755); err != nil {
		t.Fatalf("failed to create sub directory: %v", err)
	}

	// Both boundaries present
	tomlFile := filepath.Join(projectRoot, ".symmemory.toml")
	if err := os.WriteFile(tomlFile, []byte("# config"), 0644); err != nil {
		t.Fatalf("failed to write .symmemory.toml: %v", err)
	}
	gitDir := filepath.Join(projectRoot, ".git")
	if err := os.MkdirAll(gitDir, 0755); err != nil {
		t.Fatalf("failed to create .git: %v", err)
	}

	oldCwd, err := os.Getwd()
	if err != nil {
		t.Fatalf("failed to get current working directory: %v", err)
	}
	defer os.Chdir(oldCwd)

	if err := os.Chdir(subDir); err != nil {
		t.Fatalf("failed to chdir: %v", err)
	}

	detected := detector.DetectActiveProject()
	// .symmemory.toml is checked first, so it should win
	if detected != "preferred-project" {
		t.Errorf("expected 'preferred-project', got '%s'", detected)
	}
}

func TestDetectActiveProject_NoBoundary(t *testing.T) {
	detector := ProjectScopeDetector{}

	tempDir := t.TempDir()
	emptyDir := filepath.Join(tempDir, "empty-folder")
	if err := os.MkdirAll(emptyDir, 0755); err != nil {
		t.Fatalf("failed to create empty directory: %v", err)
	}

	oldCwd, err := os.Getwd()
	if err != nil {
		t.Fatalf("failed to get current working directory: %v", err)
	}
	defer os.Chdir(oldCwd)

	if err := os.Chdir(emptyDir); err != nil {
		t.Fatalf("failed to chdir: %v", err)
	}

	detected := detector.DetectActiveProject()
	if detected != "empty-folder" {
		t.Errorf("expected fallback to 'empty-folder', got '%s'", detected)
	}
}

func TestDetectActiveProject_RootLevelCall(t *testing.T) {
	detector := NewProjectScopeDetector()
	active := detector.DetectActiveProject()
	if active == "" {
		t.Errorf("DetectActiveProject returned empty project name")
	}
}
