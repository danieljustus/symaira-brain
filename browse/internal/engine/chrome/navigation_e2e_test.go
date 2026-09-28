package chrome

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"sync/atomic"
	"testing"
	"time"

	"github.com/danieljustus/symaira-browse/internal/engine"
)

func TestNavigationLoadParityRedirectReloadAndHTTPError(t *testing.T) {
	if os.Getenv("SYMBROWSE_E2E") != "1" {
		t.Skip("set SYMBROWSE_E2E=1 to run the real Chrome navigation parity test")
	}
	executable := os.Getenv("SYMBROWSE_CHROME_EXECUTABLE")
	if executable == "" {
		t.Fatal("SYMBROWSE_CHROME_EXECUTABLE is required for the real Chrome navigation parity test")
	}

	var pageRequests atomic.Int64
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/redirect":
			http.Redirect(w, r, "/page", http.StatusFound)
		case "/page":
			pageRequests.Add(1)
			w.Header().Set("Content-Type", "text/html; charset=utf-8")
			_, _ = fmt.Fprint(w, "<!doctype html><title>go012</title><main>loaded</main>")
		case "/server-error":
			w.WriteHeader(http.StatusInternalServerError)
			_, _ = fmt.Fprint(w, "<!doctype html><title>server-error</title>")
		default:
			http.NotFound(w, r)
		}
	}))
	defer server.Close()

	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	profile := filepath.Join(t.TempDir(), "chrome-profile")
	chrome := New(Options{
		ExecutablePath: executable,
		UserDataDir:    profile,
		ManagedProfile: true,
		Headless:       true,
		AllowPrivate:   true,
		StartupTimeout: 20 * time.Second,
		RequestTimeout: 10 * time.Second,
	})
	if err := chrome.Launch(ctx); err != nil {
		t.Fatalf("launch Chrome: %v", err)
	}
	defer func() {
		if err := chrome.Close(); err != nil {
			t.Errorf("close Chrome: %v", err)
		}
	}()
	browserContext, err := chrome.NewContext(ctx)
	if err != nil {
		t.Fatalf("create browser context: %v", err)
	}
	page, err := chrome.NewPage(ctx, browserContext, "about:blank")
	if err != nil {
		t.Fatalf("create blank page: %v", err)
	}
	service := engine.NewNavigationService(chrome, page, engine.NavigationOptions{
		Timeout:      10 * time.Second,
		PollInterval: 10 * time.Millisecond,
	})

	redirected, err := service.Open(ctx, server.URL+"/redirect")
	if err != nil {
		t.Fatalf("open redirect: %v", err)
	}
	if redirected.URL != server.URL+"/page" || redirected.HTTPStatus != http.StatusOK {
		t.Fatalf("redirect outcome = %+v, want final /page with HTTP 200", redirected)
	}
	assertChromeDocumentComplete(t, ctx, chrome, page)

	beforeReload := pageRequests.Load()
	if _, err := service.Open(ctx, server.URL+"/page"); err != nil {
		t.Fatalf("open same URL again: %v", err)
	}
	if pageRequests.Load() <= beforeReload {
		t.Fatal("opening the same URL again did not issue another document request")
	}
	assertChromeDocumentComplete(t, ctx, chrome, page)

	serverError, err := service.Open(ctx, server.URL+"/server-error")
	if err != nil {
		t.Fatalf("open HTTP 500 response: %v", err)
	}
	if serverError.HTTPStatus != http.StatusInternalServerError {
		t.Fatalf("HTTP error outcome = %+v, want HTTP 500", serverError)
	}
	assertChromeDocumentComplete(t, ctx, chrome, page)

	if _, err := service.Open(ctx, "http://"); err == nil {
		t.Fatal("malformed navigation URL unexpectedly succeeded")
	}
}

func assertChromeDocumentComplete(t *testing.T, ctx context.Context, chrome *Engine, page engine.Page) {
	t.Helper()
	result, err := chrome.Evaluate(ctx, page, "document.readyState")
	if err != nil {
		t.Fatalf("read document.readyState: %v", err)
	}
	if result.ExceptionText != "" {
		t.Fatalf("read document.readyState exception: %s", result.ExceptionText)
	}
	var state string
	if err := json.Unmarshal(result.Value, &state); err != nil {
		t.Fatalf("decode document.readyState %s: %v", result.Value, err)
	}
	if state != "complete" {
		t.Fatalf("document.readyState = %q, want complete", state)
	}
}
