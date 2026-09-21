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

func TestReleaseFormatCheckBatchesLargeFileSetAndRejectsDrift(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("release workflow format check uses Bash and POSIX find")
	}

	root := releaseWorkflowRepoRoot(t)
	script := releaseWorkflowFormatScript(t, filepath.Join(root, ".github", "workflows", "release.yml"))
	fixtureRoot := t.TempDir()
	for _, dir := range []string{"long go paths", "space dir", "browse"} {
		if err := os.MkdirAll(filepath.Join(fixtureRoot, dir), 0o755); err != nil {
			t.Fatalf("create fixture directory %q: %v", dir, err)
		}
	}

	newline := "\n"
	if err := os.WriteFile(filepath.Join(fixtureRoot, "browse", "ignored file.go"), []byte("package p"+newline+newline+"func f(){}"+newline), 0o644); err != nil {
		t.Fatalf("write excluded fixture: %v", err)
	}
	driftPath := filepath.Join(fixtureRoot, "space dir", "drift file.go")
	if err := os.WriteFile(driftPath, []byte("package p"+newline), 0o644); err != nil {
		t.Fatalf("write whitespace-path fixture: %v", err)
	}

	argMax := releaseWorkflowArgMax(t)
	const filenamePrefix = "gofmt-regression-"
	longPrefix := filenamePrefix + strings.Repeat("x", 150)
	targetBytes := argMax + 128*1024
	var manifestBytes int
	var fileCount int
	for manifestBytes <= targetBytes {
		name := fmt.Sprintf("%s%08d.go", longPrefix, fileCount)
		relativePath := filepath.Join("long go paths", name)
		path := filepath.Join(fixtureRoot, relativePath)
		if err := os.WriteFile(path, []byte("package p"+newline), 0o644); err != nil {
			t.Fatalf("write generated fixture %d: %v", fileCount, err)
		}
		manifestBytes += len("./") + len(relativePath) + 1
		fileCount++
	}
	if manifestBytes <= argMax {
		t.Fatalf("fixture manifest is only %d bytes; expected more than ARG_MAX=%d", manifestBytes, argMax)
	}

	if output, err := runReleaseWorkflowFormatCheck(t, script, fixtureRoot); err != nil {
		t.Fatalf("large formatted fixture failed release format check: %v\n%s", err, output)
	}

	before, err := os.ReadFile(driftPath)
	if err != nil {
		t.Fatalf("read drift fixture before check: %v", err)
	}
	if err := os.WriteFile(driftPath, []byte("package p"+newline+newline+"func f(){}"+newline), 0o644); err != nil {
		t.Fatalf("write formatting drift: %v", err)
	}

	output, err := runReleaseWorkflowFormatCheck(t, script, fixtureRoot)
	if err == nil {
		t.Fatalf("release format check accepted formatting drift; output:\n%s", output)
	}
	expectedPath := filepath.ToSlash(filepath.Join(".", "space dir", "drift file.go"))
	if !bytes.Contains(output, []byte(expectedPath)) {
		t.Fatalf("release format check output %q does not identify %q", output, expectedPath)
	}
	after, err := os.ReadFile(driftPath)
	if err != nil {
		t.Fatalf("read drift fixture after check: %v", err)
	}
	if !bytes.Equal(after, []byte("package p"+newline+newline+"func f(){}"+newline)) || bytes.Equal(before, after) {
		t.Fatalf("release format check rewrote the drift fixture")
	}

	brokenPath := filepath.Join(fixtureRoot, "broken.go")
	if err := os.WriteFile(brokenPath, []byte("package p"+newline+"func"), 0o644); err != nil {
		t.Fatalf("write invalid fixture: %v", err)
	}
	output, err = runReleaseWorkflowFormatCheck(t, script, fixtureRoot)
	if err == nil {
		t.Fatalf("release format check accepted gofmt failure; output:\n%s", output)
	}
	if !bytes.Contains(output, []byte("::error::gofmt failed while checking a batch")) {
		t.Fatalf("release format check did not report gofmt failure: %q", output)
	}
}

func releaseWorkflowRepoRoot(t *testing.T) string {
	t.Helper()
	_, sourceFile, _, ok := runtime.Caller(0)
	if !ok {
		t.Fatal("runtime.Caller failed")
	}
	root, err := filepath.Abs(filepath.Join(filepath.Dir(sourceFile), ".."))
	if err != nil {
		t.Fatalf("resolve repository root: %v", err)
	}
	return root
}

func releaseWorkflowFormatScript(t *testing.T, workflowPath string) string {
	t.Helper()
	data, err := os.ReadFile(workflowPath)
	if err != nil {
		t.Fatalf("read release workflow: %v", err)
	}
	lines := strings.Split(strings.ReplaceAll(string(data), "\r\n", "\n"), "\n")
	formatLine := releaseWorkflowLine(t, lines, "      - name: Format check", 0)
	runLine := releaseWorkflowLine(t, lines, "        run: |", formatLine)
	vetLine := releaseWorkflowLine(t, lines, "      - name: Vet", runLine)

	const bodyIndent = "          "
	var script strings.Builder
	script.WriteString("#!/usr/bin/env bash\n")
	for _, line := range lines[runLine+1 : vetLine] {
		if line == "" {
			script.WriteByte('\n')
			continue
		}
		if !strings.HasPrefix(line, bodyIndent) {
			t.Fatalf("unexpected release format-check indentation: %q", line)
		}
		script.WriteString(strings.TrimPrefix(line, bodyIndent))
		script.WriteByte('\n')
	}

	scriptPath := filepath.Join(t.TempDir(), "release-format-check.sh")
	if err := os.WriteFile(scriptPath, []byte(script.String()), 0o755); err != nil {
		t.Fatalf("write extracted release format check: %v", err)
	}
	return scriptPath
}

func releaseWorkflowLine(t *testing.T, lines []string, wanted string, start int) int {
	t.Helper()
	for index := start; index < len(lines); index++ {
		if lines[index] == wanted {
			return index
		}
	}
	t.Fatalf("release workflow line %q not found after line %d", wanted, start)
	return -1
}

func runReleaseWorkflowFormatCheck(t *testing.T, scriptPath, workingDir string) ([]byte, error) {
	t.Helper()
	runnerTemp := filepath.Join(workingDir, "runner-temp")
	if err := os.MkdirAll(runnerTemp, 0o755); err != nil {
		t.Fatalf("create RUNNER_TEMP: %v", err)
	}

	env := make([]string, 0, len(os.Environ())+1)
	for _, variable := range os.Environ() {
		if strings.HasPrefix(variable, "RUNNER_TEMP=") {
			continue
		}
		env = append(env, variable)
	}
	env = append(env, "RUNNER_TEMP="+runnerTemp)

	command := exec.Command("bash", scriptPath)
	command.Dir = workingDir
	command.Env = env
	return command.CombinedOutput()
}

func releaseWorkflowArgMax(t *testing.T) int {
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
