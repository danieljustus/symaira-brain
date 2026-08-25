package syncclient

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/db"
)

func testMemory(id, content string, updated time.Time) *db.Memory {
	return &db.Memory{
		ID:        id,
		Content:   content,
		Scope:     "global",
		CreatedAt: updated,
		UpdatedAt: updated,
	}
}

func TestClientChanges_QueryAndDecode(t *testing.T) {
	serverTime := time.Date(2026, 8, 25, 10, 0, 0, 0, time.UTC)
	since := time.Date(2026, 8, 1, 0, 0, 0, 0, time.UTC)

	var gotQuery string
	var gotAuth string
	ts := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotQuery = r.URL.RawQuery
		gotAuth = r.Header.Get("Authorization")
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(map[string]any{
			"memories":    []*db.Memory{testMemory("11111111-1111-1111-1111-111111111111", "remote fact", serverTime)},
			"deleted":     []db.DeletedMemory{{ID: "22222222-2222-2222-2222-222222222222", DeletedAt: serverTime}},
			"server_time": serverTime.Format(time.RFC3339),
		})
	}))
	defer ts.Close()

	client := NewClient(ts.URL, "tok-123", nil)
	resp, err := client.Changes(context.Background(), since, "", 100)
	if err != nil {
		t.Fatalf("Changes: %v", err)
	}
	if !strings.Contains(gotQuery, "since=2026-08-01T00%3A00%3A00Z") {
		t.Errorf("query %q missing since parameter", gotQuery)
	}
	if !strings.Contains(gotQuery, "limit=100") {
		t.Errorf("query %q missing limit parameter", gotQuery)
	}
	if gotAuth != "Bearer tok-123" {
		t.Errorf("Authorization = %q, want Bearer tok-123", gotAuth)
	}
	if len(resp.Memories) != 1 || resp.Memories[0].Content != "remote fact" {
		t.Fatalf("memories = %+v", resp.Memories)
	}
	if len(resp.Deleted) != 1 || resp.Deleted[0].ID != "22222222-2222-2222-2222-222222222222" {
		t.Fatalf("deleted = %+v", resp.Deleted)
	}
	if !resp.ServerTime.Equal(serverTime) {
		t.Errorf("server_time = %v, want %v", resp.ServerTime, serverTime)
	}
}

func TestClientChanges_PaginationCursorWinsOverSince(t *testing.T) {
	var queries []string
	ts := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		queries = append(queries, r.URL.RawQuery)
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(map[string]any{
			"memories":    []*db.Memory{},
			"deleted":     []db.DeletedMemory{},
			"server_time": "2026-08-25T10:00:00Z",
			"next_cursor": "bmV4dC1wYWdl",
		})
	}))
	defer ts.Close()

	client := NewClient(ts.URL, "", nil)
	resp, err := client.Changes(context.Background(), time.Date(2026, 1, 1, 0, 0, 0, 0, time.UTC), "bmV4dC1wYWdl", 0)
	if err != nil {
		t.Fatalf("Changes: %v", err)
	}
	if resp.NextCursor != "bmV4dC1wYWdl" {
		t.Errorf("next_cursor = %q", resp.NextCursor)
	}
	for _, q := range queries {
		if strings.Contains(q, "since=") {
			t.Errorf("query %q must not carry since when cursor is set", q)
		}
		if !strings.Contains(q, "cursor=bmV4dC1wYWdl") {
			t.Errorf("query %q missing cursor parameter", q)
		}
	}
}

