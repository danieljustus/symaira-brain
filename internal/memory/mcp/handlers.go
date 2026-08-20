package mcp

import (
	"encoding/base64"
	"encoding/json"
	"fmt"
	"log/slog"
	"net/http"
	"strconv"
	"strings"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/db"
	"github.com/danieljustus/symaira-brain/internal/memory/security"
	"github.com/danieljustus/symaira-brain/internal/memory/web"
	"github.com/google/uuid"
)

func (s *Server) handleStatus(w http.ResponseWriter, r *http.Request) {
	w.Header().Set("Content-Type", "application/json")
	if err := json.NewEncoder(w).Encode(map[string]string{
		"status":            "healthy",
		"version":           s.version,
		"server":            "symaira-memory",
		"embedding_backend": s.service.ActiveBackend(),
	}); err != nil {
		slog.Error("failed to write status response", "error", err)
	}
}

func (s *Server) handleStats(w http.ResponseWriter, r *http.Request) {
	w.Header().Set("Content-Type", "application/json")
	if err := json.NewEncoder(w).Encode(s.Stats()); err != nil {
		slog.Error("failed to write stats response", "error", err)
	}
}

// handleTokenRevoke revokes a JWT token by its jti (or by full token value)
// so it can no longer be used to authenticate. The revocation is applied in
// memory immediately; a persistence failure is reported as a 500 with a clear
// message rather than silently swallowed, since the in-memory fallback alone
// does not survive a daemon restart.
func (s *Server) handleTokenRevoke(w http.ResponseWriter, r *http.Request) {
	if r.Method != "POST" {
		writeJSONError(w, http.StatusMethodNotAllowed, CodeMethodNotAllowed, "Method not allowed", nil)
		return
	}

	var args struct {
		Token string `json:"token"`
		JTI   string `json:"jti"`
	}
	if err := json.NewDecoder(r.Body).Decode(&args); err != nil {
		writeJSONError(w, http.StatusBadRequest, CodeInvalidRequest, "Bad request body", err)
		return
	}

	jti := strings.TrimSpace(args.JTI)
	if jti == "" && strings.TrimSpace(args.Token) != "" {
		extracted, err := security.ExtractJTI(strings.TrimSpace(args.Token))
		if err != nil {
			writeJSONError(w, http.StatusBadRequest, CodeInvalidRequest, "Invalid token value", err)
			return
		}
		jti = extracted
	}
	if jti == "" {
		writeJSONError(w, http.StatusBadRequest, CodeInvalidRequest, "either 'token' or 'jti' is required", nil)
		return
	}

	if s.jwts == nil {
		writeJSONError(w, http.StatusInternalServerError, CodeInternal, "token revocation unavailable: no JWT provider configured", nil)
		return
	}

	if err := s.jwts.RevokeToken(jti); err != nil {
		writeJSONError(w, http.StatusInternalServerError, CodeInternal, "token revoked in memory but persistence failed", err)
		return
	}

	w.Header().Set("Content-Type", "application/json")
	if err := json.NewEncoder(w).Encode(map[string]string{
		"status": "revoked",
		"jti":    jti,
	}); err != nil {
		slog.Error("failed to write revoke response", "error", err)
	}
}

