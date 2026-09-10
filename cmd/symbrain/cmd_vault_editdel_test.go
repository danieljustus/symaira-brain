package main

import (
	"bytes"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestCmdVaultSetUsesStdinAndSanitizesConfirmation(t *testing.T) {
	dir := t.TempDir()
	fake := filepath.Join(dir, "symvault")
	script := "#!/bin/sh\n" +
		"if [ \"$1\" = set ]; then cat > \"$STATE\"; exit 0; fi\n" +
		"if [ \"$1\" = get ]; then printf '%s' '{\"path\":\"work/edit\",\"fields\":{\"password\":\"hidden\"}}'; exit 0; fi\n" +
		"exit 1\n"
	if err := os.WriteFile(fake, []byte(script), 0o755); err != nil {
		t.Fatal(err)
	}
	state := filepath.Join(dir, "value")
	t.Setenv("PATH", dir+string(os.PathListSeparator)+os.Getenv("PATH"))
	t.Setenv("STATE", state)
	old := os.Stdin
	inputPath := filepath.Join(dir, "input")
	if err := os.WriteFile(inputPath, []byte("new-secret\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	input, err := os.Open(inputPath)
	if err != nil {
		t.Fatal(err)
	}
	os.Stdin = input
	t.Cleanup(func() { os.Stdin = old; input.Close() })
	var stdout, stderr bytes.Buffer
	if code := cmdVaultSet([]string{"work/edit"}, &stdout, &stderr); code != 0 {
		t.Fatalf("code=%v stderr=%q", code, stderr.String())
	}
	if strings.Contains(stdout.String(), "hidden") || strings.Contains(stdout.String(), "new-secret") {
		t.Fatalf("secret leaked: %q", stdout.String())
	}
	if got, err := os.ReadFile(state); err != nil || string(got) != "new-secret\n" {
		t.Fatalf("stdin not forwarded: %q %v", got, err)
	}
	if !strings.Contains(stdout.String(), `"submitted":{"path":"work/edit"}`) {
		t.Fatalf("missing submitted metadata: %q", stdout.String())
	}
}

func TestCmdVaultDeleteRequiresYesAndConfirmsAbsence(t *testing.T) {
	dir := t.TempDir()
	fake := filepath.Join(dir, "symvault")
	state := filepath.Join(dir, "exists")
	script := "#!/bin/sh\n" +
		"if [ \"$1\" = delete ]; then [ \"$3\" = --yes ] || exit 2; rm -f \"$STATE\"; exit 0; fi\n" +
		"if [ \"$1\" = get ]; then [ -f \"$STATE\" ] && printf '%s' '{\"path\":\"work/delete\",\"fields\":{\"password\":\"hidden\"}}' && exit 0; exit 2; fi\n" +
		"exit 1\n"
	if err := os.WriteFile(fake, []byte(script), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(state, []byte("present"), 0o600); err != nil {
		t.Fatal(err)
	}
	t.Setenv("PATH", dir+string(os.PathListSeparator)+os.Getenv("PATH"))
	t.Setenv("STATE", state)
	var stdout, stderr bytes.Buffer
	if code := cmdVaultDelete([]string{"work/delete"}, &stdout, &stderr); code == 0 {
		t.Fatal("delete without --yes succeeded")
	}
	stdout.Reset()
	stderr.Reset()
	if code := cmdVaultDelete([]string{"work/delete", "--yes"}, &stdout, &stderr); code != 0 {
		t.Fatalf("code=%v stderr=%q", code, stderr.String())
	}
	if _, err := os.Stat(state); !os.IsNotExist(err) {
		t.Fatalf("state remains: %v", err)
	}
	if !strings.Contains(stdout.String(), `"confirmed":{"absent":true,"path":"work/delete"}`) {
		t.Fatalf("missing confirmation: %q", stdout.String())
	}
}

func TestCmdVaultDeleteReportsUnverifiedWhenGetFailsOtherwise(t *testing.T) {
	dir := t.TempDir()
	fake := filepath.Join(dir, "symvault")
	// delete succeeds, but the confirmation read fails with a generic error
	// (exit 1, e.g. vault locked) — absence must NOT be claimed.
	script := "#!/bin/sh\n" +
		"if [ \"$1\" = delete ]; then exit 0; fi\n" +
		"if [ \"$1\" = get ]; then exit 1; fi\n" +
		"exit 1\n"
	if err := os.WriteFile(fake, []byte(script), 0o755); err != nil {
		t.Fatal(err)
	}
	t.Setenv("PATH", dir+string(os.PathListSeparator)+os.Getenv("PATH"))
	var stdout, stderr bytes.Buffer
	if code := cmdVaultDelete([]string{"work/delete", "--yes"}, &stdout, &stderr); code == 0 {
		t.Fatal("unverified delete reported success")
	}
	if !strings.Contains(stderr.String(), "absence verification failed") {
		t.Fatalf("missing unverified report: %q", stderr.String())
	}
	if strings.Contains(stdout.String(), "absent") {
		t.Fatalf("absence claimed without proof: %q", stdout.String())
	}
}
