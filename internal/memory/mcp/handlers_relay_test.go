package mcp

import (
	"bytes"
	"encoding/json"
	"net/http/httptest"
	"testing"
)

func TestHandleSyncRelay_PostThenGet(t *testing.T) {
	s := helperServer(t)

	body, _ := json.Marshal(map[string]interface{}{
		"blobs": []map[string]interface{}{
			{"id": "relay-http-1", "updated_at": "2026-07-23T00:00:00Z", "blob": []byte("ciphertext")},
			{"id": "", "updated_at": "2026-07-23T00:00:00Z", "blob": []byte("skipped-no-id")},
		},
	})
	req := httptest.NewRequest("POST", "/api/sync/relay", bytes.NewReader(body))
	rec := httptest.NewRecorder()
	s.handleSyncRelay(rec, req)

	if rec.Code != 200 {
		t.Fatalf("expected 200, got %d: %s", rec.Code, rec.Body.String())
	}
	var postResp map[string]int
	if err := json.Unmarshal(rec.Body.Bytes(), &postResp); err != nil {
		t.Fatalf("failed to decode response: %v", err)
	}
	if postResp["stored"] != 1 || postResp["skipped"] != 1 {
		t.Errorf("expected stored=1 skipped=1, got %+v", postResp)
	}

	getReq := httptest.NewRequest("GET", "/api/sync/relay?since=2026-01-01T00:00:00Z", nil)
	getRec := httptest.NewRecorder()
	s.handleSyncRelay(getRec, getReq)

	if getRec.Code != 200 {
		t.Fatalf("expected 200, got %d: %s", getRec.Code, getRec.Body.String())
	}
	var getResp struct {
		Blobs []struct {
			ID string `json:"id"`
		} `json:"blobs"`
	}
	if err := json.Unmarshal(getRec.Body.Bytes(), &getResp); err != nil {
		t.Fatalf("failed to decode GET response: %v", err)
	}
	if len(getResp.Blobs) != 1 || getResp.Blobs[0].ID != "relay-http-1" {
		t.Errorf("expected one blob relay-http-1, got %+v", getResp.Blobs)
	}
}

func TestHandleSyncRelay_MethodNotAllowed(t *testing.T) {
	s := helperServer(t)
	req := httptest.NewRequest("DELETE", "/api/sync/relay", nil)
	rec := httptest.NewRecorder()
	s.handleSyncRelay(rec, req)

	if rec.Code != 405 {
		t.Errorf("expected 405 for DELETE, got %d", rec.Code)
	}
}

func TestHandleSyncRelay_InvalidSinceParameter(t *testing.T) {
	s := helperServer(t)
	req := httptest.NewRequest("GET", "/api/sync/relay?since=not-a-time", nil)
	rec := httptest.NewRecorder()
	s.handleSyncRelay(rec, req)

	if rec.Code != 400 {
		t.Errorf("expected 400 for invalid since parameter, got %d", rec.Code)
	}
}

func TestHandleSyncRelay_InvalidBody(t *testing.T) {
	s := helperServer(t)
	req := httptest.NewRequest("POST", "/api/sync/relay", bytes.NewReader([]byte("not json")))
	rec := httptest.NewRecorder()
	s.handleSyncRelay(rec, req)

	if rec.Code != 400 {
		t.Errorf("expected 400 for invalid body, got %d", rec.Code)
	}
}