func (s *Server) handleSearch(w http.ResponseWriter, r *http.Request) {
	if r.Method != "POST" {
		writeJSONError(w, http.StatusMethodNotAllowed, CodeMethodNotAllowed, "Method not allowed", nil)
		return
	}

	var args struct {
		Query             string `json:"query"`
		Scope             string `json:"scope"`
		Limit             int    `json:"limit"`
		Entity            string `json:"entity"`
		MinConfidence     string `json:"min_confidence"`
		Verification      string `json:"verification"`
		ExcludeSuperseded bool   `json:"exclude_superseded"`
		MaxAge            string `json:"max_age"`
		MaxSensitivity    string `json:"max_sensitivity"`
		MinSharingLevel   string `json:"min_sharing_level"`
		ClientID          string `json:"client_id"`
	}
	if err := json.NewDecoder(r.Body).Decode(&args); err != nil {
		writeJSONError(w, http.StatusBadRequest, CodeInvalidRequest, "Bad request body", err)
		return
	}

	if args.Limit <= 0 {
		args.Limit = 5
	}

	trustFilter := db.TrustFilter{
		MinConfidence:      args.MinConfidence,
		VerificationStatus: args.Verification,
		ExcludeSuperseded:  args.ExcludeSuperseded,
	}
	if args.MaxAge != "" {
		dur, err := parseDuration(args.MaxAge)
		if err != nil {
			writeJSONError(w, http.StatusBadRequest, CodeInvalidRequest, "invalid max_age parameter", err)
			return
		}
		trustFilter.MaxAge = dur
	}

	policyFilter := db.PolicyFilter{
		MaxSensitivity:  args.MaxSensitivity,
		MinSharingLevel: args.MinSharingLevel,
		ClientID:        args.ClientID,
	}

	results, err := s.service.Search(args.Query, args.Scope, args.Limit, args.Entity, trustFilter, policyFilter)
	if err != nil {
		if nf, ok := err.(*NotFoundError); ok {
			writeJSONError(w, http.StatusNotFound, CodeNotFound, nf.Error(), nil)
			return
		}
		writeJSONError(w, http.StatusInternalServerError, CodeInternal, "Search failed", err)
		return
	}
	security.RedactSearchResults(results)

	w.Header().Set("Content-Type", "application/json")
	if err := json.NewEncoder(w).Encode(results); err != nil {
		slog.Error("failed to write memory-list response", "error", err)
	}
}

func (s *Server) handleSet(w http.ResponseWriter, r *http.Request) {
	s.ensureAuditLogConfig()
	payload := payloadFromContext(r.Context())
	if r.Method != "POST" {
		writeJSONError(w, http.StatusMethodNotAllowed, CodeMethodNotAllowed, "Method not allowed", nil)
		return
	}

	var args struct {
		Content   string            `json:"content"`
		Scope     string            `json:"scope"`
		Metadata  map[string]string `json:"metadata"`
		SessionID string            `json:"session_id"`
		Entities  []string          `json:"entities"`
		Working   bool              `json:"working"`
	}
	if err := json.NewDecoder(r.Body).Decode(&args); err != nil {
		writeJSONError(w, http.StatusBadRequest, CodeInvalidRequest, "Bad request body", err)
		return
	}

	author := "api"
	if payload != nil && payload.Subject != "" {
		author = payload.Subject
	}

	ttl := s.workingMemoryTTL
	id, err := s.service.Set(args.Content, args.Scope, args.Metadata, args.SessionID, author, args.Entities, "http", args.Working, ttl)
	if err != nil {
		writeJSONError(w, http.StatusInternalServerError, CodeInternal, "Failed to save memory", err)
		return
	}

	w.Header().Set("Content-Type", "application/json")
	if err := json.NewEncoder(w).Encode(map[string]string{
		"status": "success",
		"id":     id,
	}); err != nil {
		slog.Error("failed to write set response", "error", err)
	}
}

func (s *Server) handleList(w http.ResponseWriter, r *http.Request) {
	scope := r.URL.Query().Get("scope")

	limit := 100
	if limitStr := r.URL.Query().Get("limit"); limitStr != "" {
		if l, err := strconv.Atoi(limitStr); err == nil {
			limit = l
		}
	}
	if limit < 1 {
		limit = 1
	}
	if limit > 1000 {
		limit = 1000
	}

	policyFilter := db.PolicyFilter{
		MaxSensitivity:  r.URL.Query().Get("max_sensitivity"),
		MinSharingLevel: r.URL.Query().Get("min_sharing_level"),
		ClientID:        r.URL.Query().Get("client_id"),
	}

	memories, err := s.service.ListWithPolicy(scope, limit, policyFilter)
	if err != nil {
		writeJSONError(w, http.StatusInternalServerError, CodeInternal, "Failed to list memories", err)
		return
	}
	security.RedactMemories(memories)

	w.Header().Set("Content-Type", "application/json")
	if err := json.NewEncoder(w).Encode(memories); err != nil {
		slog.Error("failed to write list response", "error", err)
	}
}

