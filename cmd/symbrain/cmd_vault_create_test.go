package main

import (
	"bytes"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestCmdVaultCreateReadsValueFromStdinAndSanitizesConfirmation(t *testing.T) {
	dir := t.TempDir()
	state := filepath.Join(dir, "stdin")
	fake := filepath.Join(dir, "symvault")
	script := "#!/bin/sh\n" +
		"if [ \"$1\" = add ]; then cat > \"$STATE\"; exit 0; fi\n" +
		"if [ \"$1\" = get ]; then printf '%s' '{\"path\":\"work/new\",\"fields\":{\"password\":\"hidden-value\"}}'; exit 0; fi\n" +
		"exit 1\n"
	if err := os.WriteFile(fake, []byte(script), 0o755); err != nil {
		t.Fatal(err)
	}
	t.Setenv("PATH", dir+string(os.PathListSeparator)+os.Getenv("PATH"))
	t.Setenv("STATE", state)
	oldStdin := os.Stdin
	input, err := os.Open(filepath.Join(dir, "input"))
	if err != nil && !os.IsNotExist(err) {
		t.Fatal(err)
	}
	if input != nil {
		input.Close()
	}
	inputPath := filepath.Join(dir, "input")
	if err := os.WriteFile(inputPath, []byte("hidden-value\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	input, err = os.Open(inputPath)
	if err != nil {
		t.Fatal(err)
	}
	os.Stdin = input
	t.Cleanup(func() { os.Stdin = oldStdin; input.Close() })

	var stdout, stderr bytes.Buffer
	code := cmdVaultCreate([]string{"work/new"}, &stdout, &stderr)
	if code != 0 {
		t.Fatalf("code=%v stderr=%q", code, stderr.String())
	}
	out := stdout.String()
	if strings.Contains(out, "hidden-value") || strings.Contains(stderr.String(), "hidden-value") {
		t.Fatal("secret leaked in output")
	}
	if !strings.Contains(out, `"submitted":{"path":"work/new"}`) || !strings.Contains(out, `"confirmed":{"field_count":1,"has_value":true,"path":"work/new"}`) {
		t.Fatalf("sanitized confirmation=%q", out)
	}
	if got, err := os.ReadFile(state); err != nil || string(got) != "hidden-value\n" {
		t.Fatalf("stdin not forwarded: %q %v", got, err)
	}
}
