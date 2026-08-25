package usage

import (
	"fmt"
	"os"

	"github.com/danieljustus/symaira-brain/internal/memory/secrets"
)

// vaultResolver resolves symvault:// (and the deprecated vault:// alias)
// secret URIs. It is a package-level variable so tests can substitute a
// fake and never spawn a real symvault subprocess; production always uses
// secrets.Resolve (internal/memory/secrets, issue #287).
var vaultResolver = secrets.Resolve

// resolveEnv reads a provider credential from the environment:
//
//   - unset → ("", "", nil): the caller falls back to its file-based source.
//   - plain value → returned unchanged, source "env".
//   - symvault://<path> (or deprecated vault://<path>) → looked up in the
//     secret store, source "vault"; resolution failures return the error so
//     the provider can surface an exact AuthStatus instead of silently
//     reporting "not configured".
//
// The secret value never appears in returned errors.
func resolveEnv(envName string) (value, source string, err error) {
	raw := os.Getenv(envName)
	if raw == "" {
		return "", "", nil
	}
	if secrets.IsVaultURI(raw) {
		secret, err := vaultResolver(raw, "")
		if err != nil {
			return "", "vault", fmt.Errorf("resolve %s: %w", envName, err)
		}
		return secret, "vault", nil
	}
	return raw, "env", nil
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
