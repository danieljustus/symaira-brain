package mcp

import (
	"net/http"
)

// CORSMiddleware handles origin validation and CORS headers.
type CORSMiddleware struct {
	allowedOrigins []string
}

// NewCORSMiddleware creates a CORS middleware with the given allowed origins.
func NewCORSMiddleware(allowedOrigins []string) *CORSMiddleware {
	return &CORSMiddleware{allowedOrigins: allowedOrigins}
}

// Handler returns an http.Handler that wraps the next handler with CORS logic.
func (c *CORSMiddleware) Handler(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		origin := r.Header.Get("Origin")
		allowed := false
		for _, o := range c.allowedOrigins {
			if matchOrigin(origin, o) {
				allowed = true
				break
			}
		}
		if !allowed {
			if origin != "" {
				writeJSONError(w, http.StatusForbidden, CodeForbidden, "origin not allowed", nil)
				return
			}
		} else {
			w.Header().Set("Access-Control-Allow-Origin", origin)
		}
		w.Header().Set("Access-Control-Allow-Methods", "POST, GET, OPTIONS, DELETE")
		w.Header().Set("Access-Control-Allow-Headers", "Content-Type, Authorization")
		if r.Method == "OPTIONS" {
			w.WriteHeader(http.StatusOK)
			return
		}
		next.ServeHTTP(w, r)
	})
}
