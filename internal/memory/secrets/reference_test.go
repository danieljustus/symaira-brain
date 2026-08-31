package secrets

import (
	"os"
	"runtime"
	"strings"
	"testing"
)

func TestIsSecretReference(t *testing.T) {
	tests := []struct {
		name  string
		value string
		want  bool
	}{
		{name: "symvault", value: "symvault://service/path", want: true},
		{name: "deprecated vault", value: "vault://service/path", want: true},
		{name: "environment", value: "env://API_TOKEN", want: true},
		{name: "keychain", value: "keychain://service/account", want: true},
		{name: "plain value", value: "literal-secret", want: false},
		{name: "near miss", value: "env:API_TOKEN", want: false},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := IsSecretReference(tt.value); got != tt.want {
				t.Errorf("IsSecretReference(%q) = %v, want %v", tt.value, got, tt.want)
			}
		})
	}
}

func TestResolveEnvReference(t *testing.T) {
	t.Setenv("SYMBRAIN_SECRETREF_TEST", "env-test-value")

	got, err := Resolve("env://SYMBRAIN_SECRETREF_TEST", "")
	if err != nil {
		t.Fatalf("Resolve env reference: %v", err)
	}
	if got != "env-test-value" {
		t.Fatalf("Resolve env reference = %q, want %q", got, "env-test-value")
	}
}

func TestResolveKeychainReference(t *testing.T) {
	if runtime.GOOS != "darwin" {
		t.Skip("keychain references use the macOS security CLI")
	}

	dir := t.TempDir()
	securityPath := dir + "/security"
	if err := os.WriteFile(securityPath, []byte("#!/bin/sh\nif [ \"$1\" != \"find-generic-password\" ]; then exit 1; fi\necho keychain-test-value\n"), 0o755); err != nil {
		t.Fatalf("write fake security CLI: %v", err)
	}
	t.Setenv("PATH", dir+":"+os.Getenv("PATH"))

	got, err := Resolve("keychain://service/account", "")
	if err != nil {
		t.Fatalf("Resolve keychain reference: %v", err)
	}
	if got != "keychain-test-value" {
		t.Fatalf("Resolve keychain reference = %q, want %q", got, "keychain-test-value")
	}
}

func TestResolveReferenceErrorDoesNotLeakValue(t *testing.T) {
	t.Setenv("SYMBRAIN_SECRETREF_MISSING", "")

	_, err := Resolve("env://SYMBRAIN_SECRETREF_MISSING", "")
	if err == nil {
		t.Fatal("Resolve missing env reference succeeded")
	}
	if strings.Contains(err.Error(), "env-test-value") {
		t.Fatalf("Resolve error leaked a resolved value: %v", err)
	}
}
