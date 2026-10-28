//go:build unix

package usage

import (
	"context"
	"fmt"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestOllamaFailClosedAndRedacted(t *testing.T) {
	home := t.TempDir()
	t.Setenv("HOME", home)
	requests := 0
	provider := NewOllamaProvider(&http.Client{Transport: ollamaRoundTrip(func(req *http.Request) (*http.Response, error) {
		requests++
		return nil, fmt.Errorf("transport echoed %s", req.Header.Get("Authorization"))
	})})
	if _, err := provider.Fetch(context.Background()); err == nil || requests != 0 {
		t.Fatal("missing key did not fail closed")
	}
	if err := os.Mkdir(filepath.Join(home, ".ollama"), 0700); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(home, ".ollama/id_ed25519")
	if err := os.WriteFile(path, []byte("not a key"), 0600); err != nil {
		t.Fatal(err)
	}
	if _, err := provider.Fetch(context.Background()); err == nil || requests != 0 {
		t.Fatal("malformed key did not fail closed")
	}
	_, data := ollamaFixture(t)
	if err := os.WriteFile(path, data, 0600); err != nil {
		t.Fatal(err)
	}
	if _, err := provider.Fetch(context.Background()); err == nil || strings.Contains(err.Error(), "transport echoed") || requests != 1 {
		t.Fatalf("unsafe transport error: %v", err)
	}
}
