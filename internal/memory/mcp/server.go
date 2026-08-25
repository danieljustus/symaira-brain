package mcp

import (
	"encoding/json"
	"fmt"
	"log/slog"
	"net/http"
	"sync"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
	"github.com/danieljustus/symaira-brain/internal/memory/conflict"
	"github.com/danieljustus/symaira-brain/internal/memory/db"
	"github.com/danieljustus/symaira-brain/internal/memory/extractor"
	"github.com/danieljustus/symaira-brain/internal/memory/security"
)

// Standard HTTP error codes for API responses.
const (
	CodeInvalidRequest   = "INVALID_REQUEST"
	CodeNotFound         = "NOT_FOUND"
	CodeForbidden        = "FORBIDDEN"
	CodeInternal         = "INTERNAL_ERROR"
	CodeMethodNotAllowed = "METHOD_NOT_ALLOWED"
)

// SystemStats holds a combined snapshot of all live system statistics.
type SystemStats struct {
	Retrieval db.RetrievalStatsSnapshot  `json:"retrieval"`
	Embedding extractor.EmbeddingMetrics `json:"embedding"`
	Database  db.DBMetrics               `json:"database"`
}

type Server struct {
	service          *MemoryService
	auth             *AuthMiddleware
	cors             *CORSMiddleware
	jwts             *security.JWTProvider
	version          string
	cfg              *config.Config
	profile          *db.Profile
	rateLimiter      *RateLimiter
	workingMemoryTTL time.Duration
	bindAddr         string

	// clientMu guards the MCP client attribution state below.
	clientMu sync.Mutex
	// clientIdentity is captured from the initialize handshake clientInfo.
	clientIdentity ClientIdentity
	// overrideClientID, when non-empty, pins attribution and wins over the
	// handshake identity (serve --client-id / [mcp] client_id config).
	overrideClientID string
}

func NewServer(database *db.DB, jwtProvider *security.JWTProvider, version string, cfg *config.Config) *Server {
	embeddings := extractor.NewEmbeddingsGenerator(cfg)
	service := NewMemoryService(database, embeddings, true)
	service.SetRankingWeights(db.WeightsFromConfig(cfg.Ranking))
	if cfg.Conflict.Enabled {
		// Write-path contradiction detection (#462): staged writes are
		// excluded inside SetGoverned via checkerForStaged.
		service.SetConflictChecker(conflict.NewChecker(database, cfg.Conflict))
	}
	auth := NewAuthMiddleware(jwtProvider, database, cfg.Security.RequireProfile)
	cors := NewCORSMiddleware([]string{"chrome-extension://*", "moz-extension://*"})

	var workingTTL time.Duration
	if cfg.WorkingMemory.TTL != "" {
		if d, err := time.ParseDuration(cfg.WorkingMemory.TTL); err == nil {
			workingTTL = d
		}
	}
	if workingTTL == 0 {
		workingTTL = 24 * time.Hour
	}

	return &Server{
		service:          service,
		auth:             auth,
		cors:             cors,
		jwts:             jwtProvider,
		version:          version,
		cfg:              cfg,
		rateLimiter:      NewRateLimiter(DefaultRateLimitConfig(), cfg.Security.TrustedProxies...),
		workingMemoryTTL: workingTTL,
		bindAddr:         "127.0.0.1:8787",
		overrideClientID: cfg.MCP.ClientID,
	}
}

func (s *Server) SetPIIEnabled(enabled bool) {
	s.service.SetPIIEnabled(enabled)
}

func (s *Server) SetAllowedOrigins(origins []string) {
	s.cors = NewCORSMiddleware(origins)
}

func (s *Server) SetProfile(p *db.Profile) {
	s.profile = p
	s.auth.SetProfile(p)
}

// Stats returns combined system statistics (retrieval + embedding + database).
func (s *Server) Stats() SystemStats {
	return SystemStats{
		Retrieval: s.service.RetrievalStats().Snapshot(),
		Embedding: s.service.EmbeddingMetrics(),
		Database:  s.service.db.Metrics(),
	}
}

func (s *Server) DB() *db.DB {
	return s.service.db
}

// HTTPHandler returns the full HTTP API handler (routes plus auth, CORS,
// CSRF and rate-limit middleware). StartHTTPServer binds this handler to a
// listener; embedding it via http.ServeMux/httptest lets callers control the
// listener themselves (custom ports, tests) without a separate binary.
func (s *Server) HTTPHandler() http.Handler {
	return s.httpMux()
}

func writeJSONError(w http.ResponseWriter, status int, code string, safeMsg string, internal error) {
	if internal != nil {
		slog.Error("HTTP error", "code", code, "msg", safeMsg, "err", internal)
	}
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(map[string]string{
		"error": safeMsg,
		"code":  code,
	})
}

func mcpError(safeMsg string, internal error) (any, error) {
	if internal != nil {
		slog.Error("MCP error", "msg", safeMsg, "err", internal)
	}
	return nil, fmt.Errorf("%s", safeMsg)
}
