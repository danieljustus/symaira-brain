package mcp

import (
	"log/slog"
	"net"
	"net/http"
	"strings"
	"sync"
	"time"

	lru "github.com/hashicorp/golang-lru/v2"
	"golang.org/x/time/rate"
)

// RateLimitConfig holds configurable rate limit parameters.
type RateLimitConfig struct {
	DataRPS         float64
	DataBurst       int
	CleanupInterval time.Duration
	LimiterTTL      time.Duration
	MaxEntries      int
}

// DefaultRateLimitConfig returns production defaults: data 100 req/min burst 20.
func DefaultRateLimitConfig() RateLimitConfig {
	return RateLimitConfig{
		DataRPS:         100.0 / 60.0,
		DataBurst:       20,
		CleanupInterval: 5 * time.Minute,
		LimiterTTL:      10 * time.Minute,
		MaxEntries:      10000,
	}
}

type ipLimiter struct {
	limiter  *rate.Limiter
	lastSeen time.Time
}

type RateLimiter struct {
	mu             sync.Mutex
	limiters       *lru.Cache[string, *ipLimiter]
	config         RateLimitConfig
	stopCh         chan struct{}
	trustedProxies []*net.IPNet

	evictions int64
	hits      int64
	misses    int64
}

func NewRateLimiter(cfg RateLimitConfig, trustedProxies ...string) *RateLimiter {
	maxEntries := cfg.MaxEntries
	if maxEntries <= 0 {
		maxEntries = 10000
	}
	cache, _ := lru.New[string, *ipLimiter](maxEntries)
	rl := &RateLimiter{
		limiters: cache,
		config:   cfg,
		stopCh:   make(chan struct{}),
	}
	for _, cidr := range trustedProxies {
		_, network, err := net.ParseCIDR(cidr)
		if err != nil {
			slog.Error("ratelimit: invalid trusted proxy CIDR", "cidr", cidr, "err", err)
			continue
		}
		rl.trustedProxies = append(rl.trustedProxies, network)
	}
	go rl.cleanupLoop()
	return rl
}

func (rl *RateLimiter) Stop() {
	close(rl.stopCh)
}

func (rl *RateLimiter) getLimiter(key string, rps float64, burst int) *rate.Limiter {
	rl.mu.Lock()
	defer rl.mu.Unlock()

	if v, ok := rl.limiters.Get(key); ok {
		v.lastSeen = time.Now()
		rl.hits++
		return v.limiter
	}

	rl.misses++
	limiter := rate.NewLimiter(rate.Limit(rps), burst)
	rl.limiters.Add(key, &ipLimiter{limiter: limiter, lastSeen: time.Now()})
	return limiter
}

func (rl *RateLimiter) cleanup() {
	rl.mu.Lock()
	defer rl.mu.Unlock()

	cutoff := time.Now().Add(-rl.config.LimiterTTL)
	for _, key := range rl.limiters.Keys() {
		if v, ok := rl.limiters.Get(key); ok && v.lastSeen.Before(cutoff) {
			rl.limiters.Remove(key)
			rl.evictions++
		}
	}
}

// Metrics returns current rate limiter metrics.
func (rl *RateLimiter) Metrics() RateLimiterMetrics {
	rl.mu.Lock()
	defer rl.mu.Unlock()
	return RateLimiterMetrics{
		Entries:   int64(rl.limiters.Len()),
		Hits:      rl.hits,
		Misses:    rl.misses,
		Evictions: rl.evictions,
	}
}

// RateLimiterMetrics holds metrics for the rate limiter.
type RateLimiterMetrics struct {
	Entries   int64
	Hits      int64
	Misses    int64
	Evictions int64
}

func (rl *RateLimiter) AllowData(key string) bool {
	return rl.getLimiter("data:"+key, rl.config.DataRPS, rl.config.DataBurst).Allow()
}

func (rl *RateLimiter) cleanupLoop() {
	ticker := time.NewTicker(rl.config.CleanupInterval)
	defer ticker.Stop()
	for {
		select {
		case <-ticker.C:
			rl.cleanup()
		case <-rl.stopCh:
			return
		}
	}
}

// clientIP extracts the client IP, respecting X-Forwarded-For and X-Real-IP only
// when the direct connection IP matches a configured trusted proxy.
func (rl *RateLimiter) clientIP(r *http.Request) string {
	host, _, err := net.SplitHostPort(r.RemoteAddr)
	if err != nil {
		host = r.RemoteAddr
	}

	directIP := net.ParseIP(host)
	if directIP == nil {
		return host
	}

	if !rl.isTrustedProxy(directIP) {
		return host
	}

	if xff := r.Header.Get("X-Forwarded-For"); xff != "" {
		if idx := strings.IndexByte(xff, ','); idx != -1 {
			xff = strings.TrimSpace(xff[:idx])
		}
		if xff != "" {
			return xff
		}
	}
	if xri := r.Header.Get("X-Real-IP"); xri != "" {
		return xri
	}
	return host
}

func (rl *RateLimiter) isTrustedProxy(ip net.IP) bool {
	if len(rl.trustedProxies) == 0 {
		return false
	}
	for _, network := range rl.trustedProxies {
		if network.Contains(ip) {
			return true
		}
	}
	return false
}

func RateLimitMiddleware(rl *RateLimiter, next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		ip := rl.clientIP(r)

		if !rl.AllowData(ip) {
			w.Header().Set("Content-Type", "application/json")
			w.Header().Set("Retry-After", "60")
			w.WriteHeader(http.StatusTooManyRequests)
			if _, err := w.Write([]byte(`{"error":"rate limit exceeded"}`)); err != nil {
				slog.Error("failed to write rate-limit response", "error", err)
			}
			return
		}

		next.ServeHTTP(w, r)
	})
}
