package security

import (
	"os"
	"path/filepath"
	"runtime"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
)

func TestNewJWTProviderResolvesEnvReference(t *testing.T) {
	t.Setenv("SYMBRAIN_JWT_SECRET_REFERENCE", "jwt-env-reference-secret")
	cfg := &config.Config{
		JWT: config.JWTConfig{
			Secret:     "env://SYMBRAIN_JWT_SECRET_REFERENCE",
			SecretPath: filepath.Join(t.TempDir(), "jwt.secret"),
		},
	}

	provider, err := NewJWTProvider(cfg, nil)
	if err != nil {
		t.Fatalf("NewJWTProvider: %v", err)
	}
	if got := string(provider.secret); got != "jwt-env-reference-secret" {
		t.Fatalf("resolved JWT secret = %q, want env reference value", got)
	}
}

func TestNewJWTProviderResolvesKeychainReference(t *testing.T) {
	if runtime.GOOS != "darwin" {
		t.Skip("keychain references use the macOS security CLI")
	}

	dir := t.TempDir()
	securityPath := filepath.Join(dir, "security")
	if err := os.WriteFile(securityPath, []byte("#!/bin/sh\nif [ \"$1\" != \"find-generic-password\" ]; then exit 1; fi\necho jwt-keychain-reference-secret\n"), 0o755); err != nil {
		t.Fatalf("write fake security CLI: %v", err)
	}
	t.Setenv("PATH", dir+":"+os.Getenv("PATH"))

	cfg := &config.Config{
		JWT: config.JWTConfig{
			Secret:     "keychain://service/account",
			SecretPath: filepath.Join(t.TempDir(), "jwt.secret"),
		},
	}
	provider, err := NewJWTProvider(cfg, nil)
	if err != nil {
		t.Fatalf("NewJWTProvider: %v", err)
	}
	if got := string(provider.secret); got != "jwt-keychain-reference-secret" {
		t.Fatalf("resolved JWT secret = %q, want keychain reference value", got)
	}
}
