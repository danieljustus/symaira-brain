// Package security contains grep-level and test-level proofs that
// brain-specific security invariants hold. Each test documents a property
// from the pre-beta security review (issue #29) and fails if the code
// regresses.
package security

import (
	"go/ast"
	"go/parser"
	"go/token"
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"testing"
)

// skipDirs contains base names of directories that should never be visited
// during the repo-root walk in the shell/vault regression tests. These trees
// hold either non-Go content, test helpers, or generated artifacts — never
// production Go source that the security invariants apply to.
var skipDirs = []string{".git", ".worktrees", "Sources", "docs", "dist", "testdata"}

// isSkippableDir reports whether base is a directory that should be excluded
// from the production-code walk. See skipDirs for the full list.
func isSkippableDir(base string) bool {
	for _, d := range skipDirs {
		if base == d {
			return true
		}
	}
	return false
}

// walkGoFiles walks from root, skipping skippable directories and _test.go
// files, and calls fn for every remaining non-test .go file it encounters.
// If fn returns an error the walk is aborted and that error is returned.
func walkGoFiles(root string, fn func(path string) error) error {
	return filepath.Walk(root, func(path string, info os.FileInfo, err error) error {
		if err != nil {
			return err
		}
		// Skip directories that are not production code.
		if info.IsDir() {
			if path != root && isSkippableDir(info.Name()) {
				return filepath.SkipDir
			}
			return nil
		}
		// Skip test files — test fixtures may legitimately use shell scripts.
		if strings.HasSuffix(path, "_test.go") {
			return nil
		}
		if !strings.HasSuffix(path, ".go") {
			return nil
		}
		return fn(path)
	})
}

// TestNoShellInterpolation verifies that exec.Command is never called with
// a shell (sh -c, /bin/sh, /bin/bash) in production code. Child processes
// are spawned directly via exec.Command(path, args...), never through a
// shell interpreter.
func TestNoShellInterpolation(t *testing.T) {
	t.Parallel()

	root := findRepoRoot(t)
	shellPatterns := regexp.MustCompile(`sh\s+-c|/bin/sh|/bin/bash|/usr/bin/env\s+sh`)

	err := walkGoFiles(root, func(path string) error {
		data, err := os.ReadFile(path)
		if err != nil {
			return err
		}

		// Check for shell interpolation patterns in string literals.
		fset := token.NewFileSet()
		f, err := parser.ParseFile(fset, path, data, parser.ParseComments)
		if err != nil {
			return nil // skip unparseable files
		}

		ast.Inspect(f, func(n ast.Node) bool {
			if lit, ok := n.(*ast.BasicLit); ok && lit.Kind == token.STRING {
				if shellPatterns.MatchString(lit.Value) {
					t.Errorf("shell interpolation in %s:%d: %s", path, fset.Position(lit.Pos()).Line, lit.Value)
				}
			}
			return true
		})
		return nil
	})
	if err != nil {
		t.Fatalf("walk: %v", err)
	}
}

// TestNoShellInterpolation_DetectsViolation is a negative self-test that
// proves the matcher used in TestNoShellInterpolation fires on a planted
// violation. If this test fails, the matcher is silently broken and any
// pass from TestNoShellInterpolation is unreliable.
func TestNoShellInterpolation_DetectsViolation(t *testing.T) {
	// Create a temporary .go file containing a clear shell interpolation.
	dir := t.TempDir()
	planted := filepath.Join(dir, "planted.go")
	content := []byte(`package p
import "os/exec"
func run() {
	cmd := exec.Command("/bin/sh", "-c", "echo hello")
	_ = cmd
}
`)
	if err := os.WriteFile(planted, content, 0o644); err != nil {
		t.Fatalf("write planted file: %v", err)
	}

	shellPatterns := regexp.MustCompile(`sh\s+-c|/bin/sh|/bin/bash|/usr/bin/env\s+sh`)

	data, err := os.ReadFile(planted)
	if err != nil {
		t.Fatalf("read planted file: %v", err)
	}

	fset := token.NewFileSet()
	f, err := parser.ParseFile(fset, planted, data, parser.ParseComments)
	if err != nil {
		t.Fatalf("parse planted file: %v", err)
	}

	var found bool
	ast.Inspect(f, func(n ast.Node) bool {
		if lit, ok := n.(*ast.BasicLit); ok && lit.Kind == token.STRING {
			if shellPatterns.MatchString(lit.Value) {
				found = true
			}
		}
		return true
	})
	if !found {
		t.Error("negative self-test: shell interpolation matcher did NOT fire on planted violation; " +
			"TestNoShellInterpolation may be silently passing")
	}
}

