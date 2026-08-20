package mcp

import (
	"context"
	"fmt"
	"log/slog"
	"net"
	"net/http"
	"net/netip"
	"os"
	"os/signal"
	"strings"
	"syscall"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/security"
)

func (s *Server) StartHTTPServer(port int) error {
	addr := fmt.Sprintf("127.0.0.1:%d", port)
	s.bindAddr = addr
	srv := &http.Server{
		Addr:              addr,
		Handler:           s.httpMux(),
		ReadHeaderTimeout: 5 * time.Second,
		ReadTimeout:       30 * time.Second,
		WriteTimeout:      60 * time.Second,
		IdleTimeout:       120 * time.Second,
		MaxHeaderBytes:    1 << 20, // 1 MiB
	}

	ln, err := net.Listen("tcp", addr)
	if err != nil {
		return fmt.Errorf("failed to bind to %s: %w", addr, err)
	}
	slog.Info("Symaira Memory API listening", "addr", addr, "url", fmt.Sprintf("http://%s", addr))

	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()

	errCh := make(chan error, 1)
	go func() {
		errCh <- srv.Serve(ln)
	}()

	select {
	case err := <-errCh:
		return err
	case <-ctx.Done():
		s.rateLimiter.Stop()
		shutdownCtx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		return srv.Shutdown(shutdownCtx)
	}
}

// routeAuth is the access level a route requires. The mux wraps each
// route's handler with the matching AuthMiddleware chain so RequireAuth /
// RequireRole stay the single implementation of auth semantics.
type routeAuth int

const (
	authNone routeAuth = iota
	authRequired
	authReadWrite
)

func (s *Server) httpMux() http.Handler {
	mux := http.NewServeMux()

	routes := []struct {
		pattern string
		handler http.HandlerFunc
		auth    routeAuth
	}{
		{"/api/status", s.handleStatus, authNone},
		{"/api/stats", s.handleStats, authRequired},
		{"/api/search", s.handleSearch, authRequired},
		{"/api/set", s.handleSet, authReadWrite},
		{"/api/list", s.handleList, authRequired},
		{"/api/sync/changes", s.handleSyncChanges, authRequired},
		{"/api/sync/apply", s.handleSyncApply, authReadWrite},
		{"/api/sync/relay", s.handleSyncRelay, authReadWrite},
		{"/api/get", s.handleGet, authRequired},
		{"/api/delete", s.handleDelete, authReadWrite},
		{"/api/rules", s.handleRules, authRequired},
		{"/api/entities", s.handleEntities, authRequired},
		{"/api/token/revoke", s.handleTokenRevoke, authRequired},
	}
	for _, rt := range routes {
		var h http.Handler = rt.handler
		switch rt.auth {
		case authReadWrite:
			h = s.auth.RequireAuth(s.auth.RequireRole(security.RoleReadWrite)(h))
		case authRequired:
			h = s.auth.RequireAuth(h)
		}
		mux.Handle(rt.pattern, s.cors.Handler(h))
	}
	mux.HandleFunc("/", s.handleStatic)

	var handler http.Handler = mux
	handler = s.hostValidationHandler(handler)
	handler = csrfProtectionHandler(handler)
	handler = securityHeadersHandler(handler)
	handler = requestLoggingMiddleware(handler)
	return RateLimitMiddleware(s.rateLimiter, handler)
}