func (s *Server) handleSyncChanges(w http.ResponseWriter, r *http.Request) {
	if r.Method != "GET" {
		writeJSONError(w, http.StatusMethodNotAllowed, CodeMethodNotAllowed, "Method not allowed", nil)
		return
	}

	var since time.Time
	sinceStr := r.URL.Query().Get("since")
	if sinceStr != "" {
		parsed, err := time.Parse(time.RFC3339, sinceStr)
		if err != nil {
			writeJSONError(w, http.StatusBadRequest, CodeInvalidRequest, "invalid since parameter; expected RFC3339", err)
			return
		}
		since = parsed
	}

	cursorStr := r.URL.Query().Get("cursor")
	if cursorStr != "" {
		decoded, err := base64.StdEncoding.DecodeString(cursorStr)
		if err != nil {
			writeJSONError(w, http.StatusBadRequest, CodeInvalidRequest, "invalid cursor parameter", err)
			return
		}
		parsed, err := time.Parse(time.RFC3339Nano, string(decoded))
		if err != nil {
			writeJSONError(w, http.StatusBadRequest, CodeInvalidRequest, "invalid cursor format", err)
			return
		}
		since = parsed
	}

	limit := 500
	if limitStr := r.URL.Query().Get("limit"); limitStr != "" {
		if l, err := strconv.Atoi(limitStr); err == nil && l > 0 {
			limit = l
		}
	}
	if limit < 1 {
		limit = 1
	}
	if limit > 10000 {
		limit = 10000
	}

	includeEmb := r.URL.Query().Get("include_embeddings") == "true"
	memories, err := s.service.GetMemoriesSinceCursor(since, limit+1, includeEmb)
	if err != nil {
		writeJSONError(w, http.StatusInternalServerError, CodeInternal, "Failed to fetch changes", err)
		return
	}

	policyFilter := db.PolicyFilter{
		MaxSensitivity:  r.URL.Query().Get("max_sensitivity"),
		MinSharingLevel: r.URL.Query().Get("min_sharing_level"),
		ClientID:        r.URL.Query().Get("client_id"),
	}
	if policyFilter.MaxSensitivity != "" || policyFilter.MinSharingLevel != "" || policyFilter.ClientID != "" {
		var filtered []*db.Memory
		for _, m := range memories {
			if db.PassesPolicyFilter(m, policyFilter) {
				filtered = append(filtered, m)
			}
		}
		memories = filtered
	}

	var nextCursor string
	if len(memories) > limit {
		memories = memories[:limit]
		last := memories[len(memories)-1]
		nextCursor = base64.StdEncoding.EncodeToString([]byte(last.UpdatedAt.Format(time.RFC3339Nano)))
	}
	security.RedactMemories(memories)

	deleted, err := s.service.GetDeletedSince(since)
	if err != nil {
		writeJSONError(w, http.StatusInternalServerError, CodeInternal, "Failed to fetch tombstones", err)
		return
	}

	resp := map[string]interface{}{
		"memories":    memories,
		"deleted":     deleted,
		"server_time": time.Now().UTC().Format(time.RFC3339),
	}
	if nextCursor != "" {
		resp["next_cursor"] = nextCursor
	}

	w.Header().Set("Content-Type", "application/json")
	if err := json.NewEncoder(w).Encode(resp); err != nil {
		slog.Error("failed to write search response", "error", err)
	}
}