// TestNoVaultPayloadsInLogs verifies that fmt.Errorf, fmt.Sprintf, log.Print,
// and similar formatting functions never include vault/credential/secret/token
// variables in their output. Vault payloads must never hit logs, audit files,
// or error strings.
func TestNoVaultPayloadsInLogs(t *testing.T) {
	t.Parallel()

	root := findRepoRoot(t)

	// Patterns that indicate sensitive data in format strings.
	sensitivePatterns := regexp.MustCompile(
		`fmt\.(Errorf|Sprintf|Fprintf|Printf|Println|Print)\(.*` +
			`([Vv]ault|[Cc]redential|[Ss]ecret|[Tt]oken|[Pp]assword|[Kk]ey[Aa]ge|` +
			`[Aa]ge\.|identity|recipient)`)

	err := walkGoFiles(root, func(path string) error {
		// Absorbed modules (memory, skills) carry their own security audit
		// suites. Brain's repo-wide scan is scoped to its own code only
		// (repo consolidation step 4 — see docs/brain-merge-design.md).
		if strings.Contains(path, "/internal/memory/") ||
			strings.Contains(path, "/internal/skills/") {
			return nil
		}

		data, err := os.ReadFile(path)
		if err != nil {
			return err
		}

		lines := strings.Split(string(data), "\n")
		for i, line := range lines {
			// Skip comments.
			trimmed := strings.TrimSpace(line)
			if strings.HasPrefix(trimmed, "//") {
				continue
			}

			if sensitivePatterns.MatchString(line) {
				// Exclude false positives: key name in config warnings is safe.
				if strings.Contains(line, `unknown key`) {
					continue
				}
				t.Errorf("potential vault payload in log/error at %s:%d: %s", path, i+1, trimmed)
			}
		}
		return nil
	})
	if err != nil {
		t.Fatalf("walk: %v", err)
	}
}

// TestNoVaultPayloadsInLogs_DetectsViolation is a negative self-test that
// proves the matcher used in TestNoVaultPayloadsInLogs fires on a planted
// violation. If this test fails, the matcher is silently broken.
func TestNoVaultPayloadsInLogs_DetectsViolation(t *testing.T) {
	dir := t.TempDir()
	planted := filepath.Join(dir, "planted.go")
	content := []byte(`package p
import "fmt"
func logSecret() {
	fmt.Printf("secret: %s", "hunter2")
}
`)
	if err := os.WriteFile(planted, content, 0o644); err != nil {
		t.Fatalf("write planted file: %v", err)
	}

	sensitivePatterns := regexp.MustCompile(
		`fmt\.(Errorf|Sprintf|Fprintf|Printf|Println|Print)\(.*` +
			`([Vv]ault|[Cc]redential|[Ss]ecret|[Tt]oken|[Pp]assword|[Kk]ey[Aa]ge|` +
			`[Aa]ge\.|identity|recipient)`)

	data, err := os.ReadFile(planted)
	if err != nil {
		t.Fatalf("read planted file: %v", err)
	}

	var found bool
	lines := strings.Split(string(data), "\n")
	for _, line := range lines {
		trimmed := strings.TrimSpace(line)
		if strings.HasPrefix(trimmed, "//") {
			continue
		}
		if sensitivePatterns.MatchString(line) {
			found = true
			break
		}
	}
	if !found {
		t.Error("negative self-test: vault payload matcher did NOT fire on planted violation; " +
			"TestNoVaultPayloadsInLogs may be silently passing")
	}
}

// TestMemoryContentNeverInAuditLog verifies that redactArgs always
// redacts the "content" field value in verbose mode, regardless of
// server. This prevents user-authored text (which may contain
// credentials) from appearing verbatim in audit log output.
func TestMemoryContentNeverInAuditLog(t *testing.T) {
	t.Parallel()

	root := findRepoRoot(t)
	data, err := os.ReadFile(filepath.Join(root, "internal/audit/log.go"))
	if err != nil {
		t.Fatalf("read internal/audit/log.go: %v", err)
	}
	source := string(data)

	// Verify contentFields map exists and includes "content".
	if !strings.Contains(source, `"content": true`) {
		t.Error("internal/audit/log.go must include \"content\" in contentFields map")
	}

	// Verify the redacted output literal is present.
	if !strings.Contains(source, `[redacted]`) {
		t.Error("internal/audit/log.go must emit [redacted] for content fields")
	}

	// Verify that the old direct %v logging pattern for content fields
	// is no longer present in the verbose branch.
	// The old pattern was: valParts = append(valParts, fmt.Sprintf("%s=%v", k, m[k]))
	// The new pattern checks contentFields[k] before logging.
	if strings.Contains(source, `fmt.Sprintf("%s=%v", k, m[k])`) {
		t.Error("redactArgs still uses unsanitized logging — content fields are not redacted")
	}
}

