package mcp

import (
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
	"github.com/danieljustus/symaira-brain/internal/memory/security"
)

// failingRevocationStore reports persistence failures so tests can verify the
// revoke endpoint surfaces them as 500 while the in-memory revocation still
// takes effect.
type failingRevocationStore struct{}

func (failingRevocationStore) RevokeToken(jti string) error {
	return errors.New("test store failure")
}

func (failingRevocationStore) IsTokenRevoked(jti string) (bool, error) {
	return false, nil
}

func helperServerWithStore(t *testing.T, store security.RevocationStore) *Server {
	t.Helper()
	database := helperDB(t)
	jwtProvider, err := security.NewJWTProvider(config.Defaults(), store)
	if err != nil {
		t.Fatalf("failed to create JWT provider: %v", err)
	}
	return NewServer(database, jwtProvider, "test", config.Defaults())
}

// postRevoke issues a POST to /api/token/revoke with an optional Bearer token
// and JSON body, returning the response and its decoded JSON body.
func postRevoke(t *testing.T, ts *httptest.Server, token, body string) (*http.Response, map[string]string) {
	t.Helper()
	req, err := http.NewRequest("POST", ts.URL+"/api/token/revoke", strings.NewReader(body))
	if err != nil {
		t.Fatalf("failed to build request: %v", err)
	}
	req.Header.Set("Content-Type", "application/json")
	if token != "" {
		req.Header.Set("Authorization", "Bearer "+token)
	}
	res, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("request failed: %v", err)
	}
	defer func() { _ = res.Body.Close() }()
	var out map[string]string
	_ = json.NewDecoder(res.Body).Decode(&out)
	return res, out
}

func TestTokenRevokeEndpointByJTI(t *testing.T) {
	s := helperServer(t)
	ts := httptest.NewServer(s.httpMux())
	defer ts.Close()

	token := helperAuthToken(t, s)
	payload, err := s.jwts.VerifyToken(token)
	if err != nil {
		t.Fatalf("token should verify before revocation: %v", err)
	}

	body, err := json.Marshal(map[string]string{"jti": payload.JWTID})
	if err != nil {
		t.Fatalf("failed to marshal body: %v", err)
	}
	res, out := postRevoke(t, ts, token, string(body))
	if res.StatusCode != http.StatusOK {
		t.Fatalf("expected 200, got %d (%v)", res.StatusCode, out)
	}
	if out["status"] != "revoked" || out["jti"] != payload.JWTID {
		t.Errorf("unexpected response body: %v", out)
	}

	if _, err := s.jwts.VerifyToken(token); err == nil || !strings.Contains(err.Error(), "revoked") {
		t.Errorf("token should fail verification after HTTP revoke, got: %v", err)
	}
}

func TestTokenRevokeEndpointByTokenValue(t *testing.T) {
	s := helperServer(t)
	ts := httptest.NewServer(s.httpMux())
	defer ts.Close()

	token := helperAuthToken(t, s)
	body, err := json.Marshal(map[string]string{"token": token})
	if err != nil {
		t.Fatalf("failed to marshal body: %v", err)
	}
	res, out := postRevoke(t, ts, token, string(body))
	if res.StatusCode != http.StatusOK {
		t.Fatalf("expected 200, got %d (%v)", res.StatusCode, out)
	}
	if out["status"] != "revoked" {
		t.Errorf("unexpected response body: %v", out)
	}

	if _, err := s.jwts.VerifyToken(token); err == nil || !strings.Contains(err.Error(), "revoked") {
		t.Errorf("token should fail verification after HTTP revoke by value, got: %v", err)
	}
}

func TestTokenRevokeEndpointRequiresAuth(t *testing.T) {
	s := helperServer(t)
	ts := httptest.NewServer(s.httpMux())
	defer ts.Close()

	// CSRF blocks unauthenticated state-changing requests before the auth
	// check, exactly like the other write endpoints (see TestSyncApplyUnauthenticated).
	res, _ := postRevoke(t, ts, "", `{"jti":"some-jti"}`)
	if res.StatusCode != http.StatusForbidden {
		t.Errorf("expected 403 (CSRF) without auth, got %d", res.StatusCode)
	}

	// An invalid bearer token reaches the auth middleware and is rejected.
	req, err := http.NewRequest("POST", ts.URL+"/api/token/revoke", strings.NewReader(`{"jti":"some-jti"}`))
	if err != nil {
		t.Fatalf("failed to build request: %v", err)
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Authorization", "Bearer not-a-real-token-value")
	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("request failed: %v", err)
	}
	defer func() { _ = resp.Body.Close() }()
	if resp.StatusCode != http.StatusUnauthorized {
		t.Errorf("expected 401 for invalid bearer token, got %d", resp.StatusCode)
	}
}

func TestTokenRevokeEndpointBadRequest(t *testing.T) {
	s := helperServer(t)
	ts := httptest.NewServer(s.httpMux())
	defer ts.Close()
	token := helperAuthToken(t, s)

	// Malformed JSON body.
	res, _ := postRevoke(t, ts, token, `{"jti":"broken`)
	if res.StatusCode != http.StatusBadRequest {
		t.Errorf("expected 400 for malformed body, got %d", res.StatusCode)
	}
	// Missing both fields.
	res, _ = postRevoke(t, ts, token, `{}`)
	if res.StatusCode != http.StatusBadRequest {
		t.Errorf("expected 400 for missing fields, got %d", res.StatusCode)
	}
	// Token value that is not a JWT.
	res, _ = postRevoke(t, ts, token, `{"token":"not.a.jwt"}`)
	if res.StatusCode != http.StatusBadRequest {
		t.Errorf("expected 400 for invalid token value, got %d", res.StatusCode)
	}
	// GET is not allowed.
	req, err := http.NewRequest("GET", ts.URL+"/api/token/revoke", nil)
	if err != nil {
		t.Fatalf("failed to build request: %v", err)
	}
	req.Header.Set("Authorization", "Bearer "+token)
	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("request failed: %v", err)
	}
	defer func() { _ = resp.Body.Close() }()
	if resp.StatusCode != http.StatusMethodNotAllowed {
		t.Errorf("expected 405 for GET, got %d", resp.StatusCode)
	}
}

func TestTokenRevokeEndpointPersistenceFailure(t *testing.T) {
	s := helperServerWithStore(t, failingRevocationStore{})
	ts := httptest.NewServer(s.httpMux())
	defer ts.Close()

	token := helperAuthToken(t, s)
	payload, err := s.jwts.VerifyToken(token)
	if err != nil {
		t.Fatalf("token should verify before revocation: %v", err)
	}

	body, err := json.Marshal(map[string]string{"jti": payload.JWTID})
	if err != nil {
		t.Fatalf("failed to marshal body: %v", err)
	}
	res, out := postRevoke(t, ts, token, string(body))
	if res.StatusCode != http.StatusInternalServerError {
		t.Fatalf("expected 500 on persistence failure, got %d", res.StatusCode)
	}
	if !strings.Contains(out["error"], "persistence failed") {
		t.Errorf("expected clear persistence-failure message, got: %v", out)
	}

	// The in-memory fallback still revokes the token immediately.
	if _, err := s.jwts.VerifyToken(token); err == nil || !strings.Contains(err.Error(), "revoked") {
		t.Errorf("token should still be revoked in memory, got: %v", err)
	}
}
