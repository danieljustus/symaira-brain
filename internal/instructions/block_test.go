package instructions

import (
	"os"
	"path/filepath"
	"testing"
)

func TestRender_Idempotent(t *testing.T) {
	content := "some managed content\nwith multiple lines\n"

	// First render into an empty target.
	step1 := Render("", content)
	step2 := Render(step1, content)
	step3 := Render(step2, content)

	if step1 != step2 {
		t.Errorf("first render != second render\n--- got ---\n%s\n--- want ---\n%s", step1, step2)
	}
	if step2 != step3 {
		t.Errorf("second render != third render\n--- got ---\n%s\n--- want ---\n%s", step2, step3)
	}
}

func TestRender_PreservesContentOutsideMarkers(t *testing.T) {
	content := "new managed content\n"
	existing := "# My Config\n\nSome user text.\n" + BeginMarker + "\nold content\n" + EndMarker + "\n\nFooter here.\n"

	got := Render(existing, content)

	want := "# My Config\n\nSome user text.\n" + BeginMarker + "\n" + content + EndMarker + "\n\nFooter here.\n"
	if got != want {
		t.Errorf("Render() did not preserve surrounding content\n--- got ---\n%s\n--- want ---\n%s", got, want)
	}
}

func TestRender_AppendsWhenNoMarkers(t *testing.T) {
	content := "block content\n"
	existing := "# Header\n\nSome content.\n"

	got := Render(existing, content)
	want := "# Header\n\nSome content.\n" + BeginMarker + "\n" + content + EndMarker + "\n"
	if got != want {
		t.Errorf("Render() did not append block correctly\n--- got ---\n%s\n--- want ---\n%s", got, want)
	}
}

func TestRender_AppendsWithoutTrailingNewline(t *testing.T) {
	content := "block\n"
	existing := "no trailing newline"

	got := Render(existing, content)
	want := "no trailing newline\n" + BeginMarker + "\n" + content + EndMarker + "\n"
	if got != want {
		t.Errorf("Render() did not handle missing trailing newline\n--- got ---\n%s\n--- want ---\n%s", got, want)
	}
}

func TestRender_EmptyTarget(t *testing.T) {
	content := "hello\n"
	got := Render("", content)
	want := BeginMarker + "\n" + content + EndMarker + "\n"
	if got != want {
		t.Errorf("Render() empty target\n--- got ---\n%s\n--- want ---\n%s", got, want)
	}
}

func TestRender_ReplaceExistingBlock(t *testing.T) {
	content := "updated content\n"
	existing := "before\n" + BeginMarker + "\nold\n" + EndMarker + "\nafter\n"

	got := Render(existing, content)
	want := "before\n" + BeginMarker + "\n" + content + EndMarker + "\nafter\n"
	if got != want {
		t.Errorf("Render() did not replace existing block\n--- got ---\n%s\n--- want ---\n%s", got, want)
	}
}

func TestRender_BeginMarkerWithoutEnd(t *testing.T) {
	content := "new\n"
	existing := "before\n" + BeginMarker + "\nold stuff\n"

	got := Render(existing, content)
	want := "before\n" + BeginMarker + "\n" + content + EndMarker + "\n"
	if got != want {
		t.Errorf("Render() should handle begin without end\n--- got ---\n%s\n--- want ---\n%s", got, want)
	}
}

func TestRender_CRLF(t *testing.T) {
	content := "content\r\n"
	existing := "before\r\n" + BeginMarker + "\r\nold\r\n" + EndMarker + "\r\nafter\r\n"

	got := Render(existing, content)
	want := "before\r\n" + BeginMarker + "\n" + content + EndMarker + "\r\nafter\r\n"
	if got != want {
		t.Errorf("Render() CRLF handling\n--- got ---\n%s\n--- want ---\n%s", got, want)
	}
}

func TestRender_CRLFIdempotent(t *testing.T) {
	content := "content\r\n"
	step1 := Render("", content)
	step2 := Render(step1, content)

	if step1 != step2 {
		t.Errorf("CRLF idempotency failed\n--- got ---\n%s\n--- want ---\n%s", step1, step2)
	}
}

func TestRender_MultipleBlocksPreservesLast(t *testing.T) {
	content := "final\n"
	// Simulate a file with duplicate begin markers (malformed but possible).
	existing := BeginMarker + "\nfirst\n" + EndMarker + "\n" + BeginMarker + "\nsecond\n" + EndMarker + "\n"

	got := Render(existing, content)
	// Only the first occurrence is replaced.
	want := BeginMarker + "\n" + content + EndMarker + "\n" + BeginMarker + "\nsecond\n" + EndMarker + "\n"
	if got != want {
		t.Errorf("Render() with duplicate markers\n--- got ---\n%s\n--- want ---\n%s", got, want)
	}
}

