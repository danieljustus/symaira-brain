package usage

import (
	"errors"
	"testing"
)

// fakeVault is a vaultResolver stub that answers only the given paths.
func fakeVault(responses map[string]string) func(string, string) (string, error) {
	return func(value, fallback string) (string, error) {
		if secret, ok := responses[value]; ok {
			return secret, nil
		}
		return "", errors.New("symvault entry not found")
	}
}

func TestResolveEnvPlain(t *testing.T) {
	t.Setenv("SYMBRAIN_TEST_PLAIN", "sk-abc")
	v, src, err := resolveEnv("SYMBRAIN_TEST_PLAIN")
	if err != nil {
		t.Fatalf("resolveEnv: %v", err)
	}
	if v != "sk-abc" || src != "env" {
		t.Fatalf("resolveEnv = (%q, %q), want (\"sk-abc\", \"env\")", v, src)
	}
}

func TestResolveEnvUnset(t *testing.T) {
	t.Setenv("SYMBRAIN_TEST_UNSET", "")
	v, src, err := resolveEnv("SYMBRAIN_TEST_UNSET")
	if err != nil || v != "" || src != "" {
		t.Fatalf("resolveEnv(unset) = (%q, %q, %v), want (\"\", \"\", nil)", v, src, err)
	}
}

func TestResolveEnvVaultURI(t *testing.T) {
	old := vaultResolver
	vaultResolver = fakeVault(map[string]string{"symvault://ai/openrouter/api-key": "top-secret"})
	defer func() { vaultResolver = old }()

	t.Setenv("SYMBRAIN_TEST_VAULT", "symvault://ai/openrouter/api-key")
	v, src, err := resolveEnv("SYMBRAIN_TEST_VAULT")
	if err != nil {
		t.Fatalf("resolveEnv: %v", err)
	}
	if v != "top-secret" || src != "vault" {
		t.Fatalf("resolveEnv = (%q, %q), want (\"top-secret\", \"vault\")", v, src)
	}
}

func TestResolveEnvVaultURIFailure(t *testing.T) {
	old := vaultResolver
	vaultResolver = fakeVault(nil)
	defer func() { vaultResolver = old }()

	t.Setenv("SYMBRAIN_TEST_VAULT_FAIL", "symvault://ai/missing/cred")
	v, src, err := resolveEnv("SYMBRAIN_TEST_VAULT_FAIL")
	if err == nil {
		t.Fatal("resolveEnv: expected error for unknown symvault path")
	}
	if v != "" || src != "vault" {
		t.Fatalf("resolveEnv = (%q, %q), want (\"\", \"vault\")", v, src)
	}
	if got := authErrStatus(err); got.Status != "missing" || got.Source != "vault" {
		t.Fatalf("authErrStatus = %+v, want missing/vault", got)
	}
}

func TestResolveFileCredentialPrecedence(t *testing.T) {
	old := vaultResolver
	vaultResolver = fakeVault(map[string]string{"symvault://ai/codex/token": "from-vault"})
	defer func() { vaultResolver = old }()

	readFile := func() string { return "from-file" }

	t.Run("env wins", func(t *testing.T) {
		t.Setenv("SYMBRAIN_TEST_FC", "from-env")
		v, src, err := resolveFileCredential("SYMBRAIN_TEST_FC", readFile)
		if err != nil || v != "from-env" || src != "env" {
			t.Fatalf("resolveFileCredential = (%q, %q, %v)", v, src, err)
		}
	})

	t.Run("file fallback", func(t *testing.T) {
		t.Setenv("SYMBRAIN_TEST_FC", "")
		v, src, err := resolveFileCredential("SYMBRAIN_TEST_FC", readFile)
		if err != nil || v != "from-file" || src != "file" {
			t.Fatalf("resolveFileCredential = (%q, %q, %v)", v, src, err)
		}
	})

	t.Run("vault URI wins over file", func(t *testing.T) {
		t.Setenv("SYMBRAIN_TEST_FC", "symvault://ai/codex/token")
		v, src, err := resolveFileCredential("SYMBRAIN_TEST_FC", readFile)
		if err != nil || v != "from-vault" || src != "vault" {
			t.Fatalf("resolveFileCredential = (%q, %q, %v)", v, src, err)
		}
	})

	t.Run("vault failure skips file", func(t *testing.T) {
		t.Setenv("SYMBRAIN_TEST_FC", "symvault://ai/unknown/x")
		v, src, err := resolveFileCredential("SYMBRAIN_TEST_FC", readFile)
		if err == nil || v != "" || src != "vault" {
			t.Fatalf("resolveFileCredential = (%q, %q, %v), want error and no file fallback", v, src, err)
		}
	})
}
