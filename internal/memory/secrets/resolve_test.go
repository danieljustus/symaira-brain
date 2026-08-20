package secrets

import (
	"os"
	"path/filepath"
	"testing"
)

func TestIsVaultURI(t *testing.T) {
	tests := []struct {
		value string
		want  bool
	}{
		{"vault://symaira/memory/jwt", true},
		{"vault://my-secret", true},
		{"vault://a", true},
		{"plain-value", false},
		{"", false},
		{"VaulT://case-sensitive", false},
		{"/path/to/file", false},
		{"env:JWT_SECRET", false},
	}

	for _, tt := range tests {
		if got := IsVaultURI(tt.value); got != tt.want {
			t.Errorf("IsVaultURI(%q) = %v, want %v", tt.value, got, tt.want)
		}
	}
}

func TestResolvePlainValuePassthrough(t *testing.T) {
	plain := "my-plain-secret-value"
	got, err := Resolve(plain, "")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if got != plain {
		t.Errorf("expected %q, got %q", plain, got)
	}
}

func TestResolveVaultWithEnvFallback(t *testing.T) {
	// When symvault is not available, vault:// falls back to env var
	envName := "TEST_SYMMEMORY_VAULT_FALLBACK"
	os.Setenv(envName, "fallback-secret-from-env")
	defer os.Unsetenv(envName)

	got, err := Resolve("vault://nonexistent/path", envName)
	if err != nil {
		t.Fatalf("expected env fallback to succeed, got error: %v", err)
	}
	if got != "fallback-secret-from-env" {
		t.Errorf("expected fallback value, got %q", got)
	}
}

func TestResolveVaultNoFallbackEnvEmpty(t *testing.T) {
	// vault:// with no symvault and no env var should fail
	envName := "TEST_SYMMEMORY_VAULT_EMPTY"
	os.Unsetenv(envName)

	_, err := Resolve("vault://secret/path", envName)
	if err == nil {
		t.Fatal("expected error when symvault missing and env empty")
	}
}

func TestResolveVaultNoFallbackEnvMissing(t *testing.T) {
	// vault:// with no symvault and non-existent env var should fail
	_, err := Resolve("vault://secret/path", "NONEXISTENT_ENV_VAR_12345")
	if err == nil {
		t.Fatal("expected error when symvault missing and env var unset")
	}
}

func TestResolveEmptyEnvFallbackName(t *testing.T) {
	// vault:// with empty env fallback name — should fail since symvault unavailable
	_, err := Resolve("vault://secret/path", "")
	if err == nil {
		t.Fatal("expected error when symvault missing and no env fallback")
	}
}

func TestResolveOrEnvWithValue(t *testing.T) {
	got, err := ResolveOrEnv("plain-secret", "UNUSED_ENV")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if got != "plain-secret" {
		t.Errorf("expected plain value, got %q", got)
	}
}

func TestResolveOrEnvWithEnv(t *testing.T) {
	envName := "TEST_SYMMEMORY_RESOLVE_OR_ENV"
	os.Setenv(envName, "env-secret-value")
	defer os.Unsetenv(envName)

	got, err := ResolveOrEnv("", envName)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if got != "env-secret-value" {
		t.Errorf("expected env value, got %q", got)
	}
}

func TestResolveOrEnvEmpty(t *testing.T) {
	got, err := ResolveOrEnv("", "NONEXISTENT_ENV_VAR_12345")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if got != "" {
		t.Errorf("expected empty string, got %q", got)
	}
}

