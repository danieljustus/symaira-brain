package secrets

import (
	"context"
	"fmt"
	"os"
	"os/exec"
	"strings"
	"time"
)

const (
	// vaultPrefix is the URI scheme for vault:// secret references.
	vaultPrefix = "vault://"
	// vaultTimeout is the maximum time to wait for symvault subprocess.
	vaultTimeout = 5 * time.Second
)

// IsVaultURI returns true if the value starts with vault://.
func IsVaultURI(value string) bool {
	return strings.HasPrefix(value, vaultPrefix)
}

// Resolve attempts to resolve a secret value that may be a vault:// URI.
//
// Resolution order:
//  1. Plain value (no vault:// prefix) → returned as-is
//  2. vault://<path> → subprocess "symvault get <path> --print" with 5s timeout
//  3. Fallback to env var named by envFallback (e.g. "JWT_SECRET_KEY")
//
// On success, the resolved plaintext is returned. On failure, a descriptive
// error is returned — the secret value is never included in error messages.
func Resolve(value, envFallback string) (string, error) {
	if !IsVaultURI(value) {
		return value, nil
	}

	path := strings.TrimPrefix(value, vaultPrefix)

	secret, err := execVaultGet(path)
	if err == nil {
		return secret, nil
	}

	// Fallback: check environment variable
	if envFallback != "" {
		if env := os.Getenv(envFallback); env != "" {
			return env, nil
		}
	}

	return "", fmt.Errorf(
		"secret resolution failed for vault://%s: %w; "+
			"set env var %s as fallback or install symvault",
		path, err, envFallback,
	)
}

// execVaultGet runs "symvault get <path> --print" as a subprocess with a timeout.
// Only the trimmed stdout is returned; stderr and exit codes are wrapped
// into the error. The secret value is never logged.
func execVaultGet(path string) (string, error) {
	if _, err := exec.LookPath("symvault"); err != nil {
		return "", fmt.Errorf("symvault not found in PATH: %w", err)
	}

	ctx, cancel := context.WithTimeout(context.Background(), vaultTimeout)
	defer cancel()

	cmd := exec.CommandContext(ctx, "symvault", "get", path, "--print")
	out, err := cmd.Output()
	if err != nil {
		if ctx.Err() == context.DeadlineExceeded {
			return "", fmt.Errorf("symvault get timed out after %s", vaultTimeout)
		}
		return "", fmt.Errorf("symvault get failed: %w", err)
	}

	secret := strings.TrimSpace(string(out))
	if secret == "" {
		return "", fmt.Errorf("symvault get returned empty secret for path %q", path)
	}

	return secret, nil
}

// ResolveOrEnv is a convenience wrapper that resolves a vault:// value
// and falls back to an environment variable. If the value is neither
// a vault:// URI nor non-empty, the env var is returned directly.
func ResolveOrEnv(value, envName string) (string, error) {
	if value != "" {
		return Resolve(value, envName)
	}
	if env := os.Getenv(envName); env != "" {
		return env, nil
	}
	return "", nil
}
