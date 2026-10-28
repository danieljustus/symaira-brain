//go:build unix

package usage

import (
	"context"
	"crypto/ed25519"
	"encoding/base64"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"golang.org/x/sys/unix"
)

func TestOllamaKeyBoundaryAndOfflineHTTP(t *testing.T) {
	key, data := ollamaFixture(t)
	home := t.TempDir()
	t.Setenv("HOME", home)
	dir := filepath.Join(home, ".ollama")
	if err := os.Mkdir(dir, 0700); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(dir, "id_ed25519")
	if err := os.WriteFile(path, data, 0600); err != nil {
		t.Fatal(err)
	}
	requests := 0
	client := &http.Client{Transport: ollamaRoundTrip(func(req *http.Request) (*http.Response, error) {
		requests++
		parts := strings.Split(req.Header.Get("Authorization"), ":")
		if len(parts) != 2 {
			t.Fatal("auth format")
		}
		sig, err := base64.StdEncoding.DecodeString(parts[1])
		if err != nil {
			t.Fatal(err)
		}
		if !ed25519.Verify(key.Public().(ed25519.PublicKey), []byte(req.Method+","+req.URL.RequestURI()), sig) {
			t.Fatal("wire signature")
		}
		if req.URL.Host != "ollama.com" || req.URL.Path != "/api/balance" || req.URL.Query().Get("ts") == "" {
			t.Fatal("origin or URI")
		}
		return ollamaResponse(200, `{"included":{"session":{"remaining_percent":75}}}`), nil
	})}
	provider := NewOllamaProvider(client)
	report := BuildReport(context.Background(), []Provider{provider})
	if len(report.Providers) != 1 || report.Providers[0].Snapshot == nil || requests != 1 {
		t.Fatalf("report %+v", report)
	}
	for _, mode := range []os.FileMode{0644, 0620, os.ModeSetuid | 0700} {
		if err := os.Chmod(path, mode); err != nil {
			t.Fatal(err)
		}
		if _, err := readOllamaKeyFile(path); err == nil {
			t.Fatalf("unsafe mode %o accepted", mode)
		}
	}
	if err := os.Chmod(path, 0600); err != nil {
		t.Fatal(err)
	}
	for _, kind := range []string{"symlink", "hardlink", "directory", "fifo", "oversized", "missing"} {
		t.Run(kind, func(t *testing.T) {
			target := filepath.Join(t.TempDir(), "key")
			var err error
			switch kind {
			case "symlink":
				err = os.Symlink(path, target)
			case "hardlink":
				err = os.Link(path, target)
			case "directory":
				err = os.Mkdir(target, 0700)
			case "fifo":
				err = unix.Mkfifo(target, 0600)
			case "oversized":
				err = os.WriteFile(target, make([]byte, maxCredentialFileBytes+1), 0600)
			}
			if err != nil {
				t.Fatal(err)
			}
			if _, err := readOllamaKeyFile(target); err == nil {
				t.Fatal("unsafe file accepted")
			}
		})
	}
}

func TestOllamaHTTPStatusAndRedirect(t *testing.T) {
	_, data := ollamaFixture(t)
	home := t.TempDir()
	t.Setenv("HOME", home)
	if err := os.Mkdir(filepath.Join(home, ".ollama"), 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(home, ".ollama/id_ed25519"), data, 0600); err != nil {
		t.Fatal(err)
	}
	for _, status := range []int{302, 401, 403, 429, 500} {
		calls := 0
		provider := NewOllamaProvider(&http.Client{Transport: ollamaRoundTrip(func(*http.Request) (*http.Response, error) {
			calls++
			response := ollamaResponse(status, `{}`)
			response.Header.Set("Location", "https://other.example/")
			return response, nil
		})})
		if _, err := provider.Fetch(context.Background()); err == nil || calls != 1 {
			t.Fatalf("status %d: calls %d err %v", status, calls, err)
		}
	}
}
