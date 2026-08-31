package usage

import (
	"fmt"
	"os"
	"strings"

	"github.com/danieljustus/symaira-brain/internal/memory/secrets"
)

// vaultResolver resolves shared secret references. It is a package-level
// variable so tests can substitute a fake and never spawn a real subprocess;
// production uses the compatibility adapter in internal/memory/secrets.
var vaultResolver = secrets.Resolve

// resolveEnv reads a provider credential from the environment:
//
//   - unset → ("", "", nil): the caller falls back to its file-based source.
//   - plain value → returned unchanged, source "env".
//   - symvault://<path> or vault://<path> → looked up in the secret store,
//     source "vault".
//   - env://NAME or keychain://service/account → resolved by the shared
//     resolver, with source "env" or "keychain".
//
// The secret value never appears in returned errors.
func resolveEnv(envName string) (value, source string, err error) {
	raw := os.Getenv(envName)
	if raw == "" {
		return "", "", nil
	}
	if secrets.IsSecretReference(raw) {
		secret, err := vaultResolver(raw, "")
		if err != nil {
			return "", secretSource(raw), fmt.Errorf("resolve %s: %w", envName, err)
		}
		return secret, secretSource(raw), nil
	}
	return raw, "env", nil
}

func secretSource(reference string) string {
	switch {
	case strings.HasPrefix(reference, "env://"):
		return "env"
	case strings.HasPrefix(reference, "keychain://"):
		return "keychain"
	default:
		return "vault"
	}
}

// resolveFileCredential resolves a credential with a file-based fallback:
// the env var (symvault-capable) wins; otherwise readFile is tried. The
// winner's source tag ("env"|"vault"|"file") is returned so AuthStatus can
// say where the credential came from.
func resolveFileCredential(envName string, readFile func() string) (value, source string, err error) {
	value, source, err = resolveEnv(envName)
	if err != nil || value != "" {
		return value, source, err
	}
	if value = readFile(); value != "" {
		source = "file"
	}
	return value, source, nil
}

// authErrStatus is the AuthStatus reported by a provider whose credential
// resolution failed (e.g. a symvault:// lookup). The error text never
// contains the secret itself — only the resolution failure reason.
func authErrStatus(err error) AuthStatus {
	return AuthStatus{Status: "missing", Detail: "credential resolution failed: " + err.Error(), Source: "vault"}
}