func TestClientApply_RoundTrip(t *testing.T) {
	var gotBody map[string]any
	ts := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodPost {
			t.Errorf("method = %s, want POST", r.Method)
		}
		if got := r.Header.Get("Content-Type"); !strings.HasPrefix(got, "application/json") {
			t.Errorf("content-type = %q", got)
		}
		_ = json.NewDecoder(r.Body).Decode(&gotBody)
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(map[string]int{
			"applied": 2, "skipped": 0, "deleted": 1,
			"skippedInvalidScope": 0, "skippedInvalidID": 0,
		})
	}))
	defer ts.Close()

	client := NewClient(ts.URL, "", nil)
	res, err := client.Apply(context.Background(),
		[]*db.Memory{testMemory("11111111-1111-1111-1111-111111111111", "a", time.Now())},
		[]db.DeletedMemory{{ID: "22222222-2222-2222-2222-222222222222", DeletedAt: time.Now()}})
	if err != nil {
		t.Fatalf("Apply: %v", err)
	}
	if res.Applied != 2 || res.Deleted != 1 {
		t.Errorf("result = %+v", res)
	}
	if gotBody["memories"] == nil || gotBody["deleted"] == nil {
		t.Errorf("body missing memories/deleted: %v", gotBody)
	}
}

func TestClientRelay_RoundTrip(t *testing.T) {
	blob := []byte("ciphertext-bytes")
	ts := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		switch r.Method {
		case http.MethodGet:
			if got := r.URL.Query().Get("since"); got == "" {
				t.Error("missing since parameter on relay GET")
			}
			_ = json.NewEncoder(w).Encode(map[string]any{
				"blobs":       []db.RelayBlob{{ID: "relay-1", UpdatedAt: time.Date(2026, 8, 25, 0, 0, 0, 0, time.UTC), Blob: blob}},
				"server_time": "2026-08-25T10:00:00.123456Z",
			})
		case http.MethodPost:
			var body struct {
				Blobs []db.RelayBlob `json:"blobs"`
			}
			_ = json.NewDecoder(r.Body).Decode(&body)
			if len(body.Blobs) != 1 || string(body.Blobs[0].Blob) != "ciphertext-bytes" {
				t.Errorf("POST body = %+v", body.Blobs)
			}
			_ = json.NewEncoder(w).Encode(map[string]int{"stored": 1, "skipped": 0})
		default:
			t.Errorf("method = %s", r.Method)
		}
	}))
	defer ts.Close()

	client := NewClient(ts.URL, "", nil)
	pulled, err := client.RelayPull(context.Background(), time.Date(2026, 1, 1, 0, 0, 0, 0, time.UTC), 100)
	if err != nil {
		t.Fatalf("RelayPull: %v", err)
	}
	if len(pulled.Blobs) != 1 || pulled.Blobs[0].ID != "relay-1" {
		t.Fatalf("pulled = %+v", pulled.Blobs)
	}

	got, err := client.RelayPush(context.Background(), []db.RelayBlob{{ID: "relay-1", UpdatedAt: time.Now(), Blob: blob}})
	if err != nil {
		t.Fatalf("RelayPush: %v", err)
	}
	if got.Stored != 1 {
		t.Errorf("stored = %d", got.Stored)
	}
}

func TestClientDo_HTTPError(t *testing.T) {
	ts := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusUnauthorized)
		_ = json.NewEncoder(w).Encode(map[string]string{"error": "invalid or expired token", "code": "FORBIDDEN"})
	}))
	defer ts.Close()

	client := NewClient(ts.URL, "bad-token", nil)
	_, err := client.Changes(context.Background(), time.Time{}, "", 0)
	if err == nil {
		t.Fatal("expected error for 401")
	}
	httpErr, ok := err.(*HTTPError)
	if !ok {
		t.Fatalf("error type = %T, want *HTTPError (%v)", err, err)
	}
	if httpErr.Status != http.StatusUnauthorized || httpErr.Code != "FORBIDDEN" {
		t.Errorf("HTTPError = %+v", httpErr)
	}
}

func TestClientDo_TransportErrorWrapped(t *testing.T) {
	client := NewClient("http://127.0.0.1:1", "", nil) // nothing listens there
	_, err := client.Changes(context.Background(), time.Time{}, "", 0)
	if err == nil {
		t.Fatal("expected connection error")
	}
	if !strings.Contains(err.Error(), "request") {
		t.Errorf("error %q lacks context", err)
	}
}
