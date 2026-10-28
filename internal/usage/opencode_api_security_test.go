package usage

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestOpenCodeAPIReferencesDoNotDowngrade(t *testing.T) {
	t.Setenv("OPENCODE_COOKIE", "session=unrelated-account")
	t.Setenv("OPENCODE_GO_API_KEY", "symvault://test/opencode/key")
	withVault(t, map[string]string{"symvault://test/opencode/key": "synthetic-key", "env://OPENCODE_TEST_KEY": "synthetic-env-key"})
	p := newCurrentOpenCodeProvider(nil)
	if p.apiKey != "synthetic-key" || p.AuthStatus().Source != "vault" || p.cookieHeader != "" {
		t.Fatal("reference route")
	}
	t.Setenv("OPENCODE_GO_API_KEY", "env://OPENCODE_TEST_KEY")
	t.Setenv("OPENCODE_TEST_KEY", "synthetic-env-key")
	if newCurrentOpenCodeProvider(nil).apiKey != "synthetic-env-key" {
		t.Fatal("env reference")
	}
	t.Setenv("OPENCODE_GO_API_KEY", "symvault://test/missing")
	p = newCurrentOpenCodeProvider(nil)
	if p.IsConfigured() || len(p.Strategies()) != 0 {
		t.Fatal("failed reference downgraded")
	}
	client, _ := fakeErrorClient(errors.New("synthetic-key transport leak"))
	_, err := (&openCodeAPIStrategy{key: "synthetic-key", client: client}).Fetch(context.Background())
	if err == nil || strings.Contains(err.Error(), "synthetic-key") {
		t.Fatal("transport error leak")
	}
}
func TestOpenCodeAPIUnsafeAuthJSON(t *testing.T) {
	for _, body := range []string{"null", "{", `{"opencode-go":null}`, `{"opencode-go":{"key":true}}`, strings.Repeat("x", (64<<10)+1)} {
		dir := t.TempDir()
		if err := os.WriteFile(filepath.Join(dir, "auth.json"), []byte(body), 0600); err != nil {
			t.Fatal(err)
		}
		if _, err := openCodeAuthJSONKey(dir); err == nil {
			t.Fatal("unsafe auth accepted")
		}
	}
	dir := t.TempDir()
	if err := os.Mkdir(filepath.Join(dir, "auth.json"), 0700); err != nil {
		t.Fatal(err)
	}
	if _, err := openCodeAuthJSONKey(dir); err == nil {
		t.Fatal("nonregular accepted")
	}
}