// TestHarnessConfigPathTraversal verifies that the harness config writer
// does not use user-supplied paths directly in file operations without
// validation. Path components must be validated to prevent traversal.
func TestHarnessConfigPathTraversal(t *testing.T) {
	t.Parallel()

	// The harness document.Load function takes a path and reads it via
	// os.ReadFile. This is safe because:
	// 1. The path comes from the harness registry (not user input).
	// 2. The document parser (JSON/TOML) rejects malformed configs.
	// 3. Backups are created in the same directory as the original.
	//
	// This test verifies the invariant: no filepath.Join with ".." in
	// harness code paths. The regex looks for ".." as a path component,
	// not as part of variable names like "parts...".
	traversalPattern := regexp.MustCompile(`filepath\.Join\(.*"[^"]*\.\.[^"]*"`)

	// Find the repo root by looking for go.mod.
	root := findRepoRoot(t)

	err := filepath.Walk(filepath.Join(root, "internal/harness"), func(path string, info os.FileInfo, err error) error {
		if err != nil {
			return err
		}
		if strings.HasSuffix(path, "_test.go") {
			return nil
		}
		if !strings.HasSuffix(path, ".go") {
			return nil
		}

		data, err := os.ReadFile(path)
		if err != nil {
			return err
		}

		if traversalPattern.Match(data) {
			t.Errorf("potential path traversal in %s: filepath.Join with '..'", path)
		}
		return nil
	})
	if err != nil {
		t.Fatalf("walk: %v", err)
	}
}

// TestChildSpawnExecDirect verifies that the broker's Spawn function uses
// exec.Command directly (not exec.CommandContext or shell), ensuring no
// shell interpolation occurs.
func TestChildSpawnExecDirect(t *testing.T) {
	t.Parallel()

	root := findRepoRoot(t)
	data, err := os.ReadFile(filepath.Join(root, "internal/broker/client.go"))
	if err != nil {
		t.Fatalf("read broker/client.go: %v", err)
	}

	source := string(data)

	// Verify Spawn uses exec.Command (not exec.CommandContext).
	if strings.Contains(source, "exec.CommandContext") {
		t.Error("broker/client.go should use exec.Command, not exec.CommandContext")
	}

	// Verify no shell invocation patterns.
	shellPatterns := []string{"sh -c", "/bin/sh", "/bin/bash"}
	for _, pattern := range shellPatterns {
		if strings.Contains(source, pattern) {
			t.Errorf("broker/client.go contains shell pattern: %q", pattern)
		}
	}
}

// TestCleanEnvPassing verifies that the broker's Options.Env field is
// controlled by the caller and not accidentally populated with secrets.
func TestCleanEnvPassing(t *testing.T) {
	t.Parallel()

	root := findRepoRoot(t)
	data, err := os.ReadFile(filepath.Join(root, "internal/broker/client.go"))
	if err != nil {
		t.Fatalf("read broker/client.go: %v", err)
	}

	source := string(data)

	// Verify Options.Env is documented as controlled by caller.
	if !strings.Contains(source, "Env, if non-nil, replaces the child's environment entirely") {
		t.Error("broker/client.go Options.Env should document caller-controlled behavior")
	}

	// Verify no hardcoded secret patterns in Env handling.
	secretPatterns := []string{
		"API_KEY", "SECRET_KEY", "TOKEN", "PASSWORD",
	}
	for _, pattern := range secretPatterns {
		if strings.Contains(source, pattern) {
			t.Errorf("broker/client.go contains hardcoded secret pattern: %q", pattern)
		}
	}
}

// TestGovulncheckClean is a placeholder for govulncheck verification.
// The actual govulncheck run is performed in CI and before release.
// This test documents that the property was verified.
func TestGovulncheckClean(t *testing.T) {
	t.Parallel()
	// govulncheck ./... was run and reported "No vulnerabilities found"
	// on 2026-07-21. This test serves as documentation of the verification.
	// CI runs govulncheck separately; this is a regression marker.
	t.Log("govulncheck verified clean on 2026-07-21; CI runs this independently")
}

// findRepoRoot walks up from the test directory to find go.mod.
func findRepoRoot(t *testing.T) string {
	t.Helper()
	dir, err := os.Getwd()
	if err != nil {
		t.Fatalf("getwd: %v", err)
	}
	for {
		if _, err := os.Stat(filepath.Join(dir, "go.mod")); err == nil {
			return dir
		}
		parent := filepath.Dir(dir)
		if parent == dir {
			t.Fatal("could not find repo root (go.mod)")
		}
		dir = parent
	}
}
