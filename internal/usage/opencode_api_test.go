package usage

import (
	"context"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
	"time"
)

const openCodeAPIExample = `{"usage":{"rolling":{"percent":0,"resetsAt":"2026-10-28T05:00:00Z"},"weekly":{"percent":23.5,"resetsAt":"2026-11-02T00:00:00Z"},"monthly":{"percent":100,"resetsAt":"2026-11-20T00:00:00Z"}}}`

func TestOpenCodeAPIParser(t *testing.T) {
	now, _ := time.Parse(time.RFC3339, "2026-10-28T00:00:00Z")
	snap, err := openCodeParseAPI([]byte(openCodeAPIExample), now, now)
	if err != nil || len(snap.Meters) != 3 {
		t.Fatalf("parse: %v", err)
	}
	if *snap.Meters[0].Used != "0" || snap.Meters[0].ResetsAt != nil || *snap.Meters[1].Used != "23.5" || snap.Meters[1].ResetsAt == nil || snap.Meters[2].Label != "Monthly" {
		t.Fatal(snap)
	}
	partial, err := openCodeParseAPI([]byte(`{"usage":{"rolling":{"resetsAt":"2026-10-28T01:00:00Z"},"weekly":{"percent":0}}}`), now, now)
	if err != nil || partial.Meters[0].Used != nil || *partial.Meters[1].Used != "0" {
		t.Fatalf("absence: %v", err)
	}
	for _, body := range []string{`{}`, `{"usage":{}}`, `{"usage":{"weekly":{"percent":true}}}`, `{"usage":{"weekly":{"percent":"oops"}}}`, `{"usage":{"weekly":{"percent":1,"resetsAt":"oops"}}}`} {
		if _, err := openCodeParseAPI([]byte(body), now, now); err == nil {
			t.Fatalf("accepted %s", body)
		}
	}
	started, err := openCodeParseAPI([]byte(strings.Replace(openCodeAPIExample, "05:00:00", "04:59:57", 1)), now, now)
	if err != nil || started.Meters[0].ResetsAt == nil {
		t.Fatal("rounded zero running session lost")
	}
}
func TestOpenCodeAPITransport(t *testing.T) {
	for _, status := range []int{200, 301, 401, 403, 404, 500} {
		client, tr := scriptedClient(scriptedResponse{status: status, body: []byte(openCodeAPIExample)})
		s := openCodeAPIStrategy{key: "synthetic-key", client: client}
		snap, err := s.Fetch(context.Background())
		if (err == nil) != (status == 200) {
			t.Fatalf("status %d: %v", status, err)
		}
		if status == 200 && snap.Source != "api" {
			t.Fatal(snap)
		}
		if len(tr.requests) != 1 {
			t.Fatal("fallback occurred")
		}
		req := tr.requests[0]
		if req.Method != "GET" || req.URL.String() != openCodeUsageURL || req.Header.Get("Authorization") != "Bearer synthetic-key" || req.Header.Get("Cookie") != "" {
			t.Fatal("wrong request")
		}
		if deadline, ok := req.Context().Deadline(); !ok || time.Until(deadline) > 15*time.Second {
			t.Fatal("missing deadline")
		}
	}
	client, _ := fakeClient(200, []byte(strings.Repeat("x", maxUsageResponseBytes+1)), nil)
	if _, err := (&openCodeAPIStrategy{key: "synthetic-key", client: client}).Fetch(context.Background()); err == nil {
		t.Fatal("oversize accepted")
	}
}
func TestOpenCodeAPIDiscovery(t *testing.T) {
	if runtime.GOOS == "windows" || runtime.GOOS == "plan9" {
		t.Skip("Go native auth.json admission requires Unix; explicit API transport remains covered")
	}
	dir := t.TempDir()
	auth := filepath.Join(dir, "auth.json")
	if err := os.WriteFile(auth, []byte(`{"unrelated":false,"opencode-go":{"key":"synthetic-key"}}`), 0600); err != nil {
		t.Fatal(err)
	}
	key, err := openCodeAuthJSONKey(dir)
	if err != nil || key != "synthetic-key" {
		t.Fatalf("authjson: %v", err)
	}
	db := filepath.Join(dir, "opencode-next.db")
	if err := os.WriteFile(db, nil, 0600); err != nil {
		t.Fatal(err)
	}
	if key, err := openCodeAuthJSONKey(dir); err == nil || key != "" {
		t.Fatal("stale auth resurrected")
	}
	t.Setenv("OPENCODE_DATA_DIR", dir)
	t.Setenv("OPENCODE_GO_API_KEY", "explicit-key")
	t.Setenv("OPENCODE_COOKIE", "session=other")
	p := newCurrentOpenCodeProvider(nil)
	if p.apiKey != "explicit-key" || len(p.Strategies()) != 1 || p.Strategies()[0].Source() != "api" {
		t.Fatal("explicit precedence")
	}
	t.Setenv("OPENCODE_GO_API_KEY", "\n")
	p = newCurrentOpenCodeProvider(nil)
	if p.IsConfigured() || len(p.Strategies()) != 0 || p.AuthStatus().Status != "missing" {
		t.Fatal("invalid API key downgraded")
	}
	t.Setenv("OPENCODE_GO_API_KEY", "")
	p = newCurrentOpenCodeProvider(nil)
	if len(p.Strategies()) != 1 || p.Strategies()[0].Source() != "web" {
		t.Fatal("explicit cookie compatibility")
	}
	t.Setenv("OPENCODE_COOKIE", "")
	t.Setenv("OPENCODE_WORKSPACE_ID", "")
	p = newCurrentOpenCodeProvider(nil)
	if p.IsConfigured() || !strings.Contains(p.AuthStatus().Detail, "database") {
		t.Fatal("unreadable database gate missing")
	}
}