func TestResolveVaultWithFakeSymvault(t *testing.T) {
	// Create a fake symvault script in a temp directory
	dir := t.TempDir()
	script := filepath.Join(dir, "symvault")
	content := `#!/bin/sh
case "$1" in
  get)
    case "$2" in
      symaira/memory/jwt)
        echo "resolved-jwt-secret-from-vault"
        ;;
      *)
        echo "unknown path: $2" >&2
        exit 1
        ;;
    esac
    ;;
  *)
    echo "usage: symvault get <path>" >&2
    exit 1
    ;;
esac
`
	if err := os.WriteFile(script, []byte(content), 0755); err != nil {
		t.Fatalf("failed to write fake symvault: %v", err)
	}

	// Prepend temp dir to PATH
	oldPath := os.Getenv("PATH")
	os.Setenv("PATH", dir+":"+oldPath)
	defer os.Setenv("PATH", oldPath)

	got, err := Resolve("vault://symaira/memory/jwt", "UNUSED")
	if err != nil {
		t.Fatalf("expected resolve to succeed, got error: %v", err)
	}
	if got != "resolved-jwt-secret-from-vault" {
		t.Errorf("expected resolved secret, got %q", got)
	}
}

func TestResolveVaultCallsSymvaultWithPrintFlag(t *testing.T) {
	dir := t.TempDir()
	argsPath := filepath.Join(dir, "args.txt")
	script := filepath.Join(dir, "symvault")
	content := `#!/bin/sh
printf '%s\n' "$@" > "` + argsPath + `"
echo "resolved-with-print"
`
	if err := os.WriteFile(script, []byte(content), 0755); err != nil {
		t.Fatalf("failed to write fake symvault: %v", err)
	}

	oldPath := os.Getenv("PATH")
	os.Setenv("PATH", dir)
	defer os.Setenv("PATH", oldPath)

	got, err := Resolve("vault://symaira/memory/jwt", "UNUSED")
	if err != nil {
		t.Fatalf("expected resolve to succeed, got error: %v", err)
	}
	if got != "resolved-with-print" {
		t.Errorf("expected resolved secret, got %q", got)
	}

	rawArgs, err := os.ReadFile(argsPath)
	if err != nil {
		t.Fatalf("failed to read fake symvault args: %v", err)
	}
	want := "get\nsymaira/memory/jwt\n--print\n"
	if string(rawArgs) != want {
		t.Fatalf("symvault args = %q, want %q", string(rawArgs), want)
	}
}

func TestResolveVaultFakeSymvaultUnknownPath(t *testing.T) {
	// Fake symvault that rejects unknown paths
	dir := t.TempDir()
	script := filepath.Join(dir, "symvault")
	content := `#!/bin/sh
echo "unknown path" >&2
exit 1
`
	if err := os.WriteFile(script, []byte(content), 0755); err != nil {
		t.Fatalf("failed to write fake symvault: %v", err)
	}

	oldPath := os.Getenv("PATH")
	os.Setenv("PATH", dir+":"+oldPath)
	defer os.Setenv("PATH", oldPath)

	// Should fail from symvault, then fall back to env
	envName := "TEST_SYMMEMORY_VAULT_UNKNOWN_PATH"
	os.Setenv(envName, "fallback-for-unknown")
	defer os.Unsetenv(envName)

	got, err := Resolve("vault://unknown/path", envName)
	if err != nil {
		t.Fatalf("expected env fallback to succeed, got error: %v", err)
	}
	if got != "fallback-for-unknown" {
		t.Errorf("expected env fallback value, got %q", got)
	}
}

func TestResolveVaultFakeSymvaultEmptyOutput(t *testing.T) {
	// Fake symvault that returns empty output
	dir := t.TempDir()
	script := filepath.Join(dir, "symvault")
	content := `#!/bin/sh
# Returns nothing
`
	if err := os.WriteFile(script, []byte(content), 0755); err != nil {
		t.Fatalf("failed to write fake symvault: %v", err)
	}

	oldPath := os.Getenv("PATH")
	os.Setenv("PATH", dir+":"+oldPath)
	defer os.Setenv("PATH", oldPath)

	envName := "TEST_SYMMEMORY_VAULT_EMPTY_OUTPUT"
	os.Setenv(envName, "fallback-for-empty")
	defer os.Unsetenv(envName)

	got, err := Resolve("vault://empty/path", envName)
	if err != nil {
		t.Fatalf("expected env fallback to succeed, got error: %v", err)
	}
	if got != "fallback-for-empty" {
		t.Errorf("expected env fallback value, got %q", got)
	}
}
