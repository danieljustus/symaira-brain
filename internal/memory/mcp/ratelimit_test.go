package mcp

import (
	"net/http"
	"net/http/httptest"
	"testing"
	"time"
)

func TestRateLimiter_AllowData(t *testing.T) {
	cfg := RateLimitConfig{
		DataRPS:         1.0,
		DataBurst:       3,
		CleanupInterval: time.Hour,
		LimiterTTL:      time.Hour,
	}
	rl := NewRateLimiter(cfg)
	defer rl.Stop()

	for i := 0; i < 3; i++ {
		if !rl.AllowData("10.0.0.1") {
			t.Fatalf("data request %d should be allowed (burst=3)", i+1)
		}
	}
	if rl.AllowData("10.0.0.1") {
		t.Fatal("fourth data request should be denied (burst exhausted)")
	}
}

func TestRateLimiter_PerIPIsolation(t *testing.T) {
	cfg := RateLimitConfig{
		DataRPS:         1.0,
		DataBurst:       1,
		CleanupInterval: time.Hour,
		LimiterTTL:      time.Hour,
	}
	rl := NewRateLimiter(cfg)
	defer rl.Stop()

	if !rl.AllowData("1.1.1.1") {
		t.Fatal("first request from IP 1.1.1.1 should be allowed")
	}
	if rl.AllowData("1.1.1.1") {
		t.Fatal("second request from IP 1.1.1.1 should be denied")
	}
	if !rl.AllowData("2.2.2.2") {
		t.Fatal("first request from IP 2.2.2.2 should be allowed (separate limiter)")
	}
}

func TestClientIP_XForwardedFor(t *testing.T) {
	rl := NewRateLimiter(RateLimitConfig{
		DataRPS:         10.0,
		DataBurst:       10,
		CleanupInterval: time.Hour,
		LimiterTTL:      time.Hour,
	}, "127.0.0.0/8")
	defer rl.Stop()

	r := httptest.NewRequest("GET", "/", nil)
	r.RemoteAddr = "127.0.0.1:1234"
	r.Header.Set("X-Forwarded-For", "203.0.113.50, 70.41.3.18")
	if got := rl.clientIP(r); got != "203.0.113.50" {
		t.Fatalf("expected 203.0.113.50, got %s", got)
	}
}

func TestClientIP_XRealIP(t *testing.T) {
	rl := NewRateLimiter(RateLimitConfig{
		DataRPS:         10.0,
		DataBurst:       10,
		CleanupInterval: time.Hour,
		LimiterTTL:      time.Hour,
	}, "127.0.0.0/8")
	defer rl.Stop()

	r := httptest.NewRequest("GET", "/", nil)
	r.RemoteAddr = "127.0.0.1:1234"
	r.Header.Set("X-Real-IP", "198.51.100.17")
	if got := rl.clientIP(r); got != "198.51.100.17" {
		t.Fatalf("expected 198.51.100.17, got %s", got)
	}
}

func TestClientIP_RemoteAddr(t *testing.T) {
	rl := NewRateLimiter(RateLimitConfig{
		DataRPS:         10.0,
		DataBurst:       10,
		CleanupInterval: time.Hour,
		LimiterTTL:      time.Hour,
	})
	defer rl.Stop()

	r := httptest.NewRequest("GET", "/", nil)
	r.RemoteAddr = "192.168.1.100:12345"
	if got := rl.clientIP(r); got != "192.168.1.100" {
		t.Fatalf("expected 192.168.1.100, got %s", got)
	}
}

func TestRateLimitMiddleware_Allowed(t *testing.T) {
	cfg := RateLimitConfig{
		DataRPS:         10.0,
		DataBurst:       10,
		CleanupInterval: time.Hour,
		LimiterTTL:      time.Hour,
	}
	rl := NewRateLimiter(cfg)
	defer rl.Stop()

	inner := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusOK)
	})
	handler := RateLimitMiddleware(rl, inner)

	req := httptest.NewRequest("GET", "/api/list", nil)
	req.RemoteAddr = "10.0.0.1:9999"
	rec := httptest.NewRecorder()
	handler.ServeHTTP(rec, req)

	if rec.Code != http.StatusOK {
		t.Fatalf("expected 200, got %d", rec.Code)
	}
}

func TestRateLimitMiddleware_Denied(t *testing.T) {
	cfg := RateLimitConfig{
		DataRPS:         1.0,
		DataBurst:       1,
		CleanupInterval: time.Hour,
		LimiterTTL:      time.Hour,
	}
	rl := NewRateLimiter(cfg)
	defer rl.Stop()

	inner := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusOK)
	})
	handler := RateLimitMiddleware(rl, inner)

	req1 := httptest.NewRequest("GET", "/api/list", nil)
	req1.RemoteAddr = "10.0.0.2:9999"
	rec1 := httptest.NewRecorder()
	handler.ServeHTTP(rec1, req1)
	if rec1.Code != http.StatusOK {
		t.Fatalf("first request: expected 200, got %d", rec1.Code)
	}

	req2 := httptest.NewRequest("GET", "/api/list", nil)
	req2.RemoteAddr = "10.0.0.2:9999"
	rec2 := httptest.NewRecorder()
	handler.ServeHTTP(rec2, req2)
	if rec2.Code != http.StatusTooManyRequests {
		t.Fatalf("second request: expected 429, got %d", rec2.Code)
	}
	if rec2.Header().Get("Retry-After") != "60" {
		t.Fatalf("expected Retry-After: 60, got %s", rec2.Header().Get("Retry-After"))
	}
}

func TestDefaultRateLimitConfig(t *testing.T) {
	cfg := DefaultRateLimitConfig()
	if cfg.DataRPS <= 0 || cfg.DataBurst <= 0 {
		t.Fatal("data limits must be positive")
	}
}

// TestRateLimitMiddleware_BurstExhaustion_429 verifies that after the burst
// budget is exhausted the middleware returns 429 for every subsequent request.
// This is a regression test for #530: the rate limiter was not wired into the
// httpMux after the refactor in commit eedfcbd.
func TestRateLimitMiddleware_BurstExhaustion_429(t *testing.T) {
	const burst = 20
	cfg := RateLimitConfig{
		DataRPS:         100.0 / 60.0, // 100 req/min
		DataBurst:       burst,
		CleanupInterval: time.Hour,
		LimiterTTL:      time.Hour,
	}
	rl := NewRateLimiter(cfg)
	defer rl.Stop()

	inner := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusOK)
	})
	handler := RateLimitMiddleware(rl, inner)

	// Consume the entire burst budget.
	for i := 0; i < burst; i++ {
		req := httptest.NewRequest("GET", "/api/list", nil)
		req.RemoteAddr = "10.0.0.50:9999"
		rec := httptest.NewRecorder()
		handler.ServeHTTP(rec, req)
		if rec.Code != http.StatusOK {
			t.Fatalf("burst request %d: expected 200, got %d", i+1, rec.Code)
		}
	}

	// The next request must be rate-limited.
	req := httptest.NewRequest("GET", "/api/list", nil)
	req.RemoteAddr = "10.0.0.50:9999"
	rec := httptest.NewRecorder()
	handler.ServeHTTP(rec, req)
	if rec.Code != http.StatusTooManyRequests {
		t.Fatalf("post-burst request: expected 429, got %d", rec.Code)
	}
	if rec.Header().Get("Retry-After") != "60" {
		t.Fatalf("expected Retry-After: 60, got %s", rec.Header().Get("Retry-After"))
	}
}