func (s *Server) handleSyncApply(w http.ResponseWriter, r *http.Request) {
	s.ensureAuditLogConfig()
	payload := payloadFromContext(r.Context())
	if r.Method != "POST" {
		writeJSONError(w, http.StatusMethodNotAllowed, CodeMethodNotAllowed, "Method not allowed", nil)
		return
	}

	var body struct {
		Memories []*db.Memory       `json:"memories"`
		Deleted  []db.DeletedMemory `json:"deleted"`
	}
	if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
		writeJSONError(w, http.StatusBadRequest, CodeInvalidRequest, "Bad request body", err)
		return
	}

	var applied, skipped, skippedInvalidScope, skippedInvalidID, deleted int
	actor := "api"
	if payload != nil && payload.Subject != "" {
		actor = payload.Subject
	}
	for _, m := range body.Memories {
		if m.ID == "" {
			skipped++
			continue
		}
		if _, err := uuid.Parse(m.ID); err != nil {
			skippedInvalidID++
			continue
		}
		if err := security.ValidateScope(m.Scope); err != nil {
			skippedInvalidScope++
			continue
		}
		isNew, err := s.service.SyncUpsertMemoryIfNewer(m)
		if err != nil {
			writeJSONError(w, http.StatusInternalServerError, CodeInternal, "Failed to apply memory", err)
			return
		}
		if isNew {
			applied++
		} else {
			skipped++
		}
	}

	for _, d := range body.Deleted {
		if d.ID == "" {
			continue
		}
		if _, err := uuid.Parse(d.ID); err != nil {
			skippedInvalidID++
			continue
		}
		removed, err := s.service.ApplyRemoteDelete(d.ID, d.DeletedAt)
		if err != nil {
			writeJSONError(w, http.StatusInternalServerError, CodeInternal, "Failed to apply delete", err)
			return
		}
		if removed {
			deleted++
		}
	}

	_ = s.service.LogAudit("sync", "", "", "", actor,
		fmt.Sprintf("applied=%d skipped=%d deleted=%d invalidScope=%d invalidID=%d", applied, skipped, deleted, skippedInvalidScope, skippedInvalidID))

	w.Header().Set("Content-Type", "application/json")
	if err := json.NewEncoder(w).Encode(map[string]int{
		"applied":             applied,
		"skipped":             skipped,
		"deleted":             deleted,
		"skippedInvalidScope": skippedInvalidScope,
		"skippedInvalidID":    skippedInvalidID,
	}); err != nil {
		slog.Error("failed to write sync-apply response", "error", err)
	}
}

// handleSyncRelay stores and serves opaque, client-side-encrypted sync blobs.
// The relay never inspects payloads: it applies last-writer-wins on the
// caller-supplied updated_at and returns ciphertext as-is.
func (s *Server) handleSyncRelay(w http.ResponseWriter, r *http.Request) {
	switch r.Method {
	case "GET":
		var since time.Time
		if sinceStr := r.URL.Query().Get("since"); sinceStr != "" {
			parsed, err := time.Parse(time.RFC3339Nano, sinceStr)
			if err != nil {
				writeJSONError(w, http.StatusBadRequest, CodeInvalidRequest, "invalid since parameter; expected RFC3339", err)
				return
			}
			since = parsed
		}
		limit := 500
		if limitStr := r.URL.Query().Get("limit"); limitStr != "" {
			if l, err := strconv.Atoi(limitStr); err == nil && l > 0 && l <= 10000 {
				limit = l
			}
		}
		blobs, err := s.service.GetRelayBlobsSince(since, limit)
		if err != nil {
			writeJSONError(w, http.StatusInternalServerError, CodeInternal, "Failed to fetch relay blobs", err)
			return
		}
		w.Header().Set("Content-Type", "application/json")
		if err := json.NewEncoder(w).Encode(map[string]interface{}{
			"blobs":       blobs,
			"server_time": time.Now().UTC().Format(time.RFC3339Nano),
		}); err != nil {
			slog.Error("failed to write sync-relay response", "error", err)
		}
	case "POST":
		var body struct {
			Blobs []db.RelayBlob `json:"blobs"`
		}
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			writeJSONError(w, http.StatusBadRequest, CodeInvalidRequest, "Bad request body", err)
			return
		}
		var stored, skipped int
		for _, b := range body.Blobs {
			if b.ID == "" || len(b.Blob) == 0 {
				skipped++
				continue
			}
			ok, err := s.service.StoreRelayBlob(b)
			if err != nil {
				writeJSONError(w, http.StatusInternalServerError, CodeInternal, "Failed to store relay blob", err)
				return
			}
			if ok {
				stored++
			} else {
				skipped++
			}
		}
		w.Header().Set("Content-Type", "application/json")
		if err := json.NewEncoder(w).Encode(map[string]int{"stored": stored, "skipped": skipped}); err != nil {
			slog.Error("failed to write sync-relay response", "error", err)
		}
	default:
		writeJSONError(w, http.StatusMethodNotAllowed, CodeMethodNotAllowed, "Method not allowed", nil)
	}
}

