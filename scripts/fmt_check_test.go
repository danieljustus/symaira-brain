package main

import (
	"bytes"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
	"testing"
)

func TestFmtCheckBatchesLargeFileSetAndRejectsDrift(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("fmt-check uses the POSIX find and make available on macOS and CI")
	}

	_, sourceFile, _, ok := runtime.Caller(0)
	if !ok {
		t.Fatal("runtime.Caller failed")
	}
	repoRoot, err := filepath.Abs(filepath.Join(filepath.Dir(sourceFile), ".."))
	if err != nil {
		t.Fatalf("resolve repository root: %v", err)
	}
	makefile := filepath.Join(repoRoot, "Makefile")
	if _, err := os.Stat(makefile); err != nil {
		t.Fatalf("stat Makefile: %v", err)
	}

	fixtureRoot := t.TempDir()
	fixtureDir := filepath.Join(fixtureRoot, "long-go-file-paths")
	if err := os.MkdirAll(fixtureDir, 0o755); err != nil {
		t.Fatalf("create fixture directory: %v", err)
	}
	if err := os.WriteFile(filepath.Join(fixtureRoot, "go.mod"), []byte("module p\n\ngo 1.26\n"), 0o644); err != nil {
		t.Fatalf("write fixture go.mod: %v", err)
	}

	argMax := systemArgMax(t)
	targetBytes := argMax + 128*1024
	const filenamePrefix = "gofmt-regression-"
	longPrefix := filenamePrefix + strings.Repeat("x", 160)
	var inputBytes int
	var driftPath string
	for index := 0; inputBytes <= targetBytes; index++ {
		name := fmt.Sprintf("%s-%08d.go", longPrefix, index)
		relativePath := filepath.Join("long-go-file-paths", name)
		path := filepath.Join(fixtureRoot, relativePath)
		if err := os.WriteFile(path, []byte("package p\n"), 0o644); err != nil {
			t.Fatalf("write fixture %d: %v", index, err)
		}
		inputBytes += len("./") + len(relativePath) + 1
		driftPath = path
	}
	if inputBytes <= argMax {
		t.Fatalf("fixture manifest is only %d bytes; expected more than ARG_MAX=%d", inputBytes, argMax)
	}

	if output, err := runFmtCheck(t, fixtureRoot, makefile); err != nil {
		t.Fatalf("large formatted fixture failed fmt-check: %v\n%s", err, output)
	}

	before, err := os.ReadFile(driftPath)
	if err != nil {
		t.Fatalf("read drift fixture before check: %v", err)
	}
	if err := os.WriteFile(driftPath, []byte("package p\n\nfunc f(){}\n"), 0o644); err != nil {
		t.Fatalf("write formatting drift: %v", err)
	}

	output, err := runFmtCheck(t, fixtureRoot, makefile)
	if err == nil {
		t.Fatalf("fmt-check accepted formatting drift; output:\n%s", output)
	}
	expectedPath := filepath.ToSlash(filepath.Join(".", "long-go-file-paths", filepath.Base(driftPath)))
	if !bytes.Contains(output, []byte(expectedPath)) {
		t.Fatalf("fmt-check output %q does not identify %q", output, expectedPath)
	}
	after, err := os.ReadFile(driftPath)
	if err != nil {
		t.Fatalf("read drift fixture after check: %v", err)
	}
	if !bytes.Equal(after, []byte("package p\n\nfunc f(){}\n")) || bytes.Equal(before, after) {
		t.Fatalf("fmt-check rewrote the drift fixture")
	}
}

func runFmtCheck(t *testing.T, workingDir, makefile string) ([]byte, error) {
	t.Helper()
	command := exec.Command("make", "-f", makefile, "fmt-check")
	command.Dir = workingDir
	return command.CombinedOutput()
}

func systemArgMax(t *testing.T) int {
	t.Helper()
	output, err := exec.Command("getconf", "ARG_MAX").Output()
	if err != nil {
		t.Logf("getconf ARG_MAX unavailable; using 1 MiB fallback: %v", err)
		return 1 << 20
	}
	argMax, err := strconv.Atoi(strings.TrimSpace(string(output)))
	if err != nil || argMax <= 0 {
		t.Logf("invalid getconf ARG_MAX output %q; using 1 MiB fallback", output)
		return 1 << 20
	}
	return argMax
}