// hostValidationHandler returns a middleware that validates the Host header
// when the server is bound to a loopback address. The guard is active only
// when the bind address is loopback (127.0.0.0/8 or ::1). When active, any
// request with a non-loopback Host header receives a 403 Forbidden response,
// regardless of HTTP method.
//
// Loopback detection uses netip.Addr.IsLoopback(), which covers the entire
// 127.0.0.0/8 range (not only 127.0.0.1) and ::1 for IPv6.
func (s *Server) hostValidationHandler(next http.Handler) http.Handler {
	bindHost, _, err := net.SplitHostPort(s.bindAddr)
	if err != nil {
		bindHost = s.bindAddr
	}

	bindIP, err := netip.ParseAddr(bindHost)
	if err != nil || !bindIP.IsLoopback() {
		// Not bound to a loopback address — guard is inactive.
		return next
	}

	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		reqHost, _, err := net.SplitHostPort(r.Host)
		if err != nil {
			reqHost = r.Host // no port present, use as-is
		}

		// Strip IPv6 brackets (e.g. [::1] -> ::1)
		addr := reqHost
		if len(addr) > 2 && addr[0] == '[' && addr[len(addr)-1] == ']' {
			addr = addr[1 : len(addr)-1]
		}

		// Check if the Host is a loopback IP (covers 127.0.0.0/8 and ::1).
		if ip, err := netip.ParseAddr(addr); err == nil && ip.IsLoopback() {
			next.ServeHTTP(w, r)
			return
		}

		// Also allow "localhost" as a valid loopback hostname.
		if strings.EqualFold(reqHost, "localhost") {
			next.ServeHTTP(w, r)
			return
		}

		slog.Warn("Host header validation rejected", "host", r.Host, "method", r.Method, "path", r.URL.Path)
		writeJSONError(w, http.StatusForbidden, CodeForbidden, "non-loopback Host header rejected", nil)
	})
}

func csrfProtectionHandler(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.Method {
		case "POST", "DELETE", "PUT", "PATCH":
		default:
			next.ServeHTTP(w, r)
			return
		}

		if strings.HasPrefix(r.Header.Get("Authorization"), "Bearer ") {
			next.ServeHTTP(w, r)
			return
		}

		origin := r.Header.Get("Origin")
		if origin != "" && isLocalOrigin(origin) {
			next.ServeHTTP(w, r)
			return
		}

		if strings.EqualFold(r.Header.Get("X-Requested-With"), "XMLHttpRequest") {
			next.ServeHTTP(w, r)
			return
		}

		slog.Warn("CSRF blocked request", "method", r.Method, "path", r.URL.Path, "origin", r.Header.Get("Origin"))
		writeJSONError(w, http.StatusForbidden, CodeForbidden, "CSRF validation failed", nil)
	})
}

func isLocalOrigin(origin string) bool {
	if origin == "" {
		return false
	}
	rest := origin
	if strings.HasPrefix(rest, "http://") {
		rest = rest[7:]
	} else if strings.HasPrefix(rest, "https://") {
		rest = rest[8:]
	} else {
		return false
	}
	// Properly split host and port, handling IPv6 addresses in brackets.
	host := rest
	if h, _, err := net.SplitHostPort(host); err == nil {
		host = h
	} else if len(host) > 2 && host[0] == '[' && host[len(host)-1] == ']' {
		// No port present — strip brackets for IPv6 address.
		host = host[1 : len(host)-1]
	}
	// Use netip for loopback detection, covering the entire 127.0.0.0/8 range
	// and ::1. Previously 0.0.0.0 was treated as local — it is not.
	if ip, err := netip.ParseAddr(host); err == nil && ip.IsLoopback() {
		return true
	}
	return host == "localhost"
}

func matchOrigin(origin, pattern string) bool {
	if pattern == "*" {
		return true
	}
	if strings.HasSuffix(pattern, "://*") {
		scheme := strings.TrimSuffix(pattern, "://*")
		return strings.HasPrefix(origin, scheme+"://")
	}
	return origin == pattern
}

func securityHeadersHandler(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Security-Policy", "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'")
		w.Header().Set("X-Content-Type-Options", "nosniff")
		w.Header().Set("X-Frame-Options", "DENY")
		next.ServeHTTP(w, r)
	})
}

type responseCapture struct {
	http.ResponseWriter
	statusCode int
}

func (rc *responseCapture) WriteHeader(code int) {
	rc.statusCode = code
	rc.ResponseWriter.WriteHeader(code)
}

func requestLoggingMiddleware(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		start := time.Now()
		rc := &responseCapture{ResponseWriter: w, statusCode: http.StatusOK}
		next.ServeHTTP(rc, r)
		slog.Info("HTTP request",
			"method", r.Method,
			"path", r.URL.Path,
			"status", rc.statusCode,
			"duration", time.Since(start).String(),
		)
	})
}