func TestRender_GoldenFiles(t *testing.T) {
	tests := []struct {
		name     string
		content  string
		existing string
		golden   string
	}{
		{
			name:     "fresh_file",
			content:  "Instructions content.\nMore lines.\n",
			existing: "",
			golden:   "testdata/block/fresh_file.golden",
		},
		{
			name:     "existing_with_user_content",
			content:  "Updated managed block.\n",
			existing: "# User Header\n\nUser content before block.\n" + BeginMarker + "\nOld content.\n" + EndMarker + "\n\nUser footer.\n",
			golden:   "testdata/block/existing_with_user_content.golden",
		},
		{
			name:     "file_without_markers",
			content:  "Managed block added.\n",
			existing: "# My Config\n\nSome config values.\n",
			golden:   "testdata/block/file_without_markers.golden",
		},
		{
			name:     "crlf_file",
			content:  "content\r\n",
			existing: "header\r\n" + BeginMarker + "\r\nold\r\n" + EndMarker + "\r\nfooter\r\n",
			golden:   "testdata/block/crlf_file.golden",
		},
		{
			name:     "no_trailing_newline",
			content:  "block\n",
			existing: "no-newline",
			golden:   "testdata/block/no_trailing_newline.golden",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := Render(tt.existing, tt.content)

			// Write golden if it doesn't exist yet.
			if _, err := os.Stat(tt.golden); os.IsNotExist(err) {
				if err := os.MkdirAll(filepath.Dir(tt.golden), 0o700); err != nil {
					t.Fatal(err)
				}
				if err := os.WriteFile(tt.golden, []byte(got), 0o644); err != nil {
					t.Fatal(err)
				}
				t.Logf("golden file created: %s", tt.golden)
				return
			}

			want, err := os.ReadFile(tt.golden)
			if err != nil {
				t.Fatal(err)
			}
			if got != string(want) {
				t.Errorf("Render() golden mismatch\n--- got ---\n%s\n--- want (golden) ---\n%s", got, string(want))
			}
		})
	}
}

func TestSource_Content_GlobalOnly(t *testing.T) {
	dir := t.TempDir()

	// Write a global instructions file.
	globalDir := filepath.Join(dir, "config", "symbrain")
	if err := os.MkdirAll(globalDir, 0o700); err != nil {
		t.Fatal(err)
	}
	globalPath := filepath.Join(globalDir, GlobalFileName)
	if err := os.WriteFile(globalPath, []byte("global instructions\n"), 0o600); err != nil {
		t.Fatal(err)
	}

	s := &Source{
		GlobalPath: globalPath,
	}

	got, err := s.Content()
	if err != nil {
		t.Fatalf("Content() error: %v", err)
	}
	want := "global instructions\n"
	if got != want {
		t.Errorf("Content() = %q, want %q", got, want)
	}
}

func TestSource_Content_ProjectAppendedAfterGlobal(t *testing.T) {
	dir := t.TempDir()

	globalDir := filepath.Join(dir, "config", "symbrain")
	if err := os.MkdirAll(globalDir, 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(globalDir, GlobalFileName), []byte("global\n"), 0o600); err != nil {
		t.Fatal(err)
	}

	projectDir := filepath.Join(dir, "project", ProjectDirName)
	if err := os.MkdirAll(projectDir, 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(projectDir, ProjectFileName), []byte("project\n"), 0o600); err != nil {
		t.Fatal(err)
	}

	s := &Source{
		GlobalPath:  filepath.Join(globalDir, GlobalFileName),
		ProjectPath: filepath.Join(projectDir, ProjectFileName),
	}

	got, err := s.Content()
	if err != nil {
		t.Fatalf("Content() error: %v", err)
	}
	want := "global\nproject\n"
	if got != want {
		t.Errorf("Content() = %q, want %q", got, want)
	}
}

func TestSource_Content_NeitherExists(t *testing.T) {
	s := &Source{
		GlobalPath:  "/nonexistent/global/instructions.md",
		ProjectPath: "/nonexistent/project/instructions.md",
	}

	got, err := s.Content()
	if err != nil {
		t.Fatalf("Content() error: %v", err)
	}
	if got != "" {
		t.Errorf("Content() = %q, want empty", got)
	}
}

func TestSource_Content_ProjectOnly(t *testing.T) {
	dir := t.TempDir()

	projectDir := filepath.Join(dir, "project", ProjectDirName)
	if err := os.MkdirAll(projectDir, 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(projectDir, ProjectFileName), []byte("project only\n"), 0o600); err != nil {
		t.Fatal(err)
	}

	s := &Source{
		GlobalPath:  "/nonexistent/global/instructions.md",
		ProjectPath: filepath.Join(projectDir, ProjectFileName),
	}

	got, err := s.Content()
	if err != nil {
		t.Fatalf("Content() error: %v", err)
	}
	want := "project only\n"
	if got != want {
		t.Errorf("Content() = %q, want %q", got, want)
	}
}

func TestNewSource(t *testing.T) {
	sNoProj := NewSource("")
	if sNoProj.GlobalPath == "" {
		t.Error("expected non-empty GlobalPath")
	}
	if sNoProj.ProjectPath != "" {
		t.Errorf("expected empty ProjectPath, got %q", sNoProj.ProjectPath)
	}

	projDir := filepath.Join("some", "project")
	sProj := NewSource(projDir)
	wantProjPath := filepath.Join(projDir, ProjectDirName, ProjectFileName)
	if sProj.ProjectPath != wantProjPath {
		t.Errorf("ProjectPath = %q, want %q", sProj.ProjectPath, wantProjPath)
	}
}

