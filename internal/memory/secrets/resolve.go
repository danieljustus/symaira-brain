package secrets

import (
	"context"
	"fmt"
	"os"
	"strings"

	"github.com/danieljustus/symaira-corekit/secretref"
)

const (
	// symvaultPrefix is the canonical URI scheme for secret references.
	symvaultPrefix = "symvault://"
	// vaultPrefix is a deprecated alias for symvaultPrefix, kept for
	// backward compatibility with existing configs.
	vaultPrefix    = "vault://"
	envPrefix      = "env://"
	keychainPrefix = "keychain://"
)

// IsVaultURI returns true if the value starts with symvault:// (canonical)
// or vault:// (deprecated alias).
func IsVaultURI(value string) bool {
	return strings.HasPrefix(value, symvaultPrefix) || strings.HasPrefix(value, vaultPrefix)
}

// IsSecretReference reports whether value uses one of the shared secret
// reference schemes. Plain values remain literal for compatibility.
func IsSecretReference(value string) bool {
	return IsVaultURI(value) || strings.HasPrefix(value, envPrefix) || strings.HasPrefix(value, keychainPrefix)
}

// Resolve resolves a shared secret reference while preserving Brain's
// historical treatment of non-reference values as literal plaintext.
//
// Resolution order:
//  1. Plain value (no supported URI prefix) → returned as-is.
//  2. symvault://<path> (or deprecated vault://<path>) → shared resolver.
//  3. env://NAME or keychain://service/account → shared resolver.
//  4. symvault failures fall back to envFallback for compatibility.
//
// On success, the resolved plaintext is returned. On failure, a descriptive
// error is returned — the secret value is never included in error messages.
func Resolve(value, envFallback string) (string, error) {
	if value == "" || !IsSecretReference(value) {
		return value, nil
	}

	reference := value
	if strings.HasPrefix(value, vaultPrefix) {
		reference = symvaultPrefix + strings.TrimPrefix(value, vaultPrefix)
	}

	secret, err := secretref.Resolve(context.Background(), reference, "")
	if err == nil && secret != "" {
		return secret, nil
	}
	if err == nil {
		err = fmt.Errorf("shared resolver returned empty secret")
	}

	// Keep the legacy fallback for vault references when the subprocess is
	// unavailable or returns an error.
	if IsVaultURI(value) && envFallback != "" {
		if fallback := os.Getenv(envFallback); fallback != "" {
			return fallback, nil
		}
	}

	display := reference
	if strings.HasPrefix(reference, symvaultPrefix) {
		display = symvaultPrefix + strings.TrimPrefix(reference, symvaultPrefix)
	}
	return "", fmt.Errorf("secret resolution failed for %s: %w; set env var %s as fallback or install symvault", display, err, envFallback)
}

// ResolveOrEnv is a convenience wrapper that resolves a shared reference
// and falls back to an environment variable. If the value is neither
// a reference nor non-empty, the env var is returned directly.
func ResolveOrEnv(value, envName string) (string, error) {
	if value != "" {
		return Resolve(value, envName)
	}
	if env := os.Getenv(envName); env != "" {
		return env, nil
	}
	return "", nil
}