func (s *Server) handleGet(w http.ResponseWriter, r *http.Request) {
	if r.Method != "GET" {
		writeJSONError(w, http.StatusMethodNotAllowed, CodeMethodNotAllowed, "Method not allowed", nil)
		return
	}

	id := r.URL.Query().Get("id")
	if id == "" {
		writeJSONError(w, http.StatusBadRequest, CodeInvalidRequest, "missing required parameter: id", nil)
		return
	}

	m, err := s.service.Get(id)
	if err != nil {
		if nf, ok := err.(*NotFoundError); ok {
			writeJSONError(w, http.StatusNotFound, CodeNotFound, nf.Error(), nil)
			return
		}
		writeJSONError(w, http.StatusInternalServerError, CodeInternal, "failed to fetch memory", err)
		return
	}

	clientID := r.URL.Query().Get("client_id")
	if clientID != "" {
		policyFilter := db.PolicyFilter{ClientID: clientID}
		if !db.PassesPolicyFilter(m, policyFilter) {
			writeJSONError(w, http.StatusForbidden, CodeForbidden, fmt.Sprintf("access denied: memory %s is not accessible by client %s", id, clientID), nil)
			return
		}
	}
	security.RedactMemory(m)

	w.Header().Set("Content-Type", "application/json")
	_ = json.NewEncoder(w).Encode(m)
}

func (s *Server) handleDelete(w http.ResponseWriter, r *http.Request) {
	s.ensureAuditLogConfig()
	if r.Method != "DELETE" && r.Method != "POST" {
		writeJSONError(w, http.StatusMethodNotAllowed, CodeMethodNotAllowed, "Method not allowed", nil)
		return
	}

	id := r.URL.Query().Get("id")
	if id == "" {
		writeJSONError(w, http.StatusBadRequest, CodeInvalidRequest, "missing required parameter: id", nil)
		return
	}

	err := s.service.Delete(id)
	if err != nil {
		if nf, ok := err.(*NotFoundError); ok {
			writeJSONError(w, http.StatusNotFound, CodeNotFound, nf.Error(), nil)
			return
		}
		writeJSONError(w, http.StatusInternalServerError, CodeInternal, "failed to delete memory", err)
		return
	}

	w.Header().Set("Content-Type", "application/json")
	_ = json.NewEncoder(w).Encode(map[string]bool{"deleted": true})
}

func (s *Server) handleRules(w http.ResponseWriter, r *http.Request) {
	if r.Method != "GET" {
		writeJSONError(w, http.StatusMethodNotAllowed, CodeMethodNotAllowed, "Method not allowed", nil)
		return
	}

	scope := r.URL.Query().Get("scope")
	rules, err := s.service.ListRules(scope)
	if err != nil {
		writeJSONError(w, http.StatusInternalServerError, CodeInternal, "failed to list rules", err)
		return
	}

	w.Header().Set("Content-Type", "application/json")
	_ = json.NewEncoder(w).Encode(map[string]interface{}{"rules": rules})
}

func (s *Server) handleEntities(w http.ResponseWriter, r *http.Request) {
	if r.Method != "GET" {
		writeJSONError(w, http.StatusMethodNotAllowed, CodeMethodNotAllowed, "Method not allowed", nil)
		return
	}

	entities, err := s.service.ListEntities()
	if err != nil {
		writeJSONError(w, http.StatusInternalServerError, CodeInternal, "failed to list entities", err)
		return
	}

	w.Header().Set("Content-Type", "application/json")
	_ = json.NewEncoder(w).Encode(map[string]interface{}{"entities": entities})
}

// ensureAuditLogConfig mirrors the audit_log_enabled config value onto the
// database audit gate so every MCP/HTTP mutation path honors it. It is
// idempotent and safe to call on each mutation request.
func (s *Server) ensureAuditLogConfig() {
	if s.cfg != nil {
		s.service.db.SetAuditLogEnabled(s.cfg.Retention.AuditLogEnabled)
	}
}

func (s *Server) handleStatic(w http.ResponseWriter, r *http.Request) {
	if r.URL.Path == "/" {
		w.Header().Set("Content-Type", "text/html; charset=utf-8")
		if _, err := w.Write(web.IndexHTML()); err != nil {
			slog.Error("failed to write index page", "error", err)
		}
		return
	}
	fileServer := http.FileServer(http.FS(web.StaticFS()))
	fileServer.ServeHTTP(w, r)
}
